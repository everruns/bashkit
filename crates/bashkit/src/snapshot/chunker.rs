// Decision: content-defined chunking is implemented in-tree rather than via the
// `fastcdc` crate. The proposal (knowledge/foundations/snapshot-history.md) named
// an in-tree gear-hash CDC as the fallback if a third-party crate could not clear
// `cargo deny`; taking the fallback up front keeps the dependency surface at zero
// for a self-contained ~100-line algorithm whose parameters we need pinned in the
// format spec anyway. Changing MIN/AVG/MAX changes every object ID, so they are
// format constants, not tunables.

//! Content-defined chunking for snapshot file content.
//!
//! Splits file content at boundaries chosen by the data itself, so an edit
//! re-chunks only the region around the edit and every untouched chunk keeps
//! its identity (and therefore its place in the object store). This is what
//! makes per-file content deltas fall out of content addressing without a diff
//! or patch algorithm, and it behaves identically for text and binary.

/// Smallest chunk the splitter will emit (except for a trailing remainder).
pub(crate) const MIN_CHUNK: usize = 2 * 1024;
/// Target average chunk size, as a power-of-two mask width.
const AVG_BITS: u32 = 14; // 16 KiB
/// Largest chunk the splitter will emit.
pub(crate) const MAX_CHUNK: usize = 64 * 1024;
/// Files at or below this size are stored inline in their manifest instead of
/// being chunked, so small files do not pay for a chunk table.
pub(crate) const INLINE_MAX: usize = 4 * 1024;

const MASK: u64 = (1u64 << AVG_BITS) - 1;

/// Deterministic gear table.
///
/// Built by a fixed xorshift64 sequence so the table is identical on every
/// platform and every build — object IDs depend on it.
const fn gear_table() -> [u64; 256] {
    let mut table = [0u64; 256];
    let mut state: u64 = 0x2545_F491_4F6C_DD1D;
    let mut i = 0;
    while i < 256 {
        state ^= state << 13;
        state ^= state >> 7;
        state ^= state << 17;
        table[i] = state;
        i += 1;
    }
    table
}

static GEAR: [u64; 256] = gear_table();

/// Split `data` into content-defined chunks, returning the chunk boundaries as
/// slices. Chunks concatenate back to `data` exactly.
pub(crate) fn chunk(data: &[u8]) -> Vec<&[u8]> {
    let mut out = Vec::new();
    let mut start = 0usize;

    while start < data.len() {
        let end = next_boundary(&data[start..]) + start;
        out.push(&data[start..end]);
        start = end;
    }

    out
}

/// Find the cut point within `data`, measured from its start.
fn next_boundary(data: &[u8]) -> usize {
    if data.len() <= MIN_CHUNK {
        return data.len();
    }

    let limit = data.len().min(MAX_CHUNK);
    let mut hash = 0u64;

    // Bytes before MIN_CHUNK still feed the rolling hash, they just cannot cut.
    // Seeding this way keeps the boundary decision dependent only on nearby
    // content, which is what makes an edit's re-chunking stay local.
    for &byte in &data[..MIN_CHUNK] {
        hash = (hash << 1).wrapping_add(GEAR[byte as usize]);
    }

    for (offset, &byte) in data.iter().enumerate().take(limit).skip(MIN_CHUNK) {
        hash = (hash << 1).wrapping_add(GEAR[byte as usize]);
        if hash & MASK == 0 {
            return offset + 1;
        }
    }

    limit
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pseudo_random(len: usize, seed: u64) -> Vec<u8> {
        let mut state = seed | 1;
        (0..len)
            .map(|_| {
                state ^= state << 13;
                state ^= state >> 7;
                state ^= state << 17;
                (state >> 24) as u8
            })
            .collect()
    }

    #[test]
    fn chunks_reassemble_exactly() {
        for len in [0, 1, 100, MIN_CHUNK, MIN_CHUNK + 1, 300_000] {
            let data = pseudo_random(len, 42);
            let rejoined: Vec<u8> = chunk(&data).concat();
            assert_eq!(rejoined, data, "round-trip failed at len {len}");
        }
    }

    #[test]
    fn chunks_respect_size_bounds() {
        let data = pseudo_random(1_000_000, 7);
        let chunks = chunk(&data);
        assert!(chunks.len() > 1, "large input should split");
        for c in &chunks[..chunks.len() - 1] {
            assert!(c.len() >= MIN_CHUNK, "chunk below minimum: {}", c.len());
            assert!(c.len() <= MAX_CHUNK, "chunk above maximum: {}", c.len());
        }
    }

    #[test]
    fn chunking_is_deterministic() {
        let data = pseudo_random(500_000, 99);
        let a: Vec<usize> = chunk(&data).iter().map(|c| c.len()).collect();
        let b: Vec<usize> = chunk(&data).iter().map(|c| c.len()).collect();
        assert_eq!(a, b);
    }

    #[test]
    fn all_zero_input_still_terminates_and_reassembles() {
        // Degenerate content never satisfies the cut mask, so every chunk is
        // driven by MAX_CHUNK. Guards against an infinite loop on zero-length
        // boundaries.
        let data = vec![0u8; 500_000];
        let chunks = chunk(&data);
        assert!(chunks.iter().all(|c| !c.is_empty()));
        assert_eq!(chunks.concat(), data);
    }

    #[test]
    fn edit_in_middle_preserves_distant_chunks() {
        // The property the whole design rests on: a localized edit must not
        // shift every subsequent boundary.
        let original = pseudo_random(400_000, 5);
        let mut edited = original.clone();
        edited.splice(200_000..200_000, b"INSERTED PAYLOAD".iter().copied());

        let before: Vec<&[u8]> = chunk(&original);
        let after: Vec<&[u8]> = chunk(&edited);

        let shared = before
            .iter()
            .filter(|c| after.iter().any(|d| d == *c))
            .count();
        let ratio = shared as f64 / before.len() as f64;
        assert!(
            ratio > 0.8,
            "expected most chunks to survive a local edit, kept {shared}/{} ({ratio:.2})",
            before.len()
        );
    }
}
