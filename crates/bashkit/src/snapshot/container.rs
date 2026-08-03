// Decision: three independent version numbers, not one. `container_version`
// and `schema_version` say what a producer wrote; `min_reader` says what a
// reader must understand. A reader accepts anything with `min_reader <= its
// own`, however much newer the other two are. That is what lets us add fields
// and object kinds without invalidating stored snapshots — the failure mode the
// v1 format had, where `version != 1` rejected in both directions.
//
// Decision: object storage compresses per object, and the object ID hashes the
// *uncompressed* bytes. A host may recompress its store at will, and identical
// content still dedups even if two producers chose different compression.

//! The packed snapshot container: a commit plus every object it reaches, in one
//! self-contained, integrity-protected blob.
//!
//! This is what [`Bash::snapshot`](crate::Bash::snapshot) returns. Hosts that
//! keep their own object store use the commit/checkout API instead and never
//! materialize a container.

use std::collections::BTreeMap;
use std::io::Read;

use super::objects::ObjectId;

/// Magic prefix identifying a v2 container body.
pub(crate) const MAGIC: &[u8; 6] = b"BKSNAP";

/// Envelope framing version.
pub(crate) const CONTAINER_VERSION: u8 = 2;
/// Metadata field-set version.
pub(crate) const SCHEMA_VERSION: u16 = 1;
/// Oldest reader that can correctly read what we currently produce.
pub(crate) const MIN_READER: u16 = 2;
/// Newest `min_reader` this build can satisfy.
pub(crate) const READER_VERSION: u16 = 2;

/// SHA-256 hash algorithm id. Recorded so the algorithm can change later
/// without a framing break.
pub(crate) const HASH_SHA256: u8 = 1;

const COMPRESSION_NONE: u8 = 0;
const COMPRESSION_DEFLATE: u8 = 1;

/// Ceiling on a single object's decompressed size (TM-SNAP-004). Filesystem
/// limits still apply afterwards; this only stops a small container from
/// expanding into an unbounded allocation before those checks can run.
const MAX_OBJECT_BYTES: u64 = 256 * 1024 * 1024;
/// Ceiling on how many objects one container may declare.
const MAX_OBJECTS: u32 = 50_000_000;

fn malformed(what: &str) -> crate::Error {
    crate::Error::Internal(format!("malformed snapshot container: {what}"))
}

/// Bytes sampled to decide whether an object is worth compressing.
const COMPRESSIBILITY_SAMPLE: usize = 4096;
/// A sample must shrink to at most this fraction of its size for the full
/// object to be worth deflating.
const COMPRESSIBILITY_THRESHOLD: f64 = 0.95;
/// Objects at or below this size skip the probe — deflating a few KiB costs
/// less than deciding not to.
const COMPRESSIBILITY_MIN_SIZE: usize = 8192;

/// Deflate `payload`, or report that it is not worth compressing.
fn deflate(payload: &[u8]) -> Option<Vec<u8>> {
    use flate2::Compression;
    use flate2::write::DeflateEncoder;
    use std::io::Write;

    let mut encoder = DeflateEncoder::new(
        Vec::with_capacity(payload.len() / 2),
        Compression::default(),
    );
    encoder
        .write_all(payload)
        .ok()
        .and_then(|()| encoder.finish().ok())
}

/// Cheap check for whether a large object is compressible at all.
///
/// Deflating incompressible data — which is what a large binary file's chunks
/// are — costs real time and is then thrown away. Probing a 4 KiB sample first
/// turns that into a fixed cost per object regardless of object size. The
/// sample is taken from the middle so a header (which often *is* compressible)
/// does not mislead the estimate for the body.
fn looks_compressible(payload: &[u8]) -> bool {
    if payload.len() <= COMPRESSIBILITY_MIN_SIZE {
        return true;
    }
    let start = (payload.len() - COMPRESSIBILITY_SAMPLE) / 2;
    let sample = &payload[start..start + COMPRESSIBILITY_SAMPLE];
    match deflate(sample) {
        Some(out) => (out.len() as f64) < sample.len() as f64 * COMPRESSIBILITY_THRESHOLD,
        None => false,
    }
}

/// Compress an object body for storage, keeping the smaller of the two forms.
///
/// Production encodes via [`storage_blob_from_parts`], which fuses the kind
/// byte in without a second copy; this stays as the readable reference the
/// fused encoder is tested against.
#[cfg(test)]
pub(crate) fn compress(payload: &[u8]) -> (u8, Vec<u8>) {
    if !looks_compressible(payload) {
        return (COMPRESSION_NONE, payload.to_vec());
    }
    match deflate(payload).filter(|out| out.len() < payload.len()) {
        Some(out) => (COMPRESSION_DEFLATE, out),
        None => (COMPRESSION_NONE, payload.to_vec()),
    }
}

/// Build an object's storage blob directly from its parts.
///
/// Equivalent to compressing `[kind][payload]`, without materializing that
/// concatenation first — which for a large file's chunks would mean an extra
/// full copy of the content per object.
pub(crate) fn storage_blob_from_parts(kind: u8, payload: &[u8]) -> Vec<u8> {
    use flate2::Compression;
    use flate2::write::DeflateEncoder;
    use std::io::Write;

    let raw_len = 1 + payload.len();
    if looks_compressible(payload) {
        let mut encoder =
            DeflateEncoder::new(Vec::with_capacity(raw_len / 2), Compression::default());
        let compressed = encoder
            .write_all(&[kind])
            .and_then(|()| encoder.write_all(payload))
            .ok()
            .and_then(|()| encoder.finish().ok())
            .filter(|out| out.len() < raw_len);
        if let Some(out) = compressed {
            let mut blob = Vec::with_capacity(1 + out.len());
            blob.push(COMPRESSION_DEFLATE);
            blob.extend_from_slice(&out);
            return blob;
        }
    }

    let mut blob = Vec::with_capacity(1 + raw_len);
    blob.push(COMPRESSION_NONE);
    blob.push(kind);
    blob.extend_from_slice(payload);
    blob
}

/// Reverse [`compress`], refusing to expand past [`MAX_OBJECT_BYTES`].
pub(crate) fn decompress(flag: u8, data: &[u8]) -> crate::Result<Vec<u8>> {
    match flag {
        COMPRESSION_NONE => Ok(data.to_vec()),
        COMPRESSION_DEFLATE => {
            let mut out = Vec::new();
            let mut decoder = flate2::read::DeflateDecoder::new(data);
            (&mut decoder)
                .take(MAX_OBJECT_BYTES + 1)
                .read_to_end(&mut out)
                .map_err(|_| malformed("object failed to decompress"))?;
            if out.len() as u64 > MAX_OBJECT_BYTES {
                return Err(malformed("object expands beyond the per-object size limit"));
            }
            // A deflate stream ends at its final-block marker, so a decoder
            // happily ignores anything after it. Without this check a store
            // could append arbitrary bytes to any object and still hand back
            // content that hashes correctly — hidden payload the content
            // address does not cover (TM-SNAP-002).
            if decoder.total_in() as usize != data.len() {
                return Err(malformed(
                    "object has trailing bytes after its deflate stream",
                ));
            }
            Ok(out)
        }
        other => Err(malformed(&format!("unknown compression flag {other}"))),
    }
}

/// Wrap an object payload in its storage envelope: `[compression][bytes]`.
#[cfg(test)]
pub(crate) fn to_storage_blob(payload: &[u8]) -> Vec<u8> {
    let (flag, data) = compress(payload);
    let mut out = Vec::with_capacity(1 + data.len());
    out.push(flag);
    out.extend_from_slice(&data);
    out
}

/// Unwrap a storage envelope produced by [`storage_blob_from_parts`].
pub(crate) fn from_storage_blob(blob: &[u8]) -> crate::Result<Vec<u8>> {
    let (flag, data) = blob
        .split_first()
        .ok_or_else(|| malformed("empty object blob"))?;
    decompress(*flag, data)
}

/// Everything needed to reconstruct a commit without a store.
pub(crate) struct Container {
    pub root: ObjectId,
    pub objects: BTreeMap<ObjectId, Vec<u8>>,
}

/// Serialize a container body (everything the integrity digest covers).
pub(crate) fn encode(root: ObjectId, objects: &BTreeMap<ObjectId, Vec<u8>>) -> Vec<u8> {
    let mut out = Vec::new();
    out.extend_from_slice(MAGIC);
    out.push(CONTAINER_VERSION);
    out.push(0); // reserved
    out.extend_from_slice(&SCHEMA_VERSION.to_le_bytes());
    out.extend_from_slice(&MIN_READER.to_le_bytes());
    out.push(HASH_SHA256);
    out.push(0); // flags
    out.extend_from_slice(root.as_bytes());
    out.extend_from_slice(&(objects.len() as u32).to_le_bytes());
    // BTreeMap iteration is ordered by ID, so identical content produces
    // identical container bytes.
    for (id, blob) in objects {
        out.extend_from_slice(id.as_bytes());
        out.extend_from_slice(&(blob.len() as u64).to_le_bytes());
        out.extend_from_slice(blob);
    }
    out
}

/// True when `body` looks like a v2 container rather than a v1 JSON payload.
pub(crate) fn is_v2(body: &[u8]) -> bool {
    body.starts_with(MAGIC)
}

/// Parse a container body, enforcing the version policy.
pub(crate) fn decode(body: &[u8]) -> crate::Result<Container> {
    struct Cursor<'a> {
        body: &'a [u8],
        pos: usize,
    }
    impl<'a> Cursor<'a> {
        fn take(&mut self, n: usize) -> crate::Result<&'a [u8]> {
            let end = self
                .pos
                .checked_add(n)
                .ok_or_else(|| malformed("length overflow"))?;
            if end > self.body.len() {
                return Err(malformed("truncated container"));
            }
            let out = &self.body[self.pos..end];
            self.pos = end;
            Ok(out)
        }
        fn u16(&mut self) -> crate::Result<u16> {
            let b = self.take(2)?;
            Ok(u16::from_le_bytes([b[0], b[1]]))
        }
    }

    let mut cur = Cursor { body, pos: 0 };
    macro_rules! take {
        ($n:expr) => {
            cur.take($n)?
        };
    }

    if take!(6) != MAGIC {
        return Err(malformed("bad magic"));
    }
    let _container_version = take!(1)[0];
    let _reserved = take!(1)[0];
    let _schema_version = cur.u16()?;
    let min_reader = cur.u16()?;

    // The whole point of the policy: only `min_reader` can reject us, and it
    // does so with a typed error rather than a panic or a misparse.
    if min_reader > READER_VERSION {
        return Err(crate::Error::SnapshotTooNew {
            required: min_reader,
            supported: READER_VERSION,
        });
    }

    let hash_algo = take!(1)[0];
    if hash_algo != HASH_SHA256 {
        return Err(malformed(&format!(
            "unsupported hash algorithm id {hash_algo}"
        )));
    }
    let _flags = take!(1)[0];

    let mut root_bytes = [0u8; 32];
    root_bytes.copy_from_slice(take!(32));
    let root = ObjectId::from_bytes(root_bytes);

    let count_bytes = take!(4);
    let count = u32::from_le_bytes([
        count_bytes[0],
        count_bytes[1],
        count_bytes[2],
        count_bytes[3],
    ]);
    if count > MAX_OBJECTS {
        return Err(malformed("container declares too many objects"));
    }

    let mut objects = BTreeMap::new();
    for _ in 0..count {
        let mut id_bytes = [0u8; 32];
        id_bytes.copy_from_slice(take!(32));
        let id = ObjectId::from_bytes(id_bytes);

        let len_bytes = take!(8);
        let mut arr = [0u8; 8];
        arr.copy_from_slice(len_bytes);
        let len = u64::from_le_bytes(arr);
        let len = usize::try_from(len).map_err(|_| malformed("object length exceeds memory"))?;

        objects.insert(id, take!(len).to_vec());
    }

    if cur.pos != body.len() {
        return Err(malformed("trailing bytes after container"));
    }

    Ok(Container { root, objects })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> (ObjectId, BTreeMap<ObjectId, Vec<u8>>) {
        let root = ObjectId::from_bytes([9u8; 32]);
        let mut objects = BTreeMap::new();
        objects.insert(ObjectId::from_bytes([1u8; 32]), to_storage_blob(b"first"));
        objects.insert(ObjectId::from_bytes([2u8; 32]), to_storage_blob(b"second"));
        (root, objects)
    }

    #[test]
    fn round_trip() {
        let (root, objects) = sample();
        let body = encode(root, &objects);
        assert!(is_v2(&body));
        let decoded = decode(&body).unwrap();
        assert_eq!(decoded.root, root);
        assert_eq!(decoded.objects, objects);
    }

    #[test]
    fn encoding_is_deterministic() {
        let (root, objects) = sample();
        assert_eq!(encode(root, &objects), encode(root, &objects));
    }

    #[test]
    fn rejects_truncated_and_trailing() {
        let (root, objects) = sample();
        let body = encode(root, &objects);
        assert!(decode(&body[..body.len() - 3]).is_err());

        let mut extra = body.clone();
        extra.push(0);
        assert!(decode(&extra).is_err());
    }

    #[test]
    fn rejects_bad_magic() {
        let (root, objects) = sample();
        let mut body = encode(root, &objects);
        body[0] = b'X';
        assert!(!is_v2(&body));
        assert!(decode(&body).is_err());
    }

    #[test]
    fn newer_min_reader_gives_typed_error_not_panic() {
        let (root, objects) = sample();
        let mut body = encode(root, &objects);
        // min_reader sits at offset 6+1+1+2 = 10.
        body[10..12].copy_from_slice(&999u16.to_le_bytes());
        match decode(&body) {
            Err(crate::Error::SnapshotTooNew {
                required,
                supported,
            }) => {
                assert_eq!(required, 999);
                assert_eq!(supported, READER_VERSION);
            }
            Err(other) => panic!("expected SnapshotTooNew, got {other}"),
            Ok(_) => panic!("expected SnapshotTooNew, got a successful decode"),
        }
    }

    #[test]
    fn newer_container_and_schema_versions_are_accepted() {
        // Forward compatibility: a producer that only added fields bumps these
        // two and leaves min_reader alone, and we must still read it.
        let (root, objects) = sample();
        let mut body = encode(root, &objects);
        body[6] = 99; // container_version
        body[8..10].copy_from_slice(&77u16.to_le_bytes()); // schema_version
        let decoded = decode(&body).expect("newer non-breaking versions must still decode");
        assert_eq!(decoded.root, root);
    }

    #[test]
    fn rejects_unknown_hash_algorithm() {
        let (root, objects) = sample();
        let mut body = encode(root, &objects);
        body[12] = 42;
        assert!(decode(&body).is_err());
    }

    #[test]
    fn rejects_absurd_object_count() {
        let mut body = encode(ObjectId::from_bytes([0u8; 32]), &BTreeMap::new());
        let at = body.len() - 4;
        body[at..].copy_from_slice(&u32::MAX.to_le_bytes());
        assert!(decode(&body).is_err());
    }

    #[test]
    fn compression_round_trips_and_shrinks_repetitive_data() {
        let repetitive = vec![b'a'; 10_000];
        let (flag, data) = compress(&repetitive);
        assert_eq!(flag, COMPRESSION_DEFLATE);
        assert!(data.len() < repetitive.len());
        assert_eq!(decompress(flag, &data).unwrap(), repetitive);
    }

    #[test]
    fn incompressible_data_is_stored_raw() {
        // Short, high-entropy input: deflate's framing costs more than it saves,
        // so the encoder must keep the original bytes rather than grow them.
        let mut state = 0x9E37_79B9_7F4A_7C15u64;
        let random: Vec<u8> = (0..48)
            .map(|_| {
                state ^= state << 13;
                state ^= state >> 7;
                state ^= state << 17;
                (state >> 32) as u8
            })
            .collect();
        let (flag, data) = compress(&random);
        assert_eq!(flag, COMPRESSION_NONE);
        assert_eq!(data, random);
    }

    #[test]
    fn storage_blob_round_trips_binary() {
        let payload: Vec<u8> = (0..=255u8).collect();
        let blob = to_storage_blob(&payload);
        assert_eq!(from_storage_blob(&blob).unwrap(), payload);
    }

    #[test]
    fn incompressible_large_objects_skip_the_deflate_attempt() {
        // A large binary file's chunks are incompressible. Deflating them and
        // discarding the result is the dominant cost of committing one, so the
        // probe must classify them as raw.
        let mut state = 0x9E37_79B9_7F4A_7C15u64;
        let random: Vec<u8> = (0..64 * 1024)
            .map(|_| {
                state ^= state << 13;
                state ^= state >> 7;
                state ^= state << 17;
                (state >> 32) as u8
            })
            .collect();
        assert!(!looks_compressible(&random));
        let (flag, data) = compress(&random);
        assert_eq!(flag, COMPRESSION_NONE);
        assert_eq!(data, random);
    }

    #[test]
    fn compressible_large_objects_still_compress() {
        let text: Vec<u8> = b"the quick brown fox jumps over the lazy dog\n"
            .iter()
            .cycle()
            .take(64 * 1024)
            .copied()
            .collect();
        assert!(looks_compressible(&text));
        let (flag, data) = compress(&text);
        assert_eq!(flag, COMPRESSION_DEFLATE);
        assert!(data.len() < text.len() / 4);
    }

    #[test]
    fn storage_blob_from_parts_matches_the_two_step_path() {
        // The fused encoder exists to skip a full copy of the payload; it must
        // stay byte-compatible with the decoder either way.
        for payload in [
            b"short".to_vec(),
            vec![b'z'; 40_000],
            (0..40_000u32).map(|i| (i % 251) as u8).collect(),
        ] {
            let blob = storage_blob_from_parts(7, &payload);
            let decoded = from_storage_blob(&blob).unwrap();
            assert_eq!(decoded[0], 7, "kind byte must survive");
            assert_eq!(&decoded[1..], &payload[..]);
        }
    }

    #[test]
    fn rejects_trailing_bytes_after_a_deflate_stream() {
        // The decoder stops at the final-block marker, so this data would
        // otherwise decompress to the correct content and pass its hash check
        // while smuggling extra bytes through the store.
        let payload = vec![b'q'; 4096];
        let (flag, mut data) = compress(&payload);
        assert_eq!(flag, COMPRESSION_DEFLATE);
        assert_eq!(decompress(flag, &data).unwrap(), payload);

        data.extend_from_slice(b"smuggled");
        assert!(decompress(flag, &data).is_err());
    }

    #[test]
    fn rejects_unknown_compression_flag() {
        assert!(decompress(200, b"x").is_err());
        assert!(from_storage_blob(&[200, 1, 2]).is_err());
        assert!(from_storage_blob(&[]).is_err());
    }

    #[test]
    fn decompression_bomb_is_bounded() {
        // A tiny deflate stream expanding past the per-object cap must be
        // refused rather than allocated (TM-SNAP-004).
        use flate2::Compression;
        use flate2::write::DeflateEncoder;
        use std::io::Write;

        let mut encoder = DeflateEncoder::new(Vec::new(), Compression::best());
        let block = vec![0u8; 1024 * 1024];
        for _ in 0..(MAX_OBJECT_BYTES / block.len() as u64 + 2) {
            encoder.write_all(&block).unwrap();
        }
        let bomb = encoder.finish().unwrap();
        assert!(bomb.len() < 1024 * 1024, "bomb should be small on the wire");
        assert!(decompress(COMPRESSION_DEFLATE, &bomb).is_err());
    }
}
