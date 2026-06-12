//! Lint binding code to the threat-model ledger.
//!
//! Every `TM-<CATEGORY>-<NNN>` ID cited anywhere in this crate's source or
//! tests (typically `// THREAT[TM-...]` mitigation anchors and threat-test
//! doc comments) must have an entry in `specs/threat-model.md`. This is the
//! same enforcement direction as `limitations_doc_tests`: code may not cite
//! a ledger entry that does not exist, so restructuring the ledger can never
//! silently orphan a mitigation anchor.

use std::collections::{BTreeMap, HashSet};
use std::path::{Path, PathBuf};

fn extract_tm_ids(text: &str, found: &mut HashSet<String>) {
    let bytes = text.as_bytes();
    let mut i = 0;
    while let Some(off) = text[i..].find("TM-") {
        let start = i + off;
        let rest = &text[start + 3..];
        let cat: String = rest
            .chars()
            .take_while(|c| c.is_ascii_uppercase())
            .collect();
        let after_cat = &rest[cat.len()..];
        if !cat.is_empty() && after_cat.starts_with('-') {
            let num: String = after_cat[1..]
                .chars()
                .take_while(|c| c.is_ascii_digit())
                .collect();
            if !num.is_empty() {
                found.insert(format!("TM-{cat}-{num}"));
                // Shorthand group citations: TM-ISO-005/006/007 cites three
                // IDs in the same category. Expand each /NNN suffix.
                let mut tail = &after_cat[1 + num.len()..];
                while let Some(stripped) = tail.strip_prefix('/') {
                    let next: String = stripped
                        .chars()
                        .take_while(|c| c.is_ascii_digit())
                        .collect();
                    if next.is_empty() {
                        break;
                    }
                    found.insert(format!("TM-{cat}-{next}"));
                    tail = &stripped[next.len()..];
                }
            }
        }
        i = start + 3;
        if i >= bytes.len() {
            break;
        }
    }
}

fn walk_rs_files(root: &Path, out: &mut Vec<PathBuf>) {
    for entry in std::fs::read_dir(root).unwrap() {
        let path = entry.unwrap().path();
        if path.is_dir() {
            walk_rs_files(&path, out);
        } else if path.extension().is_some_and(|e| e == "rs") {
            out.push(path);
        }
    }
}

#[test]
fn threat_ids_cited_in_code_exist_in_threat_model_doc() {
    let manifest = Path::new(env!("CARGO_MANIFEST_DIR"));
    let doc_path = manifest.join("../../specs/threat-model.md");
    let doc = std::fs::read_to_string(&doc_path)
        .unwrap_or_else(|e| panic!("read {}: {e}", doc_path.display()));

    let mut doc_ids = HashSet::new();
    extract_tm_ids(&doc, &mut doc_ids);
    assert!(
        doc_ids.len() > 100,
        "suspiciously few TM IDs in threat-model.md ({}) — parsing broken?",
        doc_ids.len()
    );

    let mut files = Vec::new();
    walk_rs_files(&manifest.join("src"), &mut files);
    walk_rs_files(&manifest.join("tests"), &mut files);

    // id -> first file that cites it, for actionable failure output
    let mut cited: BTreeMap<String, PathBuf> = BTreeMap::new();
    for file in files {
        let text = std::fs::read_to_string(&file).unwrap();
        let mut ids = HashSet::new();
        extract_tm_ids(&text, &mut ids);
        for id in ids {
            cited.entry(id).or_insert_with(|| file.clone());
        }
    }

    let missing: Vec<String> = cited
        .iter()
        .filter(|(id, _)| !doc_ids.contains(*id))
        .map(|(id, file)| format!("{id} (cited in {})", file.display()))
        .collect();
    assert!(
        missing.is_empty(),
        "TM IDs cited in code but missing from specs/threat-model.md:\n{}",
        missing.join("\n")
    );
}
