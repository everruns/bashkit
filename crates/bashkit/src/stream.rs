//! Byte-native shell stream values.
//!
//! Important decision: bytes are canonical. Text decoding only happens at an
//! explicit consumer boundary; invalid UTF-8 is never rewritten in transport.

use std::borrow::Cow;
use std::fmt;

/// Owned data carried by a shell byte stream.
#[derive(Clone, Default, Eq, PartialEq)]
pub struct StreamData {
    bytes: Vec<u8>,
    text: String,
}

impl StreamData {
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
    fn append_redecodes_utf8_split_across_chunks() {
        let mut stream = StreamData::from(vec![0xc3]);
        stream.append(&StreamData::from(vec![0xa9]));

        assert_eq!(stream.as_bytes(), "é".as_bytes());
        assert_eq!(stream.text_lossy(), "é");
    }
}
