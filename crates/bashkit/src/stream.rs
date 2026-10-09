//! Byte-native shell stream values.
//!
//! Important decision: bytes are canonical. Text decoding only happens at an
//! explicit consumer boundary; invalid UTF-8 is never rewritten in transport.

use std::borrow::Cow;
use std::fmt;

/// Leases for a materialized stream's byte storage and cached text storage.
pub(crate) type StreamStorage = [Option<crate::limits::ExecutionBudgetLease>; 2];

/// Owned data carried by a shell byte stream.
#[derive(Clone, Default, Eq, PartialEq)]
pub struct StreamData {
    bytes: Vec<u8>,
    text: String,
}

impl StreamData {
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
        Ok((Self { bytes, text }, [byte_storage, text_storage]))
    }

    pub const fn new() -> Self {
        Self {
            bytes: Vec::new(),
            text: String::new(),
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
    pub fn is_empty(&self) -> bool {
        self.bytes.is_empty()
    }
    pub(crate) fn append(&mut self, other: &Self) {
        let boundary = self.bytes.len();
        self.bytes.extend_from_slice(&other.bytes);
        if lossy_decode_changes_at_boundary(&self.bytes, boundary) {
            self.refresh_text();
        } else {
            self.text.push_str(&other.text);
        }
    }
    pub(crate) fn append_text(&mut self, text: &str) {
        self.append(&Self::from(text));
    }
    pub(crate) fn push_str(&mut self, text: &str) {
        self.append_text(text);
    }
    pub(crate) fn push_byte(&mut self, byte: u8) {
        self.append(&Self::from(vec![byte]));
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
        Self { bytes, text }
    }
}
impl From<String> for StreamData {
    fn from(text: String) -> Self {
        Self {
            bytes: text.as_bytes().to_vec(),
            text,
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
}
