// Decision: object IDs are SHA-256 over `[kind byte][payload]`, where the
// payload is the *uncompressed* canonical encoding. Storage may compress an
// object freely without changing its identity, so dedup is content-based and a
// host can re-compress its store without invalidating every reference.
//
// Decision: structural objects (chunk, file, tree) use a hand-rolled binary
// encoding because they carry raw file bytes, which JSON cannot hold without
// inflation. Descriptive objects (shell, caps, commit) use canonical JSON with
// sorted keys, because they need field-level forward compatibility — an unknown
// field must be ignorable, which serde gives us and a positional binary format
// does not.

//! The content-addressed object graph behind snapshot history.
//!
//! | Kind | Holds | Encoding |
//! |------|-------|----------|
//! | Chunk | One content-defined slice of file content | raw bytes |
//! | File | Inline content or an ordered chunk list | binary |
//! | Tree | Every VFS entry, sorted by path | binary |
//! | Shell | Serialized [`ShellState`](crate::interpreter::ShellState) | canonical JSON |
//! | Caps | Capability fingerprint of the producing instance | canonical JSON |
//! | Commit | Parents, tree, shell, caps, counters, host metadata | canonical JSON |

use std::collections::BTreeMap;
use std::fmt;
use std::path::PathBuf;

use sha2::{Digest, Sha256};

use super::chunker;
use crate::fs::{VfsEntry, VfsEntryKind, VfsSnapshot};

pub(crate) const KIND_CHUNK: u8 = 1;
pub(crate) const KIND_FILE: u8 = 2;
pub(crate) const KIND_TREE: u8 = 3;
pub(crate) const KIND_SHELL: u8 = 4;
pub(crate) const KIND_CAPS: u8 = 5;
pub(crate) const KIND_COMMIT: u8 = 6;

/// Upper bound on entries in one tree object, so a malformed tree cannot make
/// the decoder pre-allocate unbounded memory (TM-SNAP-005).
const MAX_TREE_ENTRIES: u32 = 10_000_000;
/// Upper bound on chunk references in one file manifest.
const MAX_FILE_CHUNKS: u32 = 10_000_000;
/// Upper bound on parents recorded by one commit.
const MAX_PARENTS: usize = 64;

fn malformed(what: &str) -> crate::Error {
    crate::Error::Internal(format!("malformed snapshot object: {what}"))
}

/// Decode one ASCII hex digit. Rejects every non-ASCII byte, so a multi-byte
/// character can never reach the arithmetic below.
fn hex_nibble(byte: u8) -> crate::Result<u8> {
    match byte {
        b'0'..=b'9' => Ok(byte - b'0'),
        b'a'..=b'f' => Ok(byte - b'a' + 10),
        b'A'..=b'F' => Ok(byte - b'A' + 10),
        _ => Err(malformed("object id contains non-hex characters")),
    }
}

/// Content address of a snapshot object: SHA-256 of `[kind][payload]`.
///
/// Displays and parses as lowercase hex. A [`CommitId`] is the same type,
/// named for the role it plays as the root pointer a host persists.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ObjectId([u8; 32]);

/// The [`ObjectId`] of a commit object — the value a host stores per message.
pub type CommitId = ObjectId;

impl ObjectId {
    fn of(kind: u8, payload: &[u8]) -> Self {
        let mut hasher = Sha256::new();
        hasher.update([kind]);
        hasher.update(payload);
        let mut out = [0u8; 32];
        out.copy_from_slice(&hasher.finalize());
        Self(out)
    }

    /// Raw 32-byte digest.
    pub fn as_bytes(&self) -> &[u8; 32] {
        &self.0
    }

    /// Build from a raw 32-byte digest.
    pub fn from_bytes(bytes: [u8; 32]) -> Self {
        Self(bytes)
    }

    /// Lowercase hex form.
    pub fn to_hex(self) -> String {
        self.0.iter().map(|b| format!("{b:02x}")).collect()
    }

    /// Parse from lowercase or uppercase hex.
    ///
    /// Decodes byte pairs directly rather than slicing the `str`. Object ids
    /// arrive from callers in three languages, and `s.len()` counts bytes: a
    /// 64-*byte* string holding any multi-byte character would otherwise be
    /// sliced mid-character and panic instead of returning an error.
    pub fn from_hex(s: &str) -> crate::Result<Self> {
        let bytes = s.as_bytes();
        if bytes.len() != 64 {
            return Err(malformed("object id must be 64 hex characters"));
        }
        let mut out = [0u8; 32];
        for (byte, pair) in out.iter_mut().zip(bytes.chunks_exact(2)) {
            let (hi, lo) = (hex_nibble(pair[0])?, hex_nibble(pair[1])?);
            *byte = (hi << 4) | lo;
        }
        Ok(Self(out))
    }
}

impl fmt::Display for ObjectId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.to_hex())
    }
}

// TM-INF-022: object IDs surface in restore diagnostics, so Debug must not
// expose a different (array-shaped) rendering than Display.
impl fmt::Debug for ObjectId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.to_hex())
    }
}

impl serde::Serialize for ObjectId {
    fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(&self.to_hex())
    }
}

impl<'de> serde::Deserialize<'de> for ObjectId {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let s = String::deserialize(d)?;
        Self::from_hex(&s).map_err(serde::de::Error::custom)
    }
}

/// A decoded object plus the bytes it hashes over.
pub(crate) struct Encoded {
    pub id: ObjectId,
    pub kind: u8,
    pub payload: Vec<u8>,
}

impl Encoded {
    pub(crate) fn new(kind: u8, payload: Vec<u8>) -> Self {
        Self {
            id: ObjectId::of(kind, &payload),
            kind,
            payload,
        }
    }

    /// Storage form: `[kind][payload]`, which is exactly what the ID covers.
    ///
    /// The commit path builds this fused with compression; this spelling is
    /// what the round-trip tests check that fused encoder against.
    #[cfg(test)]
    pub(crate) fn to_storage(&self) -> Vec<u8> {
        let mut out = Vec::with_capacity(1 + self.payload.len());
        out.push(self.kind);
        out.extend_from_slice(&self.payload);
        out
    }

    /// Parse a stored object and verify it hashes to `expected`.
    pub(crate) fn from_storage(expected: ObjectId, bytes: &[u8]) -> crate::Result<Self> {
        let (kind, payload) = bytes
            .split_first()
            .ok_or_else(|| malformed("empty object"))?;
        let id = ObjectId::of(*kind, payload);
        // TM-SNAP-002: the store is untrusted. An object only counts if it
        // hashes to the ID we asked for, which is what makes signing the root
        // commit sufficient to cover the whole graph.
        if id != expected {
            return Err(crate::Error::Internal(format!(
                "snapshot object {expected} does not match its content hash {id}"
            )));
        }
        Ok(Self {
            id,
            kind: *kind,
            payload: payload.to_vec(),
        })
    }

    pub(crate) fn expect_kind(&self, kind: u8) -> crate::Result<()> {
        // TM-SNAP-005: reject type confusion (a chunk served where a tree is
        // expected) rather than letting the decoder misread the payload.
        if self.kind != kind {
            return Err(crate::Error::Internal(format!(
                "snapshot object {} has kind {} but kind {} was expected",
                self.id, self.kind, kind
            )));
        }
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// Binary encoding helpers
// ---------------------------------------------------------------------------

fn put_u32(out: &mut Vec<u8>, v: u32) {
    out.extend_from_slice(&v.to_le_bytes());
}

fn put_u64(out: &mut Vec<u8>, v: u64) {
    out.extend_from_slice(&v.to_le_bytes());
}

fn put_bytes(out: &mut Vec<u8>, v: &[u8]) {
    put_u64(out, v.len() as u64);
    out.extend_from_slice(v);
}

struct Reader<'a> {
    data: &'a [u8],
    pos: usize,
}

impl<'a> Reader<'a> {
    fn new(data: &'a [u8]) -> Self {
        Self { data, pos: 0 }
    }

    fn take(&mut self, n: usize) -> crate::Result<&'a [u8]> {
        let end = self
            .pos
            .checked_add(n)
            .ok_or_else(|| malformed("length overflow"))?;
        if end > self.data.len() {
            return Err(malformed("truncated object"));
        }
        let out = &self.data[self.pos..end];
        self.pos = end;
        Ok(out)
    }

    fn u8(&mut self) -> crate::Result<u8> {
        Ok(self.take(1)?[0])
    }

    fn u32(&mut self) -> crate::Result<u32> {
        let b = self.take(4)?;
        Ok(u32::from_le_bytes([b[0], b[1], b[2], b[3]]))
    }

    fn u64(&mut self) -> crate::Result<u64> {
        let b = self.take(8)?;
        let mut arr = [0u8; 8];
        arr.copy_from_slice(b);
        Ok(u64::from_le_bytes(arr))
    }

    fn bytes(&mut self) -> crate::Result<&'a [u8]> {
        let len = self.u64()?;
        let len = usize::try_from(len).map_err(|_| malformed("length exceeds address space"))?;
        self.take(len)
    }

    fn string(&mut self) -> crate::Result<String> {
        let raw = self.bytes()?;
        String::from_utf8(raw.to_vec()).map_err(|_| malformed("string is not valid UTF-8"))
    }

    fn object_id(&mut self) -> crate::Result<ObjectId> {
        let raw = self.take(32)?;
        let mut out = [0u8; 32];
        out.copy_from_slice(raw);
        Ok(ObjectId(out))
    }

    fn finish(&self) -> crate::Result<()> {
        if self.pos != self.data.len() {
            return Err(malformed("trailing bytes after object"));
        }
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// File manifests
// ---------------------------------------------------------------------------

/// How a file's content is stored: inline for small files, otherwise a chunk list.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum FileContent {
    Inline(Vec<u8>),
    Chunked { size: u64, chunks: Vec<ObjectId> },
}

const FILE_INLINE: u8 = 0;
const FILE_CHUNKED: u8 = 1;

pub(crate) fn encode_file(content: &FileContent) -> Encoded {
    let mut out = Vec::new();
    match content {
        FileContent::Inline(bytes) => {
            out.push(FILE_INLINE);
            put_bytes(&mut out, bytes);
        }
        FileContent::Chunked { size, chunks } => {
            out.push(FILE_CHUNKED);
            put_u64(&mut out, *size);
            put_u32(&mut out, chunks.len() as u32);
            for id in chunks {
                out.extend_from_slice(&id.0);
            }
        }
    }
    Encoded::new(KIND_FILE, out)
}

pub(crate) fn decode_file(payload: &[u8]) -> crate::Result<FileContent> {
    let mut r = Reader::new(payload);
    let out = match r.u8()? {
        FILE_INLINE => FileContent::Inline(r.bytes()?.to_vec()),
        FILE_CHUNKED => {
            let size = r.u64()?;
            let count = r.u32()?;
            if count > MAX_FILE_CHUNKS {
                return Err(malformed("file manifest declares too many chunks"));
            }
            let mut chunks = Vec::with_capacity(count as usize);
            for _ in 0..count {
                chunks.push(r.object_id()?);
            }
            FileContent::Chunked { size, chunks }
        }
        other => return Err(malformed(&format!("unknown file representation {other}"))),
    };
    r.finish()?;
    Ok(out)
}

/// Split file content into the manifest plus any chunk objects it references.
pub(crate) fn build_file(content: &[u8]) -> (Encoded, Vec<Encoded>) {
    if content.len() <= chunker::INLINE_MAX {
        return (
            encode_file(&FileContent::Inline(content.to_vec())),
            Vec::new(),
        );
    }

    let mut chunk_objects = Vec::new();
    let mut ids = Vec::new();
    for piece in chunker::chunk(content) {
        let encoded = Encoded::new(KIND_CHUNK, piece.to_vec());
        ids.push(encoded.id);
        chunk_objects.push(encoded);
    }

    let manifest = encode_file(&FileContent::Chunked {
        size: content.len() as u64,
        chunks: ids,
    });
    (manifest, chunk_objects)
}

// ---------------------------------------------------------------------------
// Trees
// ---------------------------------------------------------------------------

const ENTRY_FILE: u8 = 0;
const ENTRY_DIR: u8 = 1;
const ENTRY_SYMLINK: u8 = 2;
const ENTRY_FIFO: u8 = 3;

/// One tree entry: a path plus its type, mode, and (for files) content pointer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct TreeEntry {
    pub path: String,
    pub mode: u32,
    pub kind: TreeEntryKind,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum TreeEntryKind {
    File(ObjectId),
    Directory,
    Symlink(String),
    Fifo,
}

pub(crate) fn encode_tree(entries: &[TreeEntry]) -> Encoded {
    let mut out = Vec::new();
    put_u32(&mut out, entries.len() as u32);
    for entry in entries {
        put_bytes(&mut out, entry.path.as_bytes());
        put_u32(&mut out, entry.mode);
        match &entry.kind {
            TreeEntryKind::File(id) => {
                out.push(ENTRY_FILE);
                out.extend_from_slice(&id.0);
            }
            TreeEntryKind::Directory => out.push(ENTRY_DIR),
            TreeEntryKind::Symlink(target) => {
                out.push(ENTRY_SYMLINK);
                put_bytes(&mut out, target.as_bytes());
            }
            TreeEntryKind::Fifo => out.push(ENTRY_FIFO),
        }
    }
    Encoded::new(KIND_TREE, out)
}

pub(crate) fn decode_tree(payload: &[u8]) -> crate::Result<Vec<TreeEntry>> {
    let mut r = Reader::new(payload);
    let count = r.u32()?;
    if count > MAX_TREE_ENTRIES {
        return Err(malformed("tree declares too many entries"));
    }
    let mut entries = Vec::with_capacity((count as usize).min(4096));
    for _ in 0..count {
        let path = r.string()?;
        let mode = r.u32()?;
        let kind = match r.u8()? {
            ENTRY_FILE => TreeEntryKind::File(r.object_id()?),
            ENTRY_DIR => TreeEntryKind::Directory,
            ENTRY_SYMLINK => TreeEntryKind::Symlink(r.string()?),
            ENTRY_FIFO => TreeEntryKind::Fifo,
            other => return Err(malformed(&format!("unknown tree entry kind {other}"))),
        };
        entries.push(TreeEntry { path, mode, kind });
    }
    r.finish()?;
    Ok(entries)
}

/// Convert a live VFS snapshot into a tree plus every object it needs.
///
/// Entries are sorted by path, so identical filesystem state always produces
/// the same tree object regardless of `HashMap` iteration order.
pub(crate) fn build_tree(vfs: &VfsSnapshot) -> crate::Result<(Encoded, Vec<Encoded>)> {
    let mut objects = Vec::new();
    let mut entries = Vec::with_capacity(vfs.entries().len());

    for entry in vfs.entries() {
        let path = path_to_string(&entry.path)?;
        let kind = match &entry.kind {
            VfsEntryKind::File { content } => {
                let (manifest, chunks) = build_file(content);
                let id = manifest.id;
                objects.push(manifest);
                objects.extend(chunks);
                TreeEntryKind::File(id)
            }
            VfsEntryKind::Directory => TreeEntryKind::Directory,
            VfsEntryKind::Symlink { target } => TreeEntryKind::Symlink(path_to_string(target)?),
            VfsEntryKind::Fifo => TreeEntryKind::Fifo,
        };
        entries.push(TreeEntry {
            path,
            mode: entry.mode,
            kind,
        });
    }

    entries.sort_by(|a, b| a.path.cmp(&b.path));
    Ok((encode_tree(&entries), objects))
}

fn path_to_string(path: &std::path::Path) -> crate::Result<String> {
    path.to_str()
        .map(str::to_owned)
        .ok_or_else(|| crate::Error::Internal("snapshot paths must be valid UTF-8".to_string()))
}

/// Rebuild VFS entries from a tree, pulling file content out of `resolve`.
pub(crate) fn tree_to_vfs(
    entries: &[TreeEntry],
    mut resolve: impl FnMut(ObjectId) -> crate::Result<Vec<u8>>,
) -> crate::Result<VfsSnapshot> {
    let mut out = Vec::with_capacity(entries.len());
    for entry in entries {
        let kind = match &entry.kind {
            TreeEntryKind::File(id) => VfsEntryKind::File {
                content: resolve(*id)?,
            },
            TreeEntryKind::Directory => VfsEntryKind::Directory,
            TreeEntryKind::Symlink(target) => VfsEntryKind::Symlink {
                target: PathBuf::from(target),
            },
            TreeEntryKind::Fifo => VfsEntryKind::Fifo,
        };
        out.push(VfsEntry {
            path: PathBuf::from(&entry.path),
            kind,
            mode: entry.mode,
        });
    }
    Ok(VfsSnapshot::from_entries(out))
}

// ---------------------------------------------------------------------------
// Canonical JSON for descriptive objects
// ---------------------------------------------------------------------------

/// Serialize to JSON with every object key sorted.
///
/// `serde_json` emits `HashMap` entries in iteration order, which varies per
/// process. Snapshot identity is a content hash, so unstable key order would
/// mean identical state produced different commit IDs — the exact failure that
/// made external delta compression useless against the v1 format.
pub(crate) fn canonical_json<T: serde::Serialize>(value: &T) -> crate::Result<Vec<u8>> {
    let value = serde_json::to_value(value).map_err(|e| crate::Error::Internal(e.to_string()))?;
    let mut out = Vec::new();
    write_canonical(&value, &mut out);
    Ok(out)
}

fn write_canonical(value: &serde_json::Value, out: &mut Vec<u8>) {
    match value {
        serde_json::Value::Object(map) => {
            let sorted: BTreeMap<&String, &serde_json::Value> = map.iter().collect();
            out.push(b'{');
            for (i, (k, v)) in sorted.iter().enumerate() {
                if i > 0 {
                    out.push(b',');
                }
                let key = serde_json::Value::String((*k).clone());
                out.extend_from_slice(key.to_string().as_bytes());
                out.push(b':');
                write_canonical(v, out);
            }
            out.push(b'}');
        }
        serde_json::Value::Array(items) => {
            out.push(b'[');
            for (i, v) in items.iter().enumerate() {
                if i > 0 {
                    out.push(b',');
                }
                write_canonical(v, out);
            }
            out.push(b']');
        }
        other => out.extend_from_slice(other.to_string().as_bytes()),
    }
}

pub(crate) fn from_canonical_json<T: serde::de::DeserializeOwned>(
    payload: &[u8],
) -> crate::Result<T> {
    serde_json::from_slice(payload).map_err(|e| malformed(&e.to_string()))
}

// ---------------------------------------------------------------------------
// Commits
// ---------------------------------------------------------------------------

/// The root object of a snapshot: what state, derived from which parents.
///
/// Deserialization tolerates unknown fields on purpose — a newer bashkit may
/// add fields, and an older reader must ignore them rather than fail, per the
/// version policy in `knowledge/foundations/snapshot-history.md`.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct CommitObject {
    /// Commits this one descends from. Empty for a root commit; a fork is a
    /// commit whose parent is not the branch tip.
    #[serde(default)]
    pub parents: Vec<CommitId>,
    /// Tree object holding VFS state, or `None` for a shell-only snapshot.
    #[serde(default)]
    pub tree: Option<ObjectId>,
    /// Shell state object.
    pub shell: ObjectId,
    /// Capability fingerprint of the instance that produced this commit.
    pub caps: ObjectId,
    /// Session command counter at commit time.
    #[serde(default)]
    pub session_commands: u64,
    /// Session exec-call counter at commit time.
    #[serde(default)]
    pub session_exec_calls: u64,
    /// Opaque host metadata (message id, timestamp, author — bashkit does not
    /// interpret these; it has no clock on wasm targets and no session model).
    #[serde(default)]
    pub meta: BTreeMap<String, String>,
}

pub(crate) fn encode_commit(commit: &CommitObject) -> crate::Result<Encoded> {
    // Enforced on the way out as well as the way in: encoding a commit the
    // decoder will refuse would hand the caller an `Ok` holding bytes that
    // cannot be checked out.
    if commit.parents.len() > MAX_PARENTS {
        return Err(crate::Error::Internal(format!(
            "commit declares {} parents, above the {MAX_PARENTS} maximum",
            commit.parents.len()
        )));
    }
    Ok(Encoded::new(KIND_COMMIT, canonical_json(commit)?))
}

pub(crate) fn decode_commit(payload: &[u8]) -> crate::Result<CommitObject> {
    let commit: CommitObject = from_canonical_json(payload)?;
    if commit.parents.len() > MAX_PARENTS {
        return Err(malformed("commit declares too many parents"));
    }
    Ok(commit)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn object_id_hex_round_trip() {
        let id = ObjectId::of(KIND_CHUNK, b"hello");
        let parsed = ObjectId::from_hex(&id.to_hex()).unwrap();
        assert_eq!(id, parsed);
    }

    #[test]
    fn object_id_rejects_bad_hex() {
        assert!(ObjectId::from_hex("abc").is_err());
        assert!(ObjectId::from_hex(&"z".repeat(64)).is_err());
    }

    #[test]
    fn object_id_rejects_non_ascii_without_panicking() {
        // `len()` counts bytes, so each of these is 64 bytes but not 64 hex
        // characters. Slicing them at byte offsets would split a character and
        // panic; they must return errors instead. Reachable from Rust, Python,
        // and JS callers, all of which pass ids as strings.
        for bad in [
            "é".repeat(32),                        // 64 bytes, 32 characters
            format!("{}é", "a".repeat(62)),        // 64 bytes, trailing 2-byte char
            format!("é{}", "a".repeat(62)),        // 64 bytes, leading 2-byte char
            format!("a{}{}", "é", "a".repeat(61)), // char straddling a pair
            "\u{10348}".repeat(16),                // 64 bytes of 4-byte characters
        ] {
            assert_eq!(bad.len(), 64, "test input must be 64 bytes: {bad:?}");
            assert!(
                ObjectId::from_hex(&bad).is_err(),
                "expected an error for {bad:?}"
            );
        }
    }

    #[test]
    fn object_id_accepts_both_hex_cases() {
        let id = ObjectId::of(KIND_CHUNK, b"case");
        let lower = id.to_hex();
        assert_eq!(ObjectId::from_hex(&lower).unwrap(), id);
        assert_eq!(ObjectId::from_hex(&lower.to_uppercase()).unwrap(), id);
    }

    #[test]
    fn encode_commit_rejects_more_parents_than_the_decoder_accepts() {
        // Producing bytes the decoder refuses would hand the caller an `Ok`
        // holding a commit that can never be checked out.
        let id = ObjectId::of(KIND_COMMIT, b"x");
        let commit = CommitObject {
            parents: vec![id; MAX_PARENTS + 1],
            tree: None,
            shell: id,
            caps: id,
            session_commands: 0,
            session_exec_calls: 0,
            meta: BTreeMap::new(),
        };
        assert!(encode_commit(&commit).is_err());

        let ok = CommitObject {
            parents: vec![id; MAX_PARENTS],
            ..commit
        };
        assert!(encode_commit(&ok).is_ok());
    }

    #[test]
    fn id_depends_on_kind_not_just_payload() {
        // Without domain separation a chunk and a tree with identical bytes
        // would collide, which is how type-confusion attacks start.
        assert_ne!(
            ObjectId::of(KIND_CHUNK, b"same"),
            ObjectId::of(KIND_TREE, b"same")
        );
    }

    #[test]
    fn storage_round_trip_verifies_hash() {
        let obj = Encoded::new(KIND_CHUNK, b"payload".to_vec());
        let stored = obj.to_storage();
        let back = Encoded::from_storage(obj.id, &stored).unwrap();
        assert_eq!(back.payload, obj.payload);
    }

    #[test]
    fn storage_rejects_content_that_does_not_match_id() {
        let obj = Encoded::new(KIND_CHUNK, b"payload".to_vec());
        let mut tampered = obj.to_storage();
        let last = tampered.len() - 1;
        tampered[last] ^= 0xff;
        assert!(Encoded::from_storage(obj.id, &tampered).is_err());
    }

    #[test]
    fn expect_kind_rejects_type_confusion() {
        let obj = Encoded::new(KIND_CHUNK, b"x".to_vec());
        assert!(obj.expect_kind(KIND_TREE).is_err());
        assert!(obj.expect_kind(KIND_CHUNK).is_ok());
    }

    #[test]
    fn file_manifest_round_trip_inline_and_chunked() {
        let inline = FileContent::Inline(b"small".to_vec());
        let encoded = encode_file(&inline);
        assert_eq!(decode_file(&encoded.payload).unwrap(), inline);

        let chunked = FileContent::Chunked {
            size: 99,
            chunks: vec![
                ObjectId::of(KIND_CHUNK, b"a"),
                ObjectId::of(KIND_CHUNK, b"b"),
            ],
        };
        let encoded = encode_file(&chunked);
        assert_eq!(decode_file(&encoded.payload).unwrap(), chunked);
    }

    #[test]
    fn small_file_stays_inline_large_file_chunks() {
        let (manifest, chunks) = build_file(b"tiny");
        assert!(chunks.is_empty());
        assert!(matches!(
            decode_file(&manifest.payload).unwrap(),
            FileContent::Inline(_)
        ));

        let big = vec![7u8; chunker::INLINE_MAX * 8];
        let (manifest, chunks) = build_file(&big);
        assert!(!chunks.is_empty());
        assert!(matches!(
            decode_file(&manifest.payload).unwrap(),
            FileContent::Chunked { .. }
        ));
    }

    #[test]
    fn binary_content_round_trips_exactly() {
        // The v1 JSON format inflated these bytes into integer arrays; the
        // point of the binary encoding is that arbitrary bytes survive as-is.
        let content: Vec<u8> = (0..=255u8).cycle().take(50_000).collect();
        let (manifest, chunks) = build_file(&content);
        let map: BTreeMap<ObjectId, Vec<u8>> =
            chunks.iter().map(|c| (c.id, c.payload.clone())).collect();

        let rebuilt = match decode_file(&manifest.payload).unwrap() {
            FileContent::Chunked { chunks, .. } => chunks
                .iter()
                .flat_map(|id| map.get(id).unwrap().clone())
                .collect::<Vec<u8>>(),
            FileContent::Inline(b) => b,
        };
        assert_eq!(rebuilt, content);
    }

    #[test]
    fn tree_round_trip() {
        let entries = vec![
            TreeEntry {
                path: "/a".to_string(),
                mode: 0o644,
                kind: TreeEntryKind::File(ObjectId::of(KIND_FILE, b"f")),
            },
            TreeEntry {
                path: "/d".to_string(),
                mode: 0o755,
                kind: TreeEntryKind::Directory,
            },
            TreeEntry {
                path: "/l".to_string(),
                mode: 0o777,
                kind: TreeEntryKind::Symlink("/a".to_string()),
            },
            TreeEntry {
                path: "/p".to_string(),
                mode: 0o644,
                kind: TreeEntryKind::Fifo,
            },
        ];
        let encoded = encode_tree(&entries);
        assert_eq!(decode_tree(&encoded.payload).unwrap(), entries);
    }

    #[test]
    fn decode_rejects_truncated_and_trailing_bytes() {
        let encoded = encode_tree(&[TreeEntry {
            path: "/a".to_string(),
            mode: 0o644,
            kind: TreeEntryKind::Directory,
        }]);
        assert!(decode_tree(&encoded.payload[..encoded.payload.len() - 1]).is_err());

        let mut extra = encoded.payload.clone();
        extra.push(0);
        assert!(decode_tree(&extra).is_err());
    }

    #[test]
    fn decode_rejects_absurd_entry_count() {
        // A 4-byte header claiming billions of entries must not cause a
        // multi-gigabyte allocation before the read fails.
        let mut payload = Vec::new();
        put_u32(&mut payload, u32::MAX);
        assert!(decode_tree(&payload).is_err());
    }

    #[test]
    fn canonical_json_sorts_keys_regardless_of_insertion_order() {
        use std::collections::HashMap;
        let mut a: HashMap<String, u32> = HashMap::new();
        let mut b: HashMap<String, u32> = HashMap::new();
        for k in ["zeta", "alpha", "mu", "beta", "omega", "gamma"] {
            a.insert(k.to_string(), 1);
        }
        for k in ["omega", "mu", "gamma", "alpha", "beta", "zeta"] {
            b.insert(k.to_string(), 1);
        }
        assert_eq!(canonical_json(&a).unwrap(), canonical_json(&b).unwrap());
    }

    #[test]
    fn canonical_json_escapes_keys_and_nests() {
        #[derive(serde::Serialize)]
        struct Nested {
            outer: BTreeMap<String, Vec<u32>>,
        }
        let mut outer = BTreeMap::new();
        outer.insert("quote\"key".to_string(), vec![1, 2]);
        let bytes = canonical_json(&Nested { outer }).unwrap();
        let parsed: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(parsed["outer"]["quote\"key"][1], 2);
    }

    #[test]
    fn commit_decode_ignores_unknown_fields() {
        // Forward compatibility: a newer producer may add fields, and this
        // reader must ignore them rather than reject the commit.
        let json = br#"{"parents":[],"shell":"00112233445566778899aabbccddeeff00112233445566778899aabbccddeeff","caps":"00112233445566778899aabbccddeeff00112233445566778899aabbccddeeff","future_field":{"nested":true}}"#;
        let commit = decode_commit(json).unwrap();
        assert!(commit.parents.is_empty());
        assert!(commit.tree.is_none());
    }

    #[test]
    fn commit_decode_rejects_excessive_parents() {
        // Encoded by hand: `encode_commit` now refuses this too, so the decoder
        // has to be exercised against bytes a hostile producer could still
        // hand us.
        let id = ObjectId::of(KIND_COMMIT, b"x");
        let commit = CommitObject {
            parents: vec![id; MAX_PARENTS + 1],
            tree: None,
            shell: id,
            caps: id,
            session_commands: 0,
            session_exec_calls: 0,
            meta: BTreeMap::new(),
        };
        let payload = canonical_json(&commit).unwrap();
        assert!(decode_commit(&payload).is_err());
    }
}
