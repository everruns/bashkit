//! Arithmetic expansion and evaluation (`$(( ))`, `(( ))`, `let`).
//!
//! Split out of interpreter/mod.rs. Recursive-descent evaluator plus the
//! variable/brace/param expansion helpers that feed it. Uses the parent's
//! `ArithmeticExpansionState` and `MAX_ARITHMETIC_EXPANSION_*` consts.

use super::*;

impl Interpreter {
    /// Evaluate arithmetic with assignment support (e.g. `X = X + 1`).
    /// Assignment must be handled before variable expansion so the LHS
    /// variable name is preserved.
    pub(super) fn evaluate_arithmetic_with_assign(&mut self, expr: &str) -> i64 {
        let expr = expr.trim();

        // Handle comma operator (lowest precedence): evaluate all, return last
        // But not inside parentheses
        {
            let mut depth = 0i32;
            let chars: Vec<char> = expr.chars().collect();
            let byte_offsets: Vec<usize> = expr.char_indices().map(|(b, _)| b).collect();
            for i in (0..chars.len()).rev() {
                match chars[i] {
                    '(' => depth += 1,
                    ')' => depth -= 1,
                    ',' if depth == 0 => {
                        let left = &expr[..byte_offsets[i]];
                        let right = &expr[byte_offsets[i] + 1..];
                        self.evaluate_arithmetic_with_assign(left);
                        return self.evaluate_arithmetic_with_assign(right);
                    }
                    _ => {}
                }
            }
        }

        // Handle pre-increment/pre-decrement: ++var, --var
        if let Some(var_name) = expr.strip_prefix("++") {
            let var_name = var_name.trim();
            if is_valid_var_name(var_name) {
                let val = self.expand_variable(var_name).parse::<i64>().unwrap_or(0) + 1;
                self.set_variable(var_name.to_string(), val.to_string());
                return val;
            }
        }
        if let Some(var_name) = expr.strip_prefix("--") {
            let var_name = var_name.trim();
            if is_valid_var_name(var_name) {
                let val = self.expand_variable(var_name).parse::<i64>().unwrap_or(0) - 1;
                self.set_variable(var_name.to_string(), val.to_string());
                return val;
            }
        }

        // Handle post-increment/post-decrement: var++, var--
        if let Some(var_name) = expr.strip_suffix("++") {
            let var_name = var_name.trim();
            if is_valid_var_name(var_name) {
                let old_val = self.expand_variable(var_name).parse::<i64>().unwrap_or(0);
                self.set_variable(var_name.to_string(), (old_val + 1).to_string());
                return old_val;
            }
        }
        if let Some(var_name) = expr.strip_suffix("--") {
            let var_name = var_name.trim();
            if is_valid_var_name(var_name) {
                let old_val = self.expand_variable(var_name).parse::<i64>().unwrap_or(0);
                self.set_variable(var_name.to_string(), (old_val - 1).to_string());
                return old_val;
            }
        }

        // Check for compound assignments: +=, -=, *=, /=, %=, &=, |=, ^=, <<=, >>=
        // and simple assignment: VAR = expr (but not == comparison)
        if let Some(eq_pos) = expr.find('=') {
            let before = &expr[..eq_pos];
            let after_char = expr.as_bytes().get(eq_pos + 1);
            // Not == or !=
            if !before.ends_with('!') && after_char != Some(&b'=') {
                // Detect compound operator: check multi-char ops first
                let (var_name, op) = if let Some(s) = before.strip_suffix("<<") {
                    (s.trim(), "<<")
                } else if let Some(s) = before.strip_suffix(">>") {
                    (s.trim(), ">>")
                } else if let Some(s) = before.strip_suffix('+') {
                    (s.trim(), "+")
                } else if let Some(s) = before.strip_suffix('-') {
                    (s.trim(), "-")
                } else if let Some(s) = before.strip_suffix('*') {
                    (s.trim(), "*")
                } else if let Some(s) = before.strip_suffix('/') {
                    (s.trim(), "/")
                } else if let Some(s) = before.strip_suffix('%') {
                    (s.trim(), "%")
                } else if let Some(s) = before.strip_suffix('&') {
                    (s.trim(), "&")
                } else if let Some(s) = before.strip_suffix('|') {
                    (s.trim(), "|")
                } else if let Some(s) = before.strip_suffix('^') {
                    (s.trim(), "^")
                } else if !before.ends_with('<') && !before.ends_with('>') {
                    (before.trim(), "")
                } else {
                    ("", "")
                };

                if is_valid_var_name(var_name) {
                    let rhs = &expr[eq_pos + 1..];
                    let rhs_val = self.evaluate_arithmetic(rhs);
                    let value = if op.is_empty() {
                        rhs_val
                    } else {
                        let lhs_val = self.expand_variable(var_name).parse::<i64>().unwrap_or(0);
                        // THREAT[TM-DOS-043]: wrapping to prevent overflow panic
                        match op {
                            "+" => lhs_val.wrapping_add(rhs_val),
                            "-" => lhs_val.wrapping_sub(rhs_val),
                            "*" => lhs_val.wrapping_mul(rhs_val),
                            "/" => {
                                if rhs_val != 0 && !(lhs_val == i64::MIN && rhs_val == -1) {
                                    lhs_val / rhs_val
                                } else {
                                    0
                                }
                            }
                            "%" => {
                                if rhs_val != 0 && !(lhs_val == i64::MIN && rhs_val == -1) {
                                    lhs_val % rhs_val
                                } else {
                                    0
                                }
                            }
                            "&" => lhs_val & rhs_val,
                            "|" => lhs_val | rhs_val,
                            "^" => lhs_val ^ rhs_val,
                            "<<" => lhs_val.wrapping_shl((rhs_val & 63) as u32),
                            ">>" => lhs_val.wrapping_shr((rhs_val & 63) as u32),
                            _ => rhs_val,
                        }
                    };
                    self.set_variable(var_name.to_string(), value.to_string());
                    return value;
                }
            }
        }

        self.evaluate_arithmetic(expr)
    }

    /// Evaluate a simple arithmetic expression
    pub(super) fn evaluate_arithmetic(&self, expr: &str) -> i64 {
        self.evaluate_arithmetic_depth(expr, 0)
    }

    /// Evaluate arithmetic while carrying recursion depth from caller contexts.
    /// THREAT[TM-DOS-026]: Preserves the recursion guard across nested array index eval.
    pub(super) fn evaluate_arithmetic_depth(&self, expr: &str, depth: usize) -> i64 {
        let mut state = ArithmeticExpansionState::new(Self::MAX_ARITHMETIC_EXPANSION_FUEL);
        self.evaluate_arithmetic_depth_state(expr, depth, &mut state)
    }

    pub(super) fn evaluate_arithmetic_depth_state(
        &self,
        expr: &str,
        depth: usize,
        state: &mut ArithmeticExpansionState,
    ) -> i64 {
        if depth >= Self::MAX_ARITHMETIC_DEPTH || !state.spend(expr.len().max(1)) {
            return 0;
        }
        // Simple arithmetic evaluation - handles basic operations
        let expr = expr.trim();

        // First expand any variables in the expression
        let expanded = self.expand_arithmetic_vars_depth_state(expr, depth + 1, state);
        if expanded.len() > Self::MAX_ARITHMETIC_EXPANSION_BYTES {
            return 0;
        }

        // Parse and evaluate with depth tracking (TM-DOS-026)
        self.parse_arithmetic_impl(&expanded, depth + 1)
    }

    /// Recursively resolve a variable value in arithmetic context.
    /// In bash arithmetic, bare variable names are recursively evaluated:
    /// if b=a and a=3, then $((b)) evaluates b -> "a" -> 3.
    /// If x='1 + 2', then $((x)) evaluates x -> "1 + 2" -> 3 (as sub-expression).
    /// THREAT[TM-DOS-026]: `depth` prevents infinite recursion.
    pub(super) fn resolve_arith_var(
        &self,
        value: &str,
        depth: usize,
        state: &mut ArithmeticExpansionState,
    ) -> String {
        if depth >= Self::MAX_ARITHMETIC_DEPTH || !state.spend(value.len().max(1)) {
            return "0".to_string();
        }
        let trimmed = value.trim();
        if trimmed.is_empty() {
            return "0".to_string();
        }
        // If value is a simple integer, return it directly
        if trimmed.parse::<i64>().is_ok() {
            return trimmed.to_string();
        }
        // If value looks like a variable name, recursively dereference
        if is_valid_var_name(trimmed) {
            let inner = self.expand_variable(trimmed);
            return self.resolve_arith_named_var(trimmed, &inner, depth + 1, state);
        }
        // Value contains an expression (e.g. "1 + 2") — expand vars in it
        // and wrap in parens to preserve grouping
        let expanded = self.expand_arithmetic_vars_depth_state(trimmed, depth + 1, state);
        if expanded.len() > Self::MAX_ARITHMETIC_EXPANSION_BYTES {
            return "0".to_string();
        }
        format!("({})", expanded)
    }

    pub(super) fn resolve_arith_named_var(
        &self,
        name: &str,
        value: &str,
        depth: usize,
        state: &mut ArithmeticExpansionState,
    ) -> String {
        if !state.enter_var(name) {
            return "0".to_string();
        }
        let resolved = self.resolve_arith_var(value, depth, state);
        state.exit_var();
        resolved
    }

    /// Expand variables in arithmetic expression (no $ needed in $((...))).
    /// THREAT[TM-DOS-026]: `depth` prevents stack overflow via recursive variable values.
    pub(super) fn expand_arithmetic_vars_depth_state(
        &self,
        expr: &str,
        depth: usize,
        state: &mut ArithmeticExpansionState,
    ) -> String {
        if depth >= Self::MAX_ARITHMETIC_DEPTH || !state.spend(expr.len().max(1)) {
            return "0".to_string();
        }

        // Strip double quotes — "$x" in arithmetic is the same as $x
        let expr = expr.replace('"', "");

        let mut result = String::new();
        let mut chars = expr.chars().peekable();
        // Track whether we're in a numeric literal context (after # or 0x)
        let mut in_numeric_literal = false;

        while let Some(ch) = chars.next() {
            if ch == '$' {
                in_numeric_literal = false;
                if chars.peek() == Some(&'{') {
                    // Handle ${...} syntax inside arithmetic
                    chars.next(); // consume '{'
                    let mut brace_content = String::new();
                    let mut brace_depth = 1i32;
                    while let Some(&c) = chars.peek() {
                        chars.next();
                        if c == '{' {
                            brace_depth += 1;
                            brace_content.push(c);
                        } else if c == '}' {
                            brace_depth -= 1;
                            if brace_depth == 0 {
                                break;
                            }
                            brace_content.push(c);
                        } else {
                            brace_content.push(c);
                        }
                    }
                    let expanded =
                        self.expand_brace_expr_in_arithmetic(&brace_content, depth + 1, state);
                    if expanded.is_empty() {
                        result.push('0');
                    } else {
                        result.push_str(&expanded);
                    }
                } else if let Some(&c) = chars.peek()
                    && matches!(c, '#' | '?' | '$' | '!' | '@' | '*' | '-')
                {
                    // Handle special variables: $#, $?, $$, $!, $@, $*, $-
                    chars.next();
                    let value = self.expand_variable(&c.to_string());
                    if value.is_empty() {
                        result.push('0');
                    } else {
                        result.push_str(&value);
                    }
                } else {
                    // Handle $var syntax (common in arithmetic)
                    let mut name = String::new();
                    while let Some(&c) = chars.peek() {
                        if c.is_ascii_alphanumeric() || c == '_' {
                            name.push(chars.next().unwrap());
                        } else {
                            break;
                        }
                    }
                    if !name.is_empty() {
                        // $var is direct text substitution — no recursive arithmetic eval.
                        // Only bare names (without $) get recursive resolution.
                        let value = self.expand_variable(&name);
                        if value.is_empty() {
                            result.push('0');
                        } else {
                            result.push_str(&value);
                        }
                    } else {
                        result.push(ch);
                    }
                }
            } else if ch == '#' {
                // base#value syntax: digits before # are base, chars after are literal digits
                result.push(ch);
                in_numeric_literal = true;
            } else if in_numeric_literal && (ch.is_ascii_alphanumeric() || ch == '_') {
                // Part of a base#value literal — don't expand as variable
                result.push(ch);
            } else if ch.is_ascii_digit() {
                result.push(ch);
                // Check for 0x/0X hex prefix
                if ch == '0'
                    && let Some(&next) = chars.peek()
                    && (next == 'x' || next == 'X')
                {
                    result.push(chars.next().unwrap());
                    in_numeric_literal = true;
                }
            } else if ch.is_ascii_alphabetic() || ch == '_' {
                in_numeric_literal = false;
                // Could be a variable name
                let mut name = String::new();
                name.push(ch);
                while let Some(&c) = chars.peek() {
                    if c.is_ascii_alphanumeric() || c == '_' {
                        name.push(chars.next().unwrap());
                    } else {
                        break;
                    }
                }

                if chars.peek() == Some(&'[') {
                    // Check for array access: name[expr]
                    chars.next(); // consume '['
                    let mut index_expr = String::new();
                    let mut bracket_depth = 1;
                    while let Some(&c) = chars.peek() {
                        chars.next();
                        if c == '[' {
                            bracket_depth += 1;
                            index_expr.push(c);
                        } else if c == ']' {
                            bracket_depth -= 1;
                            if bracket_depth == 0 {
                                break;
                            }
                            index_expr.push(c);
                        } else {
                            index_expr.push(c);
                        }
                    }
                    // Evaluate the index expression as arithmetic
                    let idx = self.evaluate_arithmetic_depth_state(&index_expr, depth + 1, state);
                    // Look up array element
                    if let Some(arr) = self.arrays.get(&name) {
                        let idx_usize: usize = idx.try_into().unwrap_or(0);
                        let value = arr.get(&idx_usize).cloned().unwrap_or_default();
                        result.push_str(&self.resolve_arith_var(&value, depth, state));
                    } else {
                        // Not an array — treat as scalar (index 0 returns the var value)
                        let value = self.expand_variable(&name);
                        if idx == 0 {
                            result.push_str(&self.resolve_arith_var(&value, depth, state));
                        } else {
                            result.push('0');
                        }
                    }
                } else {
                    // Expand the variable with recursive arithmetic resolution
                    let value = self.expand_variable(&name);
                    result.push_str(&self.resolve_arith_named_var(&name, &value, depth, state));
                }
            } else {
                in_numeric_literal = false;
                result.push(ch);
            }
            if result.len() > Self::MAX_ARITHMETIC_EXPANSION_BYTES {
                return "0".to_string();
            }
        }

        result
    }

    /// Expand a `${...}` expression encountered inside arithmetic context.
    /// Handles: `${#arr[@]}`, `${#arr[*]}`, `${#var}`, `${arr[idx]}`, `${var}`.
    pub(super) fn expand_brace_expr_in_arithmetic(
        &self,
        inner: &str,
        depth: usize,
        state: &mut ArithmeticExpansionState,
    ) -> String {
        // ${#arr[@]} or ${#arr[*]} — array length
        if let Some(rest) = inner.strip_prefix('#') {
            if let Some(bracket) = rest.find('[') {
                // Require a closing ']' — anything else (e.g. `${#arr[` with
                // an unterminated index, or `${#arr[禧` whose final byte sits
                // inside a multi-byte UTF-8 char) is malformed. Without this
                // guard `end = rest.len() - 1` could land mid-codepoint and
                // panic the slice below.
                if !rest.ends_with(']') {
                    return "0".to_string();
                }
                let end = rest.len() - 1;
                if bracket + 1 > end {
                    // Malformed — treat as string length of empty var
                    return "0".to_string();
                }
                let arr_name = &rest[..bracket];
                let idx = &rest[bracket + 1..end];
                if idx == "@" || idx == "*" {
                    if let Some(arr) = self.arrays.get(arr_name) {
                        return arr.len().to_string();
                    }
                    if let Some(arr) = self.assoc_arrays.get(arr_name) {
                        return arr.len().to_string();
                    }
                    return "0".to_string();
                }
                // ${#arr[n]} — length of element
                let idx_val = self.evaluate_arithmetic_depth_state(idx, depth + 1, state);
                let idx_usize: usize = idx_val.try_into().unwrap_or(0);
                if let Some(arr) = self.arrays.get(arr_name) {
                    return arr
                        .get(&idx_usize)
                        .map(|v| v.len().to_string())
                        .unwrap_or_else(|| "0".to_string());
                }
                return "0".to_string();
            }
            // ${#var} — string length
            let val = self.expand_variable(rest);
            return val.len().to_string();
        }

        // ${arr[idx]} — array access
        if let Some(bracket) = inner.find('[')
            && inner.ends_with(']')
        {
            let arr_name = &inner[..bracket];
            let idx_str = &inner[bracket + 1..inner.len() - 1];
            if let Some(arr) = self.assoc_arrays.get(arr_name) {
                let key = self.expand_variable_or_literal(idx_str);
                return arr.get(&key).cloned().unwrap_or_default();
            }
            if let Some(arr) = self.arrays.get(arr_name) {
                let idx_val = self.evaluate_arithmetic_depth_state(idx_str, depth + 1, state);
                let idx_usize: usize = idx_val.try_into().unwrap_or(0);
                return arr.get(&idx_usize).cloned().unwrap_or_default();
            }
            return String::new();
        }

        // Check for parameter expansion operators (%, %%, #, ##, :-, etc.)
        // If present, handle expansion with the operator applied.
        let has_operator = inner.contains("%%")
            || inner.contains('%')
            || (inner.contains('#') && !inner.starts_with('#'))
            || inner.contains(":-");
        if has_operator {
            return self.expand_param_op_in_arithmetic(inner);
        }

        // ${var} — plain variable
        self.expand_variable(inner)
    }

    /// Expand a parameter expansion with operators inside arithmetic context.
    /// Handles common cases like ${var%%-*}, ${var##prefix}, etc.
    pub(super) fn expand_param_op_in_arithmetic(&self, inner: &str) -> String {
        for (pos, ch) in inner.char_indices() {
            match ch {
                '%' => {
                    let name = &inner[..pos];
                    let value = self.expand_name_or_array_element(name);
                    if inner[pos..].starts_with("%%") {
                        let pattern = &inner[pos + 2..];
                        return self.remove_pattern(&value, pattern, false, true);
                    }
                    let pattern = &inner[pos + 1..];
                    return self.remove_pattern(&value, pattern, false, false);
                }
                '#' if pos > 0 => {
                    let name = &inner[..pos];
                    let value = self.expand_name_or_array_element(name);
                    if inner[pos..].starts_with("##") {
                        let pattern = &inner[pos + 2..];
                        return self.remove_pattern(&value, pattern, true, true);
                    }
                    let pattern = &inner[pos + 1..];
                    return self.remove_pattern(&value, pattern, true, false);
                }
                ':' if inner[pos..].starts_with(":-") => {
                    let name = &inner[..pos];
                    let default = &inner[pos + 2..];
                    let value = self.expand_name_or_array_element(name);
                    if value.is_empty() {
                        return default.to_string();
                    }
                    return value;
                }
                _ => {}
            }
        }
        // Fallback
        self.expand_name_or_array_element(inner)
    }

    /// Resolve `name` or `arr[idx]` to its current string value.
    /// Used by parameter expansion inside arithmetic so `${arr[$key]:-N}` and
    /// friends can read associative/indexed array elements — `expand_variable`
    /// alone only handles scalar names. Fixes issue #1776.
    pub(super) fn expand_name_or_array_element(&self, name: &str) -> String {
        if let Some(bracket) = name.find('[')
            && name.ends_with(']')
        {
            let arr_name = &name[..bracket];
            let resolved = self.resolve_nameref(arr_name);
            let idx_str = &name[bracket + 1..name.len() - 1];
            if let Some(arr) = self.assoc_arrays.get(resolved) {
                let key = self.expand_variable_or_literal(idx_str);
                return arr.get(&key).cloned().unwrap_or_default();
            }
            if let Some(arr) = self.arrays.get(resolved) {
                let idx_val = self.evaluate_arithmetic(idx_str);
                let idx_usize: usize = idx_val.try_into().unwrap_or(0);
                return arr.get(&idx_usize).cloned().unwrap_or_default();
            }
            return String::new();
        }
        self.expand_variable(name)
    }

    /// Parse and evaluate a simple arithmetic expression with depth tracking.
    /// THREAT[TM-DOS-026]: `arith_depth` prevents stack overflow from deeply nested expressions.
    /// Parse an arithmetic atom: unary operators, parenthesized expressions, and literals.
    pub(super) fn parse_arith_atom(&self, expr: &str, arith_depth: usize) -> i64 {
        // Unary negation and bitwise NOT
        if let Some(rest) = expr.strip_prefix('-') {
            let rest = rest.trim();
            if !rest.is_empty() {
                // THREAT[TM-DOS-029]: wrapping to prevent i64::MIN negation panic
                return self
                    .parse_arithmetic_impl(rest, arith_depth + 1)
                    .wrapping_neg();
            }
        }
        if let Some(rest) = expr.strip_prefix('~') {
            let rest = rest.trim();
            if !rest.is_empty() {
                return !self.parse_arithmetic_impl(rest, arith_depth + 1);
            }
        }
        if let Some(rest) = expr.strip_prefix('!') {
            let rest = rest.trim();
            if !rest.is_empty() {
                let val = self.parse_arithmetic_impl(rest, arith_depth + 1);
                return if val == 0 { 1 } else { 0 };
            }
        }

        // Base conversion: base#value (e.g., 16#ff = 255, 2#1010 = 10)
        if let Some(hash_pos) = expr.find('#') {
            let base_str = &expr[..hash_pos];
            let value_str = &expr[hash_pos + 1..];
            if let Ok(base) = base_str.parse::<u32>() {
                if (2..=36).contains(&base) {
                    return i64::from_str_radix(value_str, base).unwrap_or(0);
                } else if (37..=64).contains(&base) {
                    return Self::parse_base_n(value_str, base);
                }
            }
        }

        // Hex (0x...), octal (0...) literals
        if expr.starts_with("0x") || expr.starts_with("0X") {
            return i64::from_str_radix(&expr[2..], 16).unwrap_or(0);
        }
        if expr.starts_with('0') && expr.len() > 1 && expr.chars().all(|c| c.is_ascii_digit()) {
            return i64::from_str_radix(&expr[1..], 8).unwrap_or(0);
        }

        // Parse as number or variable
        expr.trim().parse().unwrap_or(0)
    }

    /// Try to parse a binary operator at the current precedence level.
    /// Scans `chars`/`bo` for operators, splitting and recursing.
    /// Returns `Some(value)` if an operator was found, `None` to try next level.
    pub(super) fn try_parse_arith_addmul(
        &self,
        expr: &str,
        chars: &[char],
        bo: &[usize],
        arith_depth: usize,
    ) -> Option<i64> {
        let mut depth: i32 = 0;

        // Addition/Subtraction
        for i in (0..chars.len()).rev() {
            match chars[i] {
                '(' => depth += 1,
                ')' => depth -= 1,
                '+' | '-' if depth == 0 && i > 0 => {
                    if chars[i] == '+' && i + 1 < chars.len() && chars[i + 1] == '+' {
                        continue;
                    }
                    if chars[i] == '+' && i > 0 && chars[i - 1] == '+' {
                        continue;
                    }
                    if chars[i] == '-' && i + 1 < chars.len() && chars[i + 1] == '-' {
                        continue;
                    }
                    if chars[i] == '-' && i > 0 && chars[i - 1] == '-' {
                        continue;
                    }
                    let left = self.parse_arithmetic_impl(&expr[..bo[i]], arith_depth + 1);
                    let right = self.parse_arithmetic_impl(&expr[bo[i] + 1..], arith_depth + 1);
                    return Some(if chars[i] == '+' {
                        left.wrapping_add(right)
                    } else {
                        left.wrapping_sub(right)
                    });
                }
                _ => {}
            }
        }

        // Multiplication/Division/Modulo
        depth = 0;
        for i in (0..chars.len()).rev() {
            match chars[i] {
                '(' => depth += 1,
                ')' => depth -= 1,
                '*' if depth == 0 => {
                    if i + 1 < chars.len() && chars[i + 1] == '*' {
                        continue;
                    }
                    if i > 0 && chars[i - 1] == '*' {
                        continue;
                    }
                    let left = self.parse_arithmetic_impl(&expr[..bo[i]], arith_depth + 1);
                    let right = self.parse_arithmetic_impl(&expr[bo[i] + 1..], arith_depth + 1);
                    return Some(left.wrapping_mul(right));
                }
                '/' | '%' if depth == 0 => {
                    let left = self.parse_arithmetic_impl(&expr[..bo[i]], arith_depth + 1);
                    let right = self.parse_arithmetic_impl(&expr[bo[i] + 1..], arith_depth + 1);
                    return Some(match chars[i] {
                        '/' if right != 0 => left.wrapping_div(right),
                        '%' if right != 0 => left.wrapping_rem(right),
                        _ => 0,
                    });
                }
                _ => {}
            }
        }

        // Exponentiation ** (right-associative)
        depth = 0;
        for i in 0..chars.len() {
            match chars[i] {
                '(' => depth += 1,
                ')' => depth -= 1,
                '*' if depth == 0 && i + 1 < chars.len() && chars[i + 1] == '*' => {
                    let left = self.parse_arithmetic_impl(&expr[..bo[i]], arith_depth + 1);
                    let right = self.parse_arithmetic_impl(&expr[bo[i] + 2..], arith_depth + 1);
                    let exp = right.clamp(0, 63) as u32;
                    return Some(left.wrapping_pow(exp));
                }
                _ => {}
            }
        }

        None
    }

    /// Try to parse comparison and logical/bitwise operators.
    pub(super) fn try_parse_arith_comparison(
        &self,
        expr: &str,
        chars: &[char],
        bo: &[usize],
        arith_depth: usize,
    ) -> Option<i64> {
        let mut depth: i32 = 0;

        // Ternary operator (lowest precedence)
        for i in 0..chars.len() {
            match chars[i] {
                '(' => depth += 1,
                ')' => depth -= 1,
                '?' if depth == 0 => {
                    let mut colon_depth = 0;
                    for j in (i + 1)..chars.len() {
                        match chars[j] {
                            '(' => colon_depth += 1,
                            ')' => colon_depth -= 1,
                            '?' => colon_depth += 1,
                            ':' if colon_depth == 0 => {
                                let cond =
                                    self.parse_arithmetic_impl(&expr[..bo[i]], arith_depth + 1);
                                let then_val = self.parse_arithmetic_impl(
                                    &expr[bo[i] + 1..bo[j]],
                                    arith_depth + 1,
                                );
                                let else_val =
                                    self.parse_arithmetic_impl(&expr[bo[j] + 1..], arith_depth + 1);
                                return Some(if cond != 0 { then_val } else { else_val });
                            }
                            ':' => colon_depth -= 1,
                            _ => {}
                        }
                    }
                }
                _ => {}
            }
        }

        // Logical OR (||)
        depth = 0;
        for i in (0..chars.len()).rev() {
            match chars[i] {
                '(' => depth += 1,
                ')' => depth -= 1,
                '|' if depth == 0 && i > 0 && chars[i - 1] == '|' => {
                    let left = self.parse_arithmetic_impl(&expr[..bo[i - 1]], arith_depth + 1);
                    if left != 0 {
                        return Some(1);
                    }
                    let right = self.parse_arithmetic_impl(&expr[bo[i] + 1..], arith_depth + 1);
                    return Some(if right != 0 { 1 } else { 0 });
                }
                _ => {}
            }
        }

        // Logical AND (&&)
        depth = 0;
        for i in (0..chars.len()).rev() {
            match chars[i] {
                '(' => depth += 1,
                ')' => depth -= 1,
                '&' if depth == 0 && i > 0 && chars[i - 1] == '&' => {
                    let left = self.parse_arithmetic_impl(&expr[..bo[i - 1]], arith_depth + 1);
                    if left == 0 {
                        return Some(0);
                    }
                    let right = self.parse_arithmetic_impl(&expr[bo[i] + 1..], arith_depth + 1);
                    return Some(if right != 0 { 1 } else { 0 });
                }
                _ => {}
            }
        }

        // Bitwise OR (|) - but not ||
        depth = 0;
        for i in (0..chars.len()).rev() {
            match chars[i] {
                '(' => depth += 1,
                ')' => depth -= 1,
                '|' if depth == 0
                    && (i == 0 || chars[i - 1] != '|')
                    && (i + 1 >= chars.len() || chars[i + 1] != '|') =>
                {
                    let left = self.parse_arithmetic_impl(&expr[..bo[i]], arith_depth + 1);
                    let right = self.parse_arithmetic_impl(&expr[bo[i] + 1..], arith_depth + 1);
                    return Some(left | right);
                }
                _ => {}
            }
        }

        // Bitwise XOR (^)
        depth = 0;
        for i in (0..chars.len()).rev() {
            match chars[i] {
                '(' => depth += 1,
                ')' => depth -= 1,
                '^' if depth == 0 => {
                    let left = self.parse_arithmetic_impl(&expr[..bo[i]], arith_depth + 1);
                    let right = self.parse_arithmetic_impl(&expr[bo[i] + 1..], arith_depth + 1);
                    return Some(left ^ right);
                }
                _ => {}
            }
        }

        // Bitwise AND (&) - but not &&
        depth = 0;
        for i in (0..chars.len()).rev() {
            match chars[i] {
                '(' => depth += 1,
                ')' => depth -= 1,
                '&' if depth == 0
                    && (i == 0 || chars[i - 1] != '&')
                    && (i + 1 >= chars.len() || chars[i + 1] != '&') =>
                {
                    let left = self.parse_arithmetic_impl(&expr[..bo[i]], arith_depth + 1);
                    let right = self.parse_arithmetic_impl(&expr[bo[i] + 1..], arith_depth + 1);
                    return Some(left & right);
                }
                _ => {}
            }
        }

        // Equality operators (==, !=)
        depth = 0;
        for i in (0..chars.len()).rev() {
            match chars[i] {
                '(' => depth += 1,
                ')' => depth -= 1,
                '=' if depth == 0 && i > 0 && chars[i - 1] == '=' => {
                    let left = self.parse_arithmetic_impl(&expr[..bo[i - 1]], arith_depth + 1);
                    let right = self.parse_arithmetic_impl(&expr[bo[i] + 1..], arith_depth + 1);
                    return Some(if left == right { 1 } else { 0 });
                }
                '=' if depth == 0 && i > 0 && chars[i - 1] == '!' => {
                    let left = self.parse_arithmetic_impl(&expr[..bo[i - 1]], arith_depth + 1);
                    let right = self.parse_arithmetic_impl(&expr[bo[i] + 1..], arith_depth + 1);
                    return Some(if left != right { 1 } else { 0 });
                }
                _ => {}
            }
        }

        // Relational operators (<, >, <=, >=)
        depth = 0;
        for i in (0..chars.len()).rev() {
            match chars[i] {
                '(' => depth += 1,
                ')' => depth -= 1,
                '=' if depth == 0 && i > 0 && chars[i - 1] == '<' => {
                    let left = self.parse_arithmetic_impl(&expr[..bo[i - 1]], arith_depth + 1);
                    let right = self.parse_arithmetic_impl(&expr[bo[i] + 1..], arith_depth + 1);
                    return Some(if left <= right { 1 } else { 0 });
                }
                '=' if depth == 0 && i > 0 && chars[i - 1] == '>' => {
                    let left = self.parse_arithmetic_impl(&expr[..bo[i - 1]], arith_depth + 1);
                    let right = self.parse_arithmetic_impl(&expr[bo[i] + 1..], arith_depth + 1);
                    return Some(if left >= right { 1 } else { 0 });
                }
                '<' if depth == 0
                    && (i + 1 >= chars.len() || (chars[i + 1] != '=' && chars[i + 1] != '<'))
                    && (i == 0 || chars[i - 1] != '<') =>
                {
                    let left = self.parse_arithmetic_impl(&expr[..bo[i]], arith_depth + 1);
                    let right = self.parse_arithmetic_impl(&expr[bo[i] + 1..], arith_depth + 1);
                    return Some(if left < right { 1 } else { 0 });
                }
                '>' if depth == 0
                    && (i + 1 >= chars.len() || (chars[i + 1] != '=' && chars[i + 1] != '>'))
                    && (i == 0 || chars[i - 1] != '>') =>
                {
                    let left = self.parse_arithmetic_impl(&expr[..bo[i]], arith_depth + 1);
                    let right = self.parse_arithmetic_impl(&expr[bo[i] + 1..], arith_depth + 1);
                    return Some(if left > right { 1 } else { 0 });
                }
                _ => {}
            }
        }

        // Bitwise shift (<< >>)
        depth = 0;
        for i in (0..chars.len()).rev() {
            match chars[i] {
                '(' => depth += 1,
                ')' => depth -= 1,
                '<' if depth == 0
                    && i > 0
                    && chars[i - 1] == '<'
                    && (i < 2 || chars[i - 2] != '<')
                    && (i + 1 >= chars.len() || chars[i + 1] != '=') =>
                {
                    let left = self.parse_arithmetic_impl(&expr[..bo[i - 1]], arith_depth + 1);
                    let right = self.parse_arithmetic_impl(&expr[bo[i] + 1..], arith_depth + 1);
                    let shift = right.clamp(0, 63) as u32;
                    return Some(left.wrapping_shl(shift));
                }
                '>' if depth == 0
                    && i > 0
                    && chars[i - 1] == '>'
                    && (i < 2 || chars[i - 2] != '>')
                    && (i + 1 >= chars.len() || chars[i + 1] != '=') =>
                {
                    let left = self.parse_arithmetic_impl(&expr[..bo[i - 1]], arith_depth + 1);
                    let right = self.parse_arithmetic_impl(&expr[bo[i] + 1..], arith_depth + 1);
                    let shift = right.clamp(0, 63) as u32;
                    return Some(left.wrapping_shr(shift));
                }
                _ => {}
            }
        }

        None
    }

    pub(super) fn parse_arithmetic_impl(&self, expr: &str, arith_depth: usize) -> i64 {
        let expr = expr.trim();

        if expr.is_empty() {
            return 0;
        }

        if !expr.is_ascii() {
            return 0;
        }

        // THREAT[TM-DOS-026]: Bail out if arithmetic nesting is too deep
        if arith_depth >= Self::MAX_ARITHMETIC_DEPTH {
            return 0;
        }

        // Handle parentheses
        if expr.starts_with('(') && expr.ends_with(')') {
            let mut depth = 0;
            let mut balanced = true;
            for (i, ch) in expr.chars().enumerate() {
                match ch {
                    '(' => depth += 1,
                    ')' => {
                        depth -= 1;
                        if depth == 0 && i < expr.len() - 1 {
                            balanced = false;
                            break;
                        }
                    }
                    _ => {}
                }
            }
            if balanced && depth == 0 {
                return self.parse_arithmetic_impl(&expr[1..expr.len() - 1], arith_depth + 1);
            }
        }

        let chars: Vec<char> = expr.chars().collect();
        let bo: Vec<usize> = expr.char_indices().map(|(b, _)| b).collect();

        // Try comparison/logical/bitwise operators (lowest precedence first)
        if let Some(val) = self.try_parse_arith_comparison(expr, &chars, &bo, arith_depth) {
            return val;
        }

        // Try additive/multiplicative/power operators
        if let Some(val) = self.try_parse_arith_addmul(expr, &chars, &bo, arith_depth) {
            return val;
        }

        // Atom: unary operators and literals
        self.parse_arith_atom(expr, arith_depth)
    }

    /// Parse a number in base 37-64 using bash's extended charset: 0-9, a-z, A-Z, @, _
    pub(super) fn parse_base_n(value_str: &str, base: u32) -> i64 {
        let mut result: i64 = 0;
        for ch in value_str.chars() {
            let digit = match ch {
                '0'..='9' => ch as u32 - '0' as u32,
                'a'..='z' => 10 + ch as u32 - 'a' as u32,
                'A'..='Z' => 36 + ch as u32 - 'A' as u32,
                '@' => 62,
                '_' => 63,
                _ => return 0,
            };
            if digit >= base {
                return 0;
            }
            result = result.wrapping_mul(base as i64).wrapping_add(digit as i64);
        }
        result
    }
}
