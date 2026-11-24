//! Byte-native shell stream values.
//!
//! Important decision: bytes are canonical. Text decoding only happens at an
//! explicit consumer boundary; invalid UTF-8 is never rewritten in transport.

use std::borrow::Cow;
use std::fmt;

/// Leases for a materialized stream's byte storage and cached text storage.
pub(crate) type StreamStorage = [Option<crate::limits::ExecutionBudgetLease>; 2];

/// Owned data carried by a shell byte stream.
#[derive(Default)]
pub struct StreamData {
    bytes: Vec<u8>,
    text: String,
    storage: Option<Box<StreamStorage>>,
    truncated: bool,
}

impl Clone for StreamData {
    fn clone(&self) -> Self {
        Self {
            bytes: self.bytes.clone(),
            text: self.text.clone(),
            truncated: self.truncated,
            storage: None,
        }
    }
}

impl PartialEq for StreamData {
    fn eq(&self, other: &Self) -> bool {
        self.bytes == other.bytes
    }
}

impl Eq for StreamData {}

impl StreamData {
    /// Decode a VFS buffer whose byte capacity is already leased by its caller.
    pub(crate) fn from_admitted_bytes(
        bytes: Vec<u8>,
        budget: &crate::limits::ExecutionBudget,
    ) -> Result<(Self, Option<crate::limits::ExecutionBudgetLease>), crate::limits::LimitExceeded>
    {
        let mut text = crate::limits::BudgetedString::new(Some(budget))?;
        for chunk in bytes.utf8_chunks() {
            budget.consume_work(1 + chunk.valid().len().div_ceil(1024) as u64)?;
            budget.consume_input(chunk.valid().len())?;
            text.try_push_str(chunk.valid())?;
            if !chunk.invalid().is_empty() {
                budget.consume_input('\u{fffd}'.len_utf8())?;
                text.try_push('\u{fffd}')?;
            }
        }
        let (text, storage) = text.into_parts();
        Ok((
            Self {
                bytes,
                text,
                ..Self::new()
            },
            storage,
        ))
    }

    /// Copy both representations through the existing capacity-owning builders.
    pub(crate) fn try_clone_budgeted(
        &self,
        budget: &crate::limits::ExecutionBudget,
    ) -> Result<(Self, StreamStorage), crate::limits::LimitExceeded> {
        budget.consume_work(1 + self.retained_len().div_ceil(1024) as u64)?;
        let mut bytes = crate::limits::BudgetedBytes::new(Some(budget))?;
        let mut text = crate::limits::BudgetedString::new(Some(budget))?;
        bytes.try_extend_from_slice(&self.bytes)?;
        text.try_push_str(&self.text)?;
        let (bytes, byte_storage) = bytes.into_parts();
        let (text, text_storage) = text.into_parts();
        Ok((
            Self {
                bytes,
                text,
                ..Self::new()
            },
            [byte_storage, text_storage],
        ))
    }

    /// Convert admitted text to the byte/text stream without an uncharged copy.
    pub(crate) fn from_budgeted_string(
        text: crate::limits::BudgetedString,
        budget: Option<&crate::limits::ExecutionBudget>,
    ) -> Result<(Self, StreamStorage), crate::limits::LimitExceeded> {
        let mut bytes = crate::limits::BudgetedBytes::new(budget)?;
        if let Some(budget) = budget {
            budget.consume_work(1 + text.len() as u64)?;
        }
        bytes.try_extend_from_slice(text.as_bytes())?;
        let (bytes, byte_storage) = bytes.into_parts();
        let (text, text_storage) = text.into_parts();
        Ok((
            Self {
                bytes,
                text,
                ..Self::new()
            },
            [byte_storage, text_storage],
        ))
    }

    /// Materialize cached text without releasing the byte buffer's lease or
    /// allocating a temporary lossy string outside the request budget.
    // THREAT[TM-DOS-103]: both representations must be admitted before growth.
    pub(crate) fn from_budgeted_bytes(
        bytes: crate::limits::BudgetedBytes,
        budget: &crate::limits::ExecutionBudget,
    ) -> Result<(Self, StreamStorage), crate::limits::LimitExceeded> {
        let mut text = crate::limits::BudgetedString::new(Some(budget))?;
        for chunk in bytes.utf8_chunks() {
            budget.consume_work(1 + chunk.valid().len().div_ceil(1024) as u64)?;
            text.try_push_str(chunk.valid())?;
            if !chunk.invalid().is_empty() {
                text.try_push('\u{fffd}')?;
            }
        }
        let (bytes, byte_storage) = bytes.into_parts();
        let (text, text_storage) = text.into_parts();
        Ok((
            Self {
                bytes,
                text,
                ..Self::new()
            },
            [byte_storage, text_storage],
        ))
    }

    pub const fn new() -> Self {
        Self {
            bytes: Vec::new(),
            text: String::new(),
            storage: None,
            truncated: false,
        }
    }
    pub fn as_bytes(&self) -> &[u8] {
        &self.bytes
    }
    pub fn into_bytes(self) -> Vec<u8> {
        self.bytes
    }
    pub fn text(&self) -> Result<&str, std::str::Utf8Error> {
        std::str::from_utf8(&self.bytes)
    }
    pub fn text_lossy(&self) -> Cow<'_, str> {
        Cow::Borrowed(&self.text)
    }
    pub fn len(&self) -> usize {
        self.bytes.len()
    }

    /// Live bytes retained by this buffer: the raw bytes plus the decoded
    /// text kept alongside them.
    pub(crate) fn retained_len(&self) -> usize {
        self.bytes.len().saturating_add(self.text.len())
    }
    pub fn is_empty(&self) -> bool {
        self.bytes.is_empty()
    }
    pub(crate) fn was_truncated(&self) -> bool {
        self.truncated
    }

    pub(crate) fn admit_storage(
        &mut self,
        budget: &crate::limits::ExecutionBudget,
    ) -> Result<(), crate::limits::LimitExceeded> {
        if self.storage.is_none() && !self.is_empty() {
            self.storage = Some(Box::new([
                Some(budget.lease_bytes(self.bytes.capacity())?),
                Some(budget.lease_bytes(self.text.capacity())?),
            ]));
        }
        Ok(())
    }

    /// Admit both retained representations before growing a compound result.
    // THREAT[TM-DOS-103]: retain the capacity leases as the stream moves out of
    // its accumulator. Only captured output gets a byte ceiling, since shell
    // consumers such as redirects must receive the original bytes.
    pub(crate) fn try_append(
        &mut self,
        other: &Self,
        limit: usize,
        budget: &crate::limits::ExecutionBudget,
    ) -> Result<bool, crate::limits::LimitExceeded> {
        use crate::limits::{BudgetedBytes, BudgetedString};

        let added = other.len().min(limit.saturating_sub(self.len()));
        self.truncated |= added < other.len() || other.truncated;
        if added == 0 {
            return Ok(self.truncated);
        }

        let boundary = self.len();
        let mut storage = self
            .storage
            .take()
            .unwrap_or_else(|| Box::new([None, None]));
        let [raw_lease, text_lease] = std::mem::take(&mut *storage);
        let cached = std::mem::take(&mut self.text);
        let mut bytes = BudgetedBytes::adopt(std::mem::take(&mut self.bytes), raw_lease, budget)?;
        let mut text = BudgetedString::adopt(cached, text_lease, budget)?;
        bytes.try_extend_from_slice_with_limit(&other.bytes[..added], limit)?;
        if lossy_decode_changes_at_boundary(&bytes, boundary) {
            drop(text);
            let (stream, leases) = Self::from_budgeted_bytes(bytes, budget)?;
            self.bytes = stream.bytes;
            self.text = stream.text;
            *storage = leases;
        } else {
            if added == other.len() {
                text.try_push_str(&other.text)?;
            } else {
                for chunk in other.bytes[..added].utf8_chunks() {
                    text.try_push_str(chunk.valid())?;
                    if !chunk.invalid().is_empty() {
                        text.try_push('\u{fffd}')?;
                    }
                }
            }
            let (raw, raw_lease) = bytes.into_parts();
            let (cached, text_lease) = text.into_parts();
            self.bytes = raw;
            self.text = cached;
            *storage = [raw_lease, text_lease];
        }
        self.storage = Some(storage);
        Ok(self.truncated)
    }

    pub(crate) fn append(&mut self, other: &Self) {
        self.storage = None;
        self.truncated |= other.truncated;
        let boundary = self.bytes.len();
        self.bytes.extend_from_slice(&other.bytes);
        if lossy_decode_changes_at_boundary(&self.bytes, boundary) {
            self.refresh_text();
        } else {
            self.text.push_str(&other.text);
        }
    }
    pub(crate) fn append_text(&mut self, text: &str) {
        // Valid UTF-8 starts at a scalar boundary, so it cannot complete an
        // incomplete scalar at the end of the existing byte buffer.
        self.bytes.extend_from_slice(text.as_bytes());
        self.text.push_str(text);
    }
    pub(crate) fn push_str(&mut self, text: &str) {
        self.append_text(text);
    }
    pub(crate) fn push_byte(&mut self, byte: u8) {
        let boundary = self.bytes.len();
        self.bytes.push(byte);
        if lossy_decode_changes_at_boundary(&self.bytes, boundary) {
            self.refresh_text();
        } else {
            self.text.push(if byte.is_ascii() {
                char::from(byte)
            } else {
                '\u{fffd}'
            });
        }
    }
    pub(crate) fn prefix(&self, limit: usize) -> Self {
        Self::from(self.bytes[..self.bytes.len().min(limit)].to_vec())
    }
    /// The bytes after the first `start`.
    pub(crate) fn suffix_from(&self, start: usize) -> Self {
        Self::from(self.bytes[self.bytes.len().min(start)..].to_vec())
    }
    /// Shell variables cannot contain NUL. This is the command-substitution text boundary.
    pub(crate) fn command_substitution_text(&self) -> String {
        let bytes: Vec<u8> = self
            .bytes
            .iter()
            .copied()
            .filter(|byte| *byte != 0)
            .collect();
        String::from_utf8_lossy(&bytes).into_owned()
    }
    fn refresh_text(&mut self) {
        self.text = String::from_utf8_lossy(&self.bytes).into_owned();
    }
}

fn lossy_decode_changes_at_boundary(bytes: &[u8], boundary: usize) -> bool {
    // A scalar can span the join only when the new chunk begins with a
    // continuation byte. Empty chunks and ordinary text need no decoding.
    if boundary == 0 || boundary == bytes.len() || bytes[boundary] & 0xc0 != 0x80 {
        return false;
    }
    let start = boundary.saturating_sub(3);
    let end = bytes.len().min(boundary.saturating_add(3));
    let mut split = String::from_utf8_lossy(&bytes[start..boundary]).into_owned();
    split.push_str(&String::from_utf8_lossy(&bytes[boundary..end]));
    split != String::from_utf8_lossy(&bytes[start..end])
}

impl fmt::Debug for StreamData {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("StreamData")
            .field("bytes", &self.bytes)
            .field("text", &self.text)
            .finish()
    }
}
impl fmt::Display for StreamData {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.text_lossy())
    }
}
impl From<Vec<u8>> for StreamData {
    fn from(bytes: Vec<u8>) -> Self {
        let text = String::from_utf8_lossy(&bytes).into_owned();
        Self {
            bytes,
            text,
            ..Self::new()
        }
    }
}
impl From<String> for StreamData {
    fn from(text: String) -> Self {
        Self {
            bytes: text.as_bytes().to_vec(),
            text,
            ..Self::new()
        }
    }
}
impl From<&str> for StreamData {
    fn from(value: &str) -> Self {
        value.to_owned().into()
    }
}
impl From<&[u8]> for StreamData {
    fn from(value: &[u8]) -> Self {
        value.to_vec().into()
    }
}
impl From<&StreamData> for String {
    fn from(value: &StreamData) -> Self {
        value.text.clone()
    }
}
impl PartialEq<str> for StreamData {
    fn eq(&self, other: &str) -> bool {
        self.bytes == other.as_bytes()
    }
}
impl PartialEq<&str> for StreamData {
    fn eq(&self, other: &&str) -> bool {
        self.bytes == other.as_bytes()
    }
}
impl PartialEq<String> for StreamData {
    fn eq(&self, other: &String) -> bool {
        self.bytes == other.as_bytes()
    }
}
impl PartialEq<StreamData> for String {
    fn eq(&self, other: &StreamData) -> bool {
        self.as_bytes() == other.bytes
    }
}
impl PartialEq<StreamData> for str {
    fn eq(&self, other: &StreamData) -> bool {
        self.as_bytes() == other.bytes
    }
}

impl std::ops::Deref for StreamData {
    type Target = str;
    fn deref(&self) -> &Self::Target {
        &self.text
    }
}

impl std::ops::Add<&StreamData> for StreamData {
    type Output = StreamData;
    fn add(mut self, rhs: &StreamData) -> Self::Output {
        self.append(rhs);
        self
    }
}

impl serde::Serialize for StreamData {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        serializer.serialize_str(&self.text)
    }
}

#[cfg(test)]
mod tests {
    use super::StreamData;
    use proptest::prelude::*;

    #[test]
    fn bounded_append_preserves_prefix_and_releases_storage() {
        let budget = crate::limits::ExecutionBudget::new(
            &crate::ExecutionLimits::new().max_live_intermediate_bytes(32),
            std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false)),
        );
        let mut stream = StreamData::new();
        assert!(
            !stream
                .try_append(&StreamData::from(&[0xc3][..]), 2, &budget)
                .unwrap()
        );
        assert!(
            stream
                .try_append(&StreamData::from(&[0xa9, b'x'][..]), 2, &budget)
                .unwrap()
        );
        assert_eq!(stream.text_lossy(), "é");
        assert!(budget.live_bytes_for_test() >= 4);
        drop(stream);
        assert_eq!(budget.live_bytes_for_test(), 0);
    }

    #[test]
    fn bounded_append_charges_cached_text_before_growth() {
        let budget = crate::limits::ExecutionBudget::new(
            &crate::ExecutionLimits::new().max_live_intermediate_bytes(3),
            std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false)),
        );
        let mut stream = StreamData::new();
        assert!(
            stream
                .try_append(&StreamData::from(&[0xff][..]), 1, &budget)
                .is_err()
        );
        assert!(stream.is_empty());
        assert_eq!(budget.live_bytes_for_test(), 0);
    }

    proptest! {
        #[test]
        fn bounded_append_decodes_the_retained_bytes(
            left in proptest::collection::vec(any::<u8>(), 0..32),
            right in proptest::collection::vec(any::<u8>(), 0..32),
            room in 0..40usize,
        ) {
            let budget = crate::limits::ExecutionBudget::new(
                &crate::ExecutionLimits::new(),
                std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false)),
            );
            let limit = left.len() + room;
            let mut stream = StreamData::from(left.clone());
            let truncated = stream.try_append(&StreamData::from(right.clone()), limit, &budget).unwrap();
            let prefix: Vec<u8> = left.into_iter().chain(right.iter().copied()).take(limit).collect();
            prop_assert_eq!(truncated, right.len() > room);
            prop_assert_eq!(stream.as_bytes(), prefix.as_slice());
            prop_assert_eq!(stream.text_lossy(), String::from_utf8_lossy(&prefix));
        }

        #[test]
        fn appending_chunks_preserves_the_combined_lossy_view(
            left in proptest::collection::vec(any::<u8>(), 0..64),
            right in proptest::collection::vec(any::<u8>(), 0..64),
        ) {
            let mut stream = StreamData::from(left.clone());
            stream.append(&StreamData::from(right.clone()));
            let joined: Vec<u8> = left.into_iter().chain(right).collect();
            prop_assert_eq!(stream.as_bytes(), joined.as_slice());
            prop_assert_eq!(stream.text_lossy(), String::from_utf8_lossy(&joined));
        }

        #[test]
        fn appending_text_and_individual_bytes_preserves_decoding(
            left in proptest::collection::vec(any::<u8>(), 0..64),
            right in any::<String>(),
            byte in any::<u8>(),
        ) {
            let mut stream = StreamData::from(left.clone());
            stream.append_text(&right);
            stream.push_byte(byte);
            let mut joined = left;
            joined.extend_from_slice(right.as_bytes());
            joined.push(byte);
            prop_assert_eq!(stream.as_bytes(), joined.as_slice());
            prop_assert_eq!(stream.text_lossy(), String::from_utf8_lossy(&joined));
        }
    }

    #[test]
    fn admitted_bytes_preserve_lossy_text_and_lease() {
        let budget = crate::limits::ExecutionBudget::new(
            &crate::ExecutionLimits::new(),
            std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false)),
        );
        let bytes = b"a\xff\0".to_vec();
        let raw = budget.lease_bytes(bytes.capacity()).unwrap();
        let (stream, text) = StreamData::from_admitted_bytes(bytes, &budget).unwrap();
        assert_eq!(stream.as_bytes(), b"a\xff\0");
        assert_eq!(stream.text_lossy(), "a\u{fffd}\0");
        assert!(budget.live_bytes_for_test() >= 8);
        drop((stream, raw, text));
        assert_eq!(budget.live_bytes_for_test(), 0);
    }

    #[test]
    fn budgeted_clone_exact_boundary_and_refund() {
        for (limit, succeeds) in [(3, false), (4, true)] {
            let budget = crate::limits::ExecutionBudget::new(
                &crate::ExecutionLimits::new().max_live_intermediate_bytes(limit),
                std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false)),
            );
            let source = StreamData::from("ab");
            let result = source.try_clone_budgeted(&budget);
            if succeeds {
                let (copy, storage) = result.unwrap();
                assert_eq!(copy, source);
                assert_eq!(budget.live_bytes_for_test(), 4);
                drop((copy, storage));
            } else {
                assert!(result.is_err());
            }
            assert_eq!(budget.live_bytes_for_test(), 0);
        }
    }

    #[test]
    fn budgeted_string_stream_admits_both_representations() {
        for limit in [7, 8] {
            let budget = crate::limits::ExecutionBudget::new(
                &crate::ExecutionLimits::new().max_live_intermediate_bytes(limit),
                std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false)),
            );
            let mut text = crate::limits::BudgetedString::new(Some(&budget)).unwrap();
            text.try_push_str("text").unwrap();
            let result = StreamData::from_budgeted_string(text, Some(&budget));
            if limit == 7 {
                assert!(
                    result
                        .unwrap_err()
                        .to_string()
                        .contains("live intermediate")
                );
            } else {
                let (stream, storage) = result.unwrap();
                assert_eq!(stream.as_bytes(), b"text");
                assert_eq!(stream.text_lossy(), "text");
                assert!(budget.lease_bytes(1).is_err());
                drop(storage);
            }
        }
    }

    #[test]
    fn budgeted_text_preserves_bytes_and_lossy_decoding() {
        for input in [
            b"plain".as_slice(),
            "é".as_bytes(),
            b"a\xff\xc3z\0",
            b"\xf0\x90\x80",
        ] {
            let budget = crate::limits::ExecutionBudget::new(
                &crate::limits::ExecutionLimits::default(),
                std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false)),
            );
            let mut bytes = crate::limits::BudgetedBytes::new(Some(&budget)).unwrap();
            bytes.try_extend_from_slice(input).unwrap();
            let (stream, _storage) = StreamData::from_budgeted_bytes(bytes, &budget).unwrap();
            assert_eq!(stream.as_bytes(), input);
            assert_eq!(stream.text_lossy(), String::from_utf8_lossy(input));
        }
    }

    #[test]
    fn budgeted_text_copy_counts_against_live_memory() {
        let limits = crate::limits::ExecutionLimits::new().max_live_intermediate_bytes(7);
        let budget = crate::limits::ExecutionBudget::new(
            &limits,
            std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false)),
        );
        let mut bytes = crate::limits::BudgetedBytes::new(Some(&budget)).unwrap();
        bytes.try_extend_from_slice(b"text").unwrap();
        let error = StreamData::from_budgeted_bytes(bytes, &budget).unwrap_err();
        assert!(error.to_string().contains("live intermediate"), "{error}");
    }

    #[test]
    fn append_redecodes_utf8_split_across_chunks() {
        let mut stream = StreamData::from(vec![0xc3]);
        stream.append(&StreamData::from(vec![0xa9]));

        assert_eq!(stream.as_bytes(), "é".as_bytes());
        assert_eq!(stream.text_lossy(), "é");
    }

    #[test]
    fn append_handles_all_splits_of_valid_and_invalid_utf8() {
        for bytes in [
            "aé€🦀z".as_bytes(),
            b"\xf0\x90\x80",
            b"\xe2\x82x\xff\xc3\xa9",
            b"\xed\xa0\x80\0\xf4\x90\x80\x80",
        ] {
            for split in 0..=bytes.len() {
                let mut stream = StreamData::from(&bytes[..split]);
                stream.append(&StreamData::from(&bytes[split..]));
                assert_eq!(stream.text_lossy(), String::from_utf8_lossy(bytes));
            }
            let mut stream = StreamData::new();
            for byte in bytes {
                stream.push_byte(*byte);
            }
            assert_eq!(stream.as_bytes(), bytes);
            assert_eq!(stream.text_lossy(), String::from_utf8_lossy(bytes));
        }
    }
}
