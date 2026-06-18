//! Brace expansion (`{a,b,c}`, `{1..5}`).
//!
//! Split out of the monolithic interpreter module; these are pure
//! `&self` helpers driven from the word-expansion path.

use super::*;

impl Interpreter {
    /// Check if a string contains glob characters
    /// Expand brace patterns like {a,b,c} or {1..5}
    /// Returns a Vec of expanded strings, or a single-element Vec if no braces
    /// THREAT[TM-DOS-042]: Cap total expansion count to prevent combinatorial OOM.
    pub(super) fn expand_braces(&self, s: &str) -> Vec<String> {
        const MAX_BRACE_EXPANSION_TOTAL: usize = 100_000;
        let mut count = 0;
        let mut bytes = 0;
        self.expand_braces_capped(s, &mut count, &mut bytes, MAX_BRACE_EXPANSION_TOTAL, 0)
    }

    /// THREAT[TM-DOS-042]: The combinatorial `count` cap alone is insufficient
    /// because it is only incremented *after* each recursive call returns, so
    /// the first DFS path descends to the full nesting/sequence depth (one
    /// level per brace group) with `count` still zero. An input like
    /// `{a,b}{a,b}...` repeated tens of thousands of times (well under
    /// `max_input_bytes`) therefore stack-overflows the worker thread, and
    /// allocates O(depth * suffix) memory, before the count cap or execution
    /// timeout can fire. The `depth` cap bounds the descent up front;
    /// legitimate scripts never approach this many nested/sequential groups.
    fn expand_braces_capped(
        &self,
        s: &str,
        count: &mut usize,
        bytes: &mut usize,
        max: usize,
        recursion_depth: usize,
    ) -> Vec<String> {
        const MAX_BRACE_EXPANSION_DEPTH: usize = 100;
        if *count >= max
            || *bytes >= Self::MAX_EXPANSION_RESULT_BYTES
            || recursion_depth >= MAX_BRACE_EXPANSION_DEPTH
        {
            return vec![s.to_string()];
        }

        // Find the first brace that has a matching close brace
        let mut depth = 0;
        let mut brace_start = None;
        let mut brace_end = None;
        let chars: Vec<char> = s.chars().collect();

        let mut escaped = false;
        for (i, &ch) in chars.iter().enumerate() {
            if escaped {
                escaped = false;
                continue;
            }
            if ch == '\\' {
                escaped = true;
                continue;
            }
            match ch {
                '{' => {
                    if depth == 0 {
                        brace_start = Some(i);
                    }
                    depth += 1;
                }
                '}' => {
                    if depth > 0 {
                        depth -= 1;
                    }
                    if depth == 0 && brace_start.is_some() {
                        brace_end = Some(i);
                        break;
                    }
                }
                _ => {}
            }
        }

        // No valid brace pattern found
        let (start, end) = match (brace_start, brace_end) {
            (Some(s), Some(e)) => (s, e),
            _ => return vec![s.to_string()],
        };

        let prefix: String = chars[..start].iter().collect();
        let suffix: String = chars[end + 1..].iter().collect();
        let brace_content: String = chars[start + 1..end].iter().collect();

        // Brace content with leading/trailing space is not expanded
        if brace_content.starts_with(' ') || brace_content.ends_with(' ') {
            return vec![s.to_string()];
        }

        // Check for range expansion like {1..5} or {a..z}
        if let Some(range_result) = self.try_expand_range(&brace_content) {
            let mut results = Vec::new();
            for item in range_result {
                if *count >= max || *bytes >= Self::MAX_EXPANSION_RESULT_BYTES {
                    break;
                }
                let expanded = format!("{}{}{}", prefix, item, suffix);
                let sub =
                    self.expand_braces_capped(&expanded, count, bytes, max, recursion_depth + 1);
                *count += sub.len();
                *bytes += sub.iter().map(String::len).sum::<usize>();
                results.extend(sub);
            }
            return results;
        }

        // List expansion like {a,b,c}
        // Need to split by comma, but respect nested braces
        let items = self.split_brace_items(&brace_content);
        if items.len() <= 1 && !brace_content.contains(',') {
            // Not a valid brace expansion (e.g., just {foo})
            return vec![s.to_string()];
        }

        let mut results = Vec::new();
        for item in items {
            if *count >= max || *bytes >= Self::MAX_EXPANSION_RESULT_BYTES {
                break;
            }
            let expanded = format!("{}{}{}", prefix, item, suffix);
            let sub = self.expand_braces_capped(&expanded, count, bytes, max, recursion_depth + 1);
            *count += sub.len();
            *bytes += sub.iter().map(String::len).sum::<usize>();
            results.extend(sub);
        }

        results
    }

    /// Try to expand a range like 1..5, a..z, or 1..10..2
    /// THREAT[TM-DOS-041]: Cap range size to prevent OOM from {1..999999999}
    pub(super) fn try_expand_range(&self, content: &str) -> Option<Vec<String>> {
        /// Maximum number of elements in a brace range expansion
        const MAX_BRACE_RANGE: u64 = 10_000;

        // Check for .. separator: accept {start..end} or {start..end..step}
        let parts: Vec<&str> = content.split("..").collect();
        if parts.len() != 2 && parts.len() != 3 {
            return None;
        }

        let start = parts[0];
        let end = parts[1];

        // Try numeric range
        if let (Ok(start_num), Ok(end_num)) = (start.parse::<i64>(), end.parse::<i64>()) {
            // Parse optional step (default: 1 or -1 based on direction)
            let step: i64 = if parts.len() == 3 {
                match parts[2].parse::<i64>() {
                    Ok(0) => return None, // step=0 is invalid
                    Ok(s) => s,
                    Err(_) => return None,
                }
            } else if start_num <= end_num {
                1
            } else {
                -1
            };

            let abs_step = step.unsigned_abs() as u128;
            let abs_diff = (end_num as i128 - start_num as i128).unsigned_abs();
            let range_size = abs_diff / abs_step + 1;
            if range_size > MAX_BRACE_RANGE as u128 {
                return None; // Treat as literal — too large
            }

            let mut results = Vec::new();
            // Bash behavior: direction is determined by start/end. Keep stepping in i128 so
            // huge but valid i64 steps cannot overflow after the precomputed range cap passes.
            let step_magnitude = step.unsigned_abs() as i128;
            let effective_step = if start_num <= end_num {
                step_magnitude
            } else {
                -step_magnitude
            };

            let mut i = start_num as i128;
            let end = end_num as i128;
            if effective_step > 0 {
                while i <= end {
                    results.push(i.to_string());
                    i += effective_step;
                }
            } else {
                while i >= end {
                    results.push(i.to_string());
                    i += effective_step;
                }
            }
            return Some(results);
        }

        // Try character range (single chars only)
        if start.len() == 1 && end.len() == 1 {
            let start_char = start.chars().next().unwrap();
            let end_char = end.chars().next().unwrap();

            if start_char.is_ascii_alphabetic() && end_char.is_ascii_alphabetic() {
                let step: i64 = if parts.len() == 3 {
                    match parts[2].parse::<i64>() {
                        Ok(0) => return None,
                        Ok(s) => s,
                        Err(_) => return None,
                    }
                } else {
                    1
                };
                let abs_step = step.unsigned_abs();

                let mut results = Vec::new();
                let start_byte = u64::from(start_char as u8);
                let end_byte = u64::from(end_char as u8);

                if start_byte <= end_byte {
                    let mut b = start_byte;
                    while b <= end_byte {
                        results.push(((b as u8) as char).to_string());
                        b = match b.checked_add(abs_step) {
                            Some(v) => v,
                            None => break,
                        };
                    }
                } else {
                    let mut b = start_byte;
                    while b >= end_byte {
                        results.push(((b as u8) as char).to_string());
                        b = match b.checked_sub(abs_step) {
                            Some(v) => v,
                            None => break,
                        };
                    }
                }
                return Some(results);
            }
        }

        None
    }

    /// Split brace content by commas, respecting nested braces
    fn split_brace_items(&self, content: &str) -> Vec<String> {
        let mut items = Vec::new();
        let mut current = String::new();
        let mut depth = 0;

        for ch in content.chars() {
            match ch {
                '{' => {
                    depth += 1;
                    current.push(ch);
                }
                '}' => {
                    depth -= 1;
                    current.push(ch);
                }
                ',' if depth == 0 => {
                    items.push(current);
                    current = String::new();
                }
                _ => {
                    current.push(ch);
                }
            }
        }
        items.push(current);

        items
    }
}
