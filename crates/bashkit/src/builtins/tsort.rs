//! tsort builtin - topological sort
//!
//! Decision: output order matches GNU tsort: zero-indegree nodes are seeded in
//! sorted (byte) order, and each node's successors are released most recent
//! edge first, since GNU keeps nodes in a search tree and successor lists by
//! prepending. Loops are reported on stderr (exit 1) and broken at the
//! smallest remaining node so every node is still printed, like GNU.

use std::collections::{BTreeMap, VecDeque};

use async_trait::async_trait;

use super::{Builtin, Context};
use crate::error::Result;
use crate::interpreter::ExecResult;

/// `tsort` builtin.
pub struct Tsort;

const HELP: &str = "Usage: tsort [OPTION] [FILE]\nWrite totally ordered list consistent with the partial ordering in FILE.\n\n  --help\tdisplay this help and exit\n  --version\toutput version information and exit\n";

#[derive(Default)]
struct Node {
    indegree: usize,
    successors: Vec<usize>,
}

fn tsort(input: &str, name: &str) -> (String, String, i32) {
    let tokens: Vec<&str> = input.split_whitespace().collect();
    if !tokens.len().is_multiple_of(2) {
        return (
            String::new(),
            format!("tsort: {name}: input contains an odd number of tokens\n"),
            1,
        );
    }
    let mut ids: BTreeMap<&str, usize> = BTreeMap::new();
    let mut names: Vec<&str> = Vec::new();
    let mut nodes: Vec<Node> = Vec::new();
    for pair in tokens.chunks(2) {
        let mut pair_ids = [0usize; 2];
        for (slot, tok) in pair_ids.iter_mut().zip(pair) {
            *slot = *ids.entry(tok).or_insert_with(|| {
                names.push(tok);
                nodes.push(Node::default());
                nodes.len() - 1
            });
        }
        let [a, b] = pair_ids;
        if a != b {
            nodes[b].indegree += 1;
            nodes[a].successors.push(b);
        }
    }

    // Sorted order of node ids.
    let mut sorted: Vec<usize> = (0..names.len()).collect();
    sorted.sort_by(|&x, &y| names[x].cmp(names[y]));

    let mut done = vec![false; names.len()];
    let mut out = String::new();
    let mut err = String::new();
    let mut code = 0;
    let mut queue: VecDeque<usize> = sorted
        .iter()
        .copied()
        .filter(|&n| nodes[n].indegree == 0)
        .collect();
    let mut remaining = names.len();
    let mut cursor = 0usize;
    while remaining > 0 {
        while let Some(n) = queue.pop_front() {
            if done[n] {
                continue;
            }
            done[n] = true;
            remaining -= 1;
            out.push_str(names[n]);
            out.push('\n');
            for &s in nodes[n].successors.clone().iter().rev() {
                if done[s] {
                    continue;
                }
                nodes[s].indegree = nodes[s].indegree.saturating_sub(1);
                if nodes[s].indegree == 0 {
                    queue.push_back(s);
                }
            }
        }
        if remaining == 0 {
            break;
        }
        // A loop: report it, then break it at the smallest remaining node.
        code = 1;
        // `done` only grows, so the scan cursor never moves back: O(n) total.
        while done[sorted[cursor]] {
            cursor += 1;
        }
        let start = sorted[cursor];
        err.push_str(&format!("tsort: {name}: input contains a loop:\n"));
        let mut path = vec![start];
        let mut seen = std::collections::HashSet::from([start]);
        let mut cur = start;
        while let Some(&next) = nodes[cur].successors.iter().find(|&&s| !done[s]) {
            if seen.contains(&next) {
                let from = path.iter().position(|&p| p == next).unwrap_or(0);
                for &p in &path[from..] {
                    err.push_str(&format!("tsort: {}\n", names[p]));
                }
                break;
            }
            seen.insert(next);
            path.push(next);
            cur = next;
        }
        nodes[start].indegree = 0;
        queue.push_back(start);
    }
    (out, err, code)
}

#[async_trait]
impl Builtin for Tsort {
    async fn execute(&self, ctx: Context<'_>) -> Result<ExecResult> {
        if let Some(r) = super::check_help_version(ctx.args, HELP, Some("tsort (bashkit) 0.1")) {
            return Ok(r);
        }
        let operands: Vec<&str> = ctx
            .args
            .iter()
            .map(String::as_str)
            .filter(|a| *a != "--")
            .collect();
        if let Some(bad) = operands.iter().find(|a| a.starts_with('-') && **a != "-") {
            return Ok(ExecResult::err(
                format!("tsort: unrecognized option '{bad}'\n"),
                1,
            ));
        }
        if operands.len() > 1 {
            return Ok(ExecResult::err(
                format!("tsort: extra operand '{}'\n", operands[1]),
                1,
            ));
        }
        let name = operands.first().copied().unwrap_or("-");
        let input = if name == "-" {
            ctx.stdin
                .map(|s| s.text_lossy().into_owned())
                .unwrap_or_default()
        } else {
            let path = super::resolve_path(ctx.cwd, name);
            match ctx.fs.read_file(&path).await {
                Ok(d) => String::from_utf8_lossy(&d).into_owned(),
                Err(_) => {
                    return Ok(ExecResult::err(
                        format!("tsort: {name}: No such file or directory\n"),
                        1,
                    ));
                }
            }
        };
        let (out, err, code) = tsort(&input, name);
        Ok(ExecResult {
            stdout: out.into(),
            stderr: err.into(),
            exit_code: code,
            ..Default::default()
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn matches_gnu_order() {
        assert_eq!(
            tsort("a b\nb c\na d\nd c\ne f\n", "-").0,
            "a\ne\nd\nb\nf\nc\n"
        );
        assert_eq!(tsort("c a\nb a\nz y\n", "-").0, "b\nc\nz\na\ny\n");
        assert_eq!(tsort("x x\n", "-").0, "x\n");
    }

    #[test]
    fn reports_loops_and_odd_input() {
        let (out, err, code) = tsort("a b b a", "-");
        assert_eq!((out.as_str(), code), ("a\nb\n", 1));
        assert_eq!(
            err,
            "tsort: -: input contains a loop:\ntsort: a\ntsort: b\n"
        );
        let (_, err, code) = tsort("x x y", "-");
        assert_eq!(code, 1);
        assert!(err.contains("odd number of tokens"));
    }
}
