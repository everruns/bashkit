//! Lint for `specs/limitations.md` — the negative spec.
//!
//! Limitations record absences, which no code-level test can witness
//! directly; the next-best enforcement is keeping the document machine
//! checkable: stable well-formed `L-<AREA>-<NNN>` IDs (referenced from
//! code comments like TM-* threat IDs), no duplicates, and a non-empty
//! Evidence column on every ID'd row.

use std::collections::HashSet;
use std::path::Path;

fn limitations_doc() -> String {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../specs/limitations.md");
    std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("read {}: {e}", path.display()))
}

/// Extract (id, evidence) from every table row whose first cell is an L-* ID.
fn id_rows(doc: &str) -> Vec<(String, String)> {
    doc.lines()
        .filter_map(|line| {
            let line = line.trim();
            if !line.starts_with("| L-") {
                return None;
            }
            let cells: Vec<&str> = line.trim_matches('|').split('|').collect();
            let id = cells.first()?.trim().to_string();
            let evidence = cells.last()?.trim().to_string();
            Some((id, evidence))
        })
        .collect()
}

#[test]
fn limitations_doc_format() {
    let doc = limitations_doc();
    let rows = id_rows(&doc);
    assert!(
        rows.len() >= 8,
        "expected the intentional-limitation tables to have ID'd rows, found {}",
        rows.len()
    );

    let mut seen = HashSet::new();
    for (id, evidence) in &rows {
        let parts: Vec<&str> = id.split('-').collect();
        assert!(
            parts.len() == 3
                && parts[0] == "L"
                && !parts[1].is_empty()
                && parts[1].chars().all(|c| c.is_ascii_uppercase())
                && parts[2].len() == 3
                && parts[2].chars().all(|c| c.is_ascii_digit()),
            "malformed limitation ID: {id}"
        );
        assert!(seen.insert(id.clone()), "duplicate limitation ID: {id}");
        assert!(!evidence.is_empty(), "{id}: empty Evidence cell");
    }
}

#[test]
fn limitation_evidence_tests_exist() {
    // Evidence cells citing an `l_*` test must resolve to a test function
    // in limitations_evidence_tests.rs, so lifting a limitation can't leave
    // the doc citing a deleted test (or vice versa).
    let doc = limitations_doc();
    let evidence_src = {
        let path = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/integration/limitations_evidence_tests.rs");
        std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("read {}: {e}", path.display()))
    };

    for (id, evidence) in id_rows(&doc) {
        let cited = evidence.trim_matches('`');
        if cited.starts_with("l_") {
            assert!(
                evidence_src.contains(&format!("async fn {cited}(")),
                "{id}: evidence test `{cited}` not found in limitations_evidence_tests.rs"
            );
        }
    }
}

#[test]
fn limitation_ids_referenced_from_code_exist_in_doc() {
    // Code comments may cite L-* IDs (e.g. path.rs cites L-FS-001). Every
    // citation must resolve to a row, so lifting a limitation can't leave
    // dangling references.
    let doc = limitations_doc();
    let ids: HashSet<String> = id_rows(&doc).into_iter().map(|(id, _)| id).collect();

    let src_root = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    let mut stack = vec![src_root];
    let mut cited = HashSet::new();
    while let Some(dir) = stack.pop() {
        for entry in std::fs::read_dir(&dir).unwrap() {
            let path = entry.unwrap().path();
            if path.is_dir() {
                stack.push(path);
            } else if path.extension().is_some_and(|e| e == "rs") {
                let text = std::fs::read_to_string(&path).unwrap();
                let bytes = text.as_bytes();
                let mut i = 0;
                while let Some(off) = text[i..].find("L-") {
                    let start = i + off;
                    let rest = &text[start..];
                    // Match L-<UPPER>-<3 digits>
                    let token: String = rest
                        .chars()
                        .take_while(|c| c.is_ascii_uppercase() || c.is_ascii_digit() || *c == '-')
                        .collect();
                    let parts: Vec<&str> = token.split('-').collect();
                    if parts.len() >= 3
                        && parts[1].chars().all(|c| c.is_ascii_uppercase())
                        && !parts[1].is_empty()
                        && parts[2].len() == 3
                        && parts[2].chars().all(|c| c.is_ascii_digit())
                    {
                        cited.insert(format!("L-{}-{}", parts[1], parts[2]));
                    }
                    i = start + 2;
                    if i >= bytes.len() {
                        break;
                    }
                }
            }
        }
    }

    for id in &cited {
        assert!(
            ids.contains(id),
            "{id} cited in source but missing from specs/limitations.md"
        );
    }
}
