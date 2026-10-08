//! Word and parameter expansion.
//!
//! Split out of interpreter/mod.rs: the core `expand_word` /
//! `expand_word_to_fields` pipeline, parameter-expansion operators
//! (`${x:-y}`, `${x/a/b}`, `${x#p}`, ...), IFS field splitting, operand
//! quoting, and pattern matching helpers. Command-substitution and
//! subshell-snapshot machinery stay in the parent module.

use super::*;

/// Case change for `${v^}`, `${v,}` and `${v~}`.
#[derive(Clone, Copy)]
enum CaseChange {
    Upper,
    Lower,
    Toggle,
}

impl Interpreter {
    /// Elements of `name` as `${name[@]}` sees them, as `(key, value)`:
    /// an associative array in bash's hash order, an indexed array by
    /// index, and a set scalar as its element 0 (`x=5; echo ${x[@]}
    /// ${!x[@]}` prints `5 0`). `None` when the name has no value.
    pub(super) fn array_view(&self, name: &str) -> Option<Vec<(String, String)>> {
        let name = self.resolve_nameref(name);
        if let Some(arr) = self.scoped.assoc_arrays.get(name) {
            return Some(
                super::declare::assoc_keys_bash_order(arr)
                    .into_iter()
                    .map(|k| (k.clone(), arr[k].clone()))
                    .collect(),
            );
        }
        if let Some(arr) = self.scoped.arrays.get(name) {
            let mut items: Vec<_> = arr.iter().collect();
            items.sort_unstable_by_key(|(i, _)| **i);
            return Some(
                items
                    .into_iter()
                    .map(|(i, v)| (i.to_string(), v.clone()))
                    .collect(),
            );
        }
        self.scalar_value(name).map(|v| vec![("0".to_string(), v)])
    }

    /// Values of `${name[@]}` (see [`Self::array_view`]).
    pub(super) fn array_values(&self, name: &str) -> Vec<String> {
        self.array_view(name)
            .map(|items| items.into_iter().map(|(_, v)| v).collect())
            .unwrap_or_default()
    }

    /// Keys of `${!name[@]}` (see [`Self::array_view`]).
    pub(super) fn array_keys(&self, name: &str) -> Vec<String> {
        self.array_view(name)
            .map(|items| items.into_iter().map(|(k, _)| k).collect())
            .unwrap_or_default()
    }

    /// The value of a plain (non-array) variable, if set.
    fn scalar_value(&self, name: &str) -> Option<String> {
        self.scoped
            .variables
            .get(name)
            .or_else(|| self.env.get(name))
            .cloned()
    }

    /// `${x[i]}` on a scalar `x`: element 0 (or -1) is the scalar itself.
    fn scalar_element(&self, name: &str, index: &str) -> Option<String> {
        if self.scoped.arrays.contains_key(name) || self.scoped.assoc_arrays.contains_key(name) {
            return None;
        }
        let value = self.scalar_value(name)?;
        matches!(self.evaluate_arithmetic(index), 0 | -1).then_some(value)
    }

    /// Expand an array access expression (`${arr[index]}`).
    pub(super) fn expand_array_access_part(&self, name: &str, index: &str) -> String {
        let resolved_name = self.resolve_nameref(name);
        let (arr_name, extra_index) = parse_embedded_array_ref(resolved_name)
            .map(|(arr_name, idx_part)| (arr_name, Some(idx_part.to_string())))
            .unwrap_or((resolved_name, None));

        let mut result = String::new();
        if index == "@" || index == "*" {
            let sep = if index == "*" {
                self.get_ifs_separator()
            } else {
                " ".to_string()
            };
            result.push_str(&self.array_values(arr_name).join(&sep));
        } else if let Some(extra_idx) = extra_index {
            if let Some(arr) = self.scoped.assoc_arrays.get(arr_name) {
                if let Some(value) = arr.get(&extra_idx) {
                    result.push_str(value);
                }
            } else {
                let idx: usize = self.evaluate_arithmetic(&extra_idx).try_into().unwrap_or(0);
                if let Some(arr) = self.scoped.arrays.get(arr_name)
                    && let Some(value) = arr.get(&idx)
                {
                    result.push_str(value);
                }
            }
        } else if let Some(arr) = self.scoped.assoc_arrays.get(arr_name) {
            let key = self.expand_variable_or_literal(index);
            if let Some(value) = arr.get(&key) {
                result.push_str(value);
            }
        } else if let Some(arr) = self.scoped.arrays.get(arr_name) {
            let idx = self.resolve_indexed_array_subscript(arr_name, index);
            if let Some(value) = arr.get(&idx) {
                result.push_str(value);
            }
        } else if let Some(value) = self.scalar_element(arr_name, index) {
            result.push_str(&value);
        }
        result
    }

    /// `${#v}`: characters, or bytes when the shell's locale is C/POSIX.
    /// The locale comes from `LC_ALL`, then `LC_CTYPE`, then `LANG`, the first
    /// one set and non-empty. With none set bashkit stays UTF-8 (bash would
    /// fall back to C), so only an explicit `C`/`POSIX` switches to bytes.
    pub(super) fn shell_length(&self, value: &str) -> usize {
        let locale = ["LC_ALL", "LC_CTYPE", "LANG"]
            .iter()
            .map(|name| self.expand_variable(name))
            .find(|v| !v.is_empty());
        match locale.as_deref() {
            Some("C" | "POSIX") => value.len(),
            _ => value.chars().count(),
        }
    }

    /// Apply a `${var@operator}` transformation.
    pub(super) fn apply_transformation(&self, name: &str, operator: char) -> String {
        // `${v@a}`: the variable's attribute letters, the same ones
        // `declare -p` prints. A subscript picks the array, a nameref its
        // target, and `${a[@]@a}` repeats the letters once per element.
        if operator == 'a' {
            let base = name.split('[').next().unwrap_or(name);
            let letters = self.attr_letters(self.resolve_nameref(base));
            let letters = if letters == "-" {
                String::new()
            } else {
                letters
            };
            if name.ends_with("[@]") || name.ends_with("[*]") {
                let count = self
                    .resolve_param_expansion_elements(name)
                    .map_or(0, |elems| elems.len());
                return vec![letters; count].join(" ");
            }
            return letters;
        }
        // `${a[@]@Q}`, `${@@U}`: value transforms apply to each element.
        if matches!(operator, 'Q' | 'E' | 'P' | 'u' | 'U' | 'L')
            && let Some(elems) = self.resolve_param_expansion_elements(name)
        {
            // THREAT[TM-DOS]: same output cap as per-element pattern ops.
            let mut out = String::new();
            for v in &elems {
                let t = Self::transform_value(v, operator);
                if out.len().saturating_add(t.len() + 1) > Self::MAX_EXPANSION_RESULT_BYTES {
                    return elems.join(" ");
                }
                if !out.is_empty() {
                    out.push(' ');
                }
                out.push_str(&t);
            }
            return out;
        }
        let value = if name.contains('[') {
            self.resolve_param_expansion_name(name).1
        } else {
            self.expand_variable(name)
        };
        match operator {
            'A' => format!("{}='{}'", name, value.replace('\'', "'\\''")),
            _ => Self::transform_value(&value, operator),
        }
    }

    /// The `${v@op}` transforms that depend only on the value.
    fn transform_value(value: &str, operator: char) -> String {
        match operator {
            'Q' => format!("'{}'", value.replace('\'', "'\\''")),
            'E' => value
                .replace("\\n", "\n")
                .replace("\\t", "\t")
                .replace("\\\\", "\\"),
            'U' => value.to_uppercase(),
            'u' => {
                let mut chars = value.chars();
                match chars.next() {
                    Some(first) => first.to_uppercase().collect::<String>() + chars.as_str(),
                    None => String::new(),
                }
            }
            'L' => value.to_lowercase(),
            _ => value.to_string(),
        }
    }

    // THREAT[TM-DOS-089]: Box::pin the expand_word future to cap per-level
    // stack usage. Without this, the async state machine of expand_word (which
    // contains all WordPart match arms) is inlined into the caller's future,
    // causing stack overflow at moderate command substitution depths.
    pub(super) fn expand_word<'a>(
        &'a mut self,
        word: &'a Word,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<String>> + Send + 'a>> {
        Box::pin(async move {
            let expanded = self.expand_word_inner(word).await?;
            let expanded = Self::strip_quote_markers(&expanded);
            // A `QuotedGlobWord` carries glob escapes for its quoted text;
            // contexts that do not glob (command names, redirect targets,
            // operands) want the text itself.
            if word.quoted && word.has_unquoted_glob {
                Ok(Self::glob_path_unescape(&expanded))
            } else {
                Ok(expanded)
            }
        })
    }

    /// Quote expansion output that came from a quoted segment of a mixed word.
    /// THREAT[TM-INF-022]: Quoted user-controlled values must stay literal; only
    /// unquoted suffix/prefix glob syntax in the source word may drive expansion.
    /// The one definition of which characters the quoting above escapes.
    ///
    /// THREAT[TM-DOS-115]: `expansion_appended_len` charges the execution budget
    /// for what `append_expansion_for_word` is about to append, so both must
    /// agree on this set. Keeping it in one place stops the byte charge from
    /// silently under-counting when a metacharacter is added.
    fn needs_glob_escape(ch: char) -> bool {
        matches!(
            ch,
            '\\' | '*' | '?' | '[' | ']' | '{' | '}' | ',' | '@' | '!' | '+' | '(' | ')' | '|'
        )
    }

    pub(super) fn quote_expansion_for_quoted_glob(value: &str) -> String {
        let mut quoted = String::with_capacity(value.len());
        for ch in value.chars() {
            if Self::needs_glob_escape(ch) {
                quoted.push('\\');
            }
            quoted.push(ch);
        }
        quoted
    }

    /// How many bytes `append_expansion_for_word` will append for `value`,
    /// counted before anything is allocated so the budget is charged first.
    fn expansion_appended_len(word: &Word, value: &str) -> usize {
        if word.quoted && word.has_unquoted_glob {
            value
                .chars()
                .map(|ch| ch.len_utf8() + usize::from(Self::needs_glob_escape(ch)))
                .sum()
        } else {
            value.len()
        }
    }

    pub(super) fn append_expansion_for_word(result: &mut String, word: &Word, value: &str) {
        if word.quoted && word.has_unquoted_glob {
            result.push_str(&Self::quote_expansion_for_quoted_glob(value));
        } else {
            result.push_str(value);
        }
    }

    /// Expand a word used as a pattern (`case` item, `[[ == ]]` operand).
    /// Quoted text must match literally, so a fully quoted word has every
    /// glob metacharacter escaped. A mixed word (`QuotedGlobWord`) already
    /// carries escapes for its quoted literals from the lexer and for its
    /// quoted expansions from `append_expansion_for_word`.
    pub(super) async fn expand_pattern_word(&mut self, word: &Word) -> Result<String> {
        let expanded = Box::pin(self.expand_word_inner(word)).await?;
        let expanded = Self::strip_quote_markers(&expanded);
        if word.quoted && !word.has_unquoted_glob {
            Ok(Self::quote_expansion_for_quoted_glob(&expanded))
        } else {
            Ok(expanded)
        }
    }

    pub(super) async fn expand_word_inner(&mut self, word: &Word) -> Result<String> {
        let mut result = String::new();
        // Keep command-substitution bytes charged until the complete word has
        // been consumed; sibling substitutions otherwise evade live-byte caps.
        let mut substitution_leases = Vec::new();
        let mut is_first_part = true;

        for part in &word.parts {
            match part {
                WordPart::CompoundAssignment { .. } => {
                    // Only declaration builtins take `name=(...)`; elsewhere
                    // the text stands for itself.
                    let text = Word {
                        parts: vec![part.clone()],
                        quoted: false,
                        has_unquoted_glob: false,
                        part_quoted: Vec::new(),
                        raw: None,
                    }
                    .to_string();
                    result.push_str(&text);
                }
                WordPart::Literal(s) => {
                    // Tilde expansion: ~ at start of word expands to $HOME
                    // A fully quoted word (`"~"`, `'~'`) keeps its tilde.
                    let tilde_ok = !word.quoted
                        || (word.has_unquoted_glob && word.part_quoted.first() == Some(&false));
                    if is_first_part && tilde_ok && s.starts_with('~') {
                        // Tilde prefix runs to the first `/`: `~` is HOME,
                        // `~+` PWD, `~-` OLDPWD (literal when unset).
                        let (prefix, rest) = match s.find('/') {
                            Some(i) => (&s[1..i], &s[i..]),
                            None => (&s[1..], ""),
                        };
                        let lookup = |name: &str| {
                            self.scoped
                                .variables
                                .get(name)
                                .or_else(|| self.env.get(name))
                                .cloned()
                        };
                        let dir = match prefix {
                            "" => Some(
                                self.env
                                    .get("HOME")
                                    .or_else(|| self.scoped.variables.get("HOME"))
                                    .cloned()
                                    .unwrap_or_else(|| "/home/user".to_string()),
                            ),
                            "+" => Some(
                                lookup("PWD").unwrap_or_else(|| self.cwd.display().to_string()),
                            ),
                            "-" => lookup("OLDPWD"),
                            _ => None,
                        };
                        match dir {
                            Some(dir) => {
                                result.push_str(&dir);
                                result.push_str(rest);
                            }
                            None => result.push_str(s),
                        }
                    } else {
                        result.push_str(s);
                    }
                }
                WordPart::Variable(name) => {
                    if self.is_nounset() && !self.is_variable_set(name) {
                        self.nounset_error = Some(format!("bash: {}: unbound variable\n", name));
                    }
                    if name == "*" && word.quoted {
                        let positional = self
                            .call_stack
                            .last()
                            .map(|f| f.positional.clone())
                            .unwrap_or_default();
                        let sep = match self.scoped.variables.get("IFS") {
                            Some(ifs) => ifs
                                .chars()
                                .next()
                                .map(|c| c.to_string())
                                .unwrap_or_default(),
                            None => " ".to_string(),
                        };
                        Self::append_expansion_for_word(&mut result, word, &positional.join(&sep));
                    } else {
                        Self::append_expansion_for_word(
                            &mut result,
                            word,
                            &self.expand_variable(name),
                        );
                    }
                }
                WordPart::CommandSubstitution(commands) => {
                    // THREAT[TM-DOS-088]: Track substitution depth to prevent OOM.
                    if self.counters.push_subst(&self.limits).is_err() {
                        return Err(crate::error::Error::Execution(
                            "maximum command substitution depth exceeded".to_string(),
                        ));
                    }
                    // THREAT[TM-DOS-089]: Delegate to Box::pin-ed helper to
                    // prevent stack growth proportional to nesting depth.
                    let trimmed = self.execute_cmd_subst(commands).await?;
                    let appended_bytes = Self::expansion_appended_len(word, &trimmed);
                    substitution_leases.push(self.execution_budget.lease_bytes(appended_bytes)?);
                    Self::append_expansion_for_word(&mut result, word, &trimmed);
                }
                WordPart::ArithmeticExpansion(expr) => {
                    let expanded_expr = if expr.contains("$(") {
                        self.expand_command_subs_in_arithmetic(expr).await?
                    } else {
                        expr.to_string()
                    };
                    let value = self
                        .try_evaluate_arithmetic_with_assign(&expanded_expr)
                        .map_err(|msg| crate::error::Error::LineAbort(self.arith_diag("", &msg)))?;
                    Self::append_expansion_for_word(&mut result, word, &value.to_string());
                }
                WordPart::Length(name) => {
                    let value = if let Some(bracket_pos) = name.find('[') {
                        let arr_name = &name[..bracket_pos];
                        // Search for ']' after '[' to avoid panic when malformed
                        // input has ']' before '[' (e.g. null-byte-laden fuzz input).
                        let index_end = name[bracket_pos..]
                            .find(']')
                            .map(|i| bracket_pos + i)
                            .unwrap_or(name.len());
                        let start = (bracket_pos + 1).min(index_end);
                        let index_str = &name[start..index_end];
                        self.expand_array_access_part(arr_name, index_str)
                    } else {
                        self.expand_variable(name)
                    };
                    result.push_str(&self.shell_length(&value).to_string());
                }
                WordPart::ParameterExpansion {
                    name,
                    operator,
                    operand,
                    colon_variant,
                } => {
                    if name.is_empty()
                        && !matches!(
                            operator,
                            ParameterOp::UseDefault
                                | ParameterOp::AssignDefault
                                | ParameterOp::UseReplacement
                                | ParameterOp::Error
                        )
                    {
                        self.nounset_error = Some("bash: ${}: bad substitution\n".to_string());
                        continue;
                    }

                    let suppress_nounset = matches!(
                        operator,
                        ParameterOp::UseDefault
                            | ParameterOp::AssignDefault
                            | ParameterOp::UseReplacement
                            | ParameterOp::Error
                    );

                    let (is_set, value) = self.resolve_param_expansion_name(name);

                    if self.is_nounset() && !suppress_nounset && !is_set {
                        self.nounset_error = Some(format!("bash: {}: unbound variable\n", name));
                    }

                    if operand.contains("$(") {
                        // Boxed: keeps this frame small for deep `$(...)` nesting
                        // (TM-DOS-089).
                        Box::pin(self.prefetch_operand_substs(
                            operator,
                            operand,
                            *colon_variant,
                            is_set,
                            &value,
                        ))
                        .await?;
                    } else {
                        self.operand_substs.clear();
                    }
                    // Delegate to sync helper to avoid bloating the async state
                    // machine with Vec<String> locals (causes stack overflow at
                    // depth 32 in debug builds — see stack_overflow_regression_tests).
                    let expanded = self.apply_param_op_maybe_per_element(
                        &value,
                        name,
                        operator,
                        operand,
                        *colon_variant,
                        is_set,
                    );
                    Self::append_expansion_for_word(&mut result, word, &expanded);
                }
                WordPart::ArrayAccess { name, index } => {
                    Self::append_expansion_for_word(
                        &mut result,
                        word,
                        &self.expand_array_access_part(name, index),
                    );
                }
                WordPart::ArrayIndices(name) => {
                    let keys = self.array_keys(name);
                    Self::append_expansion_for_word(&mut result, word, &keys.join(" "));
                }
                WordPart::Substring {
                    name,
                    offset,
                    length,
                } if name == "@" || name == "*" => {
                    let items = self.positional_slice(offset, length.as_deref());
                    let sep = if name == "*" {
                        self.get_ifs_separator()
                    } else {
                        " ".to_string()
                    };
                    Self::append_expansion_for_word(&mut result, word, &items.join(&sep));
                }
                WordPart::Substring {
                    name,
                    offset,
                    length,
                } => {
                    let value = self.expand_variable(name);
                    let char_count = value.chars().count();
                    let offset_val: isize = self.evaluate_arithmetic(offset) as isize;
                    let start = if offset_val < 0 {
                        (char_count as isize + offset_val).max(0) as usize
                    } else {
                        (offset_val as usize).min(char_count)
                    };
                    let substr: String = if let Some(len_expr) = length {
                        let len_val = self.evaluate_arithmetic(len_expr) as usize;
                        value.chars().skip(start).take(len_val).collect()
                    } else {
                        value.chars().skip(start).collect()
                    };
                    Self::append_expansion_for_word(&mut result, word, &substr);
                }
                WordPart::ArraySlice {
                    name,
                    offset,
                    length,
                } => {
                    if let Some(items) = self.array_view(name) {
                        let values: Vec<String> = items.into_iter().map(|(_, v)| v).collect();

                        let offset_val: isize = self.evaluate_arithmetic(offset) as isize;
                        let start = if offset_val < 0 {
                            (values.len() as isize + offset_val).max(0) as usize
                        } else {
                            (offset_val as usize).min(values.len())
                        };

                        let sliced = if let Some(len_expr) = length {
                            let len_val = self.evaluate_arithmetic(len_expr) as usize;
                            let end = start.saturating_add(len_val).min(values.len());
                            &values[start..end]
                        } else {
                            &values[start..]
                        };
                        Self::append_expansion_for_word(&mut result, word, &sliced.join(" "));
                    }
                }
                WordPart::IndirectExpansion {
                    name,
                    operator,
                    operand,
                    colon_variant,
                } => {
                    let nameref_target = self.scoped.namerefs.get(name).cloned();
                    let is_nameref = nameref_target.is_some();

                    if is_nameref && operator.is_none() {
                        // Nameref without operator: ${!ref} is the name
                        // at the end of the nameref chain.
                        let target = self.resolve_nameref(name).to_string();
                        Self::append_expansion_for_word(&mut result, word, &target);
                    } else {
                        // Resolve the indirect target variable name
                        let resolved_name = if let Some(target) = nameref_target {
                            target
                        } else {
                            self.expand_variable(name)
                        };

                        if let Some(op) = operator {
                            // Indirect + operator: resolve indirect, then
                            // apply op to the target variable
                            let (is_set, value) = self.resolve_param_expansion_name(&resolved_name);
                            let expanded = self.apply_parameter_op(
                                &value,
                                &resolved_name,
                                op,
                                operand,
                                *colon_variant,
                                is_set,
                            );
                            Self::append_expansion_for_word(&mut result, word, &expanded);
                        } else {
                            // Plain indirect expansion (no operator)
                            if let Some(arr) = self.scoped.arrays.get(&resolved_name) {
                                if let Some(first) = arr.get(&0) {
                                    Self::append_expansion_for_word(&mut result, word, first);
                                }
                            } else {
                                let value = self.expand_variable(&resolved_name);
                                Self::append_expansion_for_word(&mut result, word, &value);
                            }
                        }
                    }
                }
                WordPart::PrefixMatch(prefix) => {
                    let mut names: Vec<String> = self
                        .scoped
                        .variables
                        .keys()
                        .filter(|k| k.starts_with(prefix.as_str()))
                        // THREAT[TM-INF-017]: Hide internal/hidden marker variables
                        .filter(|k| !Self::is_hidden_variable(k))
                        .cloned()
                        .collect();
                    for k in self.env.keys() {
                        if k.starts_with(prefix.as_str())
                            && !names.contains(k)
                            // THREAT[TM-INF-017]: Hide internal/hidden marker variables
                            && !Self::is_hidden_variable(k)
                        {
                            names.push(k.clone());
                        }
                    }
                    names.sort();
                    Self::append_expansion_for_word(&mut result, word, &names.join(" "));
                }
                WordPart::ArrayLength(name) => {
                    let resolved = self.resolve_nameref(name);
                    let len = if let Some(arr) = self.scoped.assoc_arrays.get(resolved) {
                        arr.len()
                    } else if let Some(arr) = self.scoped.arrays.get(resolved) {
                        arr.len()
                    } else {
                        usize::from(self.scalar_value(resolved).is_some())
                    };
                    result.push_str(&len.to_string());
                }
                WordPart::ProcessSubstitution { commands, is_input } => {
                    let expanded = self
                        .expand_process_substitution(commands, *is_input)
                        .await?;
                    Self::append_expansion_for_word(&mut result, word, &expanded);
                }
                WordPart::Transformation { name, operator } => {
                    Self::append_expansion_for_word(
                        &mut result,
                        word,
                        &self.apply_transformation(name, *operator),
                    );
                }
            }
            is_first_part = false;
        }

        Ok(result)
    }

    /// Expand a word to multiple fields (for array iteration and command args)
    /// Returns Vec<String> where array expansions like "${arr[@]}" produce multiple fields.
    /// "${arr[*]}" in quoted context joins elements into a single field (bash behavior).
    /// Boxed because nested command substitution repeatedly enters this helper through
    /// `expand_command_args`, and its special-parameter/array handling still inflated
    /// the recursive poll path enough to trip smaller stacks.
    pub(super) fn expand_word_to_fields<'a>(
        &'a mut self,
        word: &'a Word,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<Vec<String>>> + Send + 'a>> {
        Box::pin(async move {
            // Check if the word contains only an array expansion or $@/$*
            if word.parts.len() == 1 {
                // Handle $@ and $* as special parameters
                if let WordPart::Variable(name) = &word.parts[0] {
                    if name == "@" {
                        let positional = self
                            .call_stack
                            .last()
                            .map(|f| f.positional.clone())
                            .unwrap_or_default();
                        if word.quoted {
                            // "$@" preserves individual positional params
                            return Ok(positional);
                        }
                        // $@ unquoted: each param is subject to further IFS splitting
                        let mut fields = Vec::new();
                        for p in &positional {
                            fields.extend(self.ifs_split(p)?);
                        }
                        return Ok(fields);
                    }
                    if name == "*" {
                        let positional = self
                            .call_stack
                            .last()
                            .map(|f| f.positional.clone())
                            .unwrap_or_default();
                        if word.quoted {
                            // "$*" joins with first char of IFS.
                            // IFS unset → space; IFS="" → no separator.
                            let sep = match self.scoped.variables.get("IFS") {
                                Some(ifs) => ifs
                                    .chars()
                                    .next()
                                    .map(|c| c.to_string())
                                    .unwrap_or_default(),
                                None => " ".to_string(),
                            };
                            return Ok(vec![positional.join(&sep)]);
                        }
                        // $* unquoted: each param is subject to IFS splitting
                        let mut fields = Vec::new();
                        for p in &positional {
                            fields.extend(self.ifs_split(p)?);
                        }
                        return Ok(fields);
                    }
                }
                if let WordPart::ArrayAccess { name, index } = &word.parts[0]
                    && (index == "@" || index == "*")
                {
                    let Some(items) = self.array_view(name) else {
                        return Ok(Vec::new());
                    };
                    let values: Vec<String> = items.into_iter().map(|(_, v)| v).collect();
                    // "${arr[*]}" joins into single field with IFS; "${arr[@]}" keeps separate
                    if word.quoted && index == "*" {
                        let sep = self.get_ifs_separator();
                        return Ok(vec![values.join(&sep)]);
                    }
                    return Ok(values);
                }
                // "${a[@]/x/y}", "${@^}", "${a[@]@Q}": one field per element.
                if let Some((elems, star)) = self.elementwise_fields(&word.parts[0]) {
                    if word.quoted {
                        if star {
                            return Ok(vec![elems.join(&self.get_ifs_separator())]);
                        }
                        return Ok(elems);
                    }
                    let mut fields = Vec::new();
                    for e in &elems {
                        fields.extend(self.ifs_split(e)?);
                    }
                    return Ok(fields);
                }
                // "${!arr[@]}" - array keys/indices as separate fields
                if let WordPart::ArrayIndices(name) = &word.parts[0] {
                    return Ok(self.array_keys(name));
                }
            }

            let has_mixed_part_quotes =
                word.part_quoted.iter().any(|q| *q) && word.part_quoted.iter().any(|q| !*q);
            if has_mixed_part_quotes {
                let mut segments = Vec::new();
                let mut sentinel_haystack = self
                    .scoped
                    .variables
                    .get("IFS")
                    .cloned()
                    .unwrap_or_default();
                for (idx, part) in word.parts.iter().enumerate() {
                    let part_is_quoted = word.part_quoted.get(idx).copied().unwrap_or(word.quoted);
                    let part_has_expansion = Self::is_field_split_expansion(part);
                    let value = if idx > 0
                        && let WordPart::Literal(s) = part
                    {
                        s.clone()
                    } else {
                        let single = Word {
                            parts: vec![part.clone()],
                            quoted: part_is_quoted,
                            has_unquoted_glob: false,
                            part_quoted: vec![part_is_quoted],
                            raw: None,
                        };
                        self.expand_word(&single).await?
                    };

                    if part_has_expansion && !part_is_quoted {
                        sentinel_haystack.push_str(&value);
                        segments.push((value, false, false));
                    } else {
                        let value =
                            if part_is_quoted && part_has_expansion && word.has_unquoted_glob {
                                Self::quote_expansion_for_quoted_glob(&value)
                            } else {
                                value
                            };
                        let preserves_empty_field = part_is_quoted && value.is_empty();
                        sentinel_haystack.push_str(&value);
                        segments.push((value, true, preserves_empty_field));
                    }
                }

                // Pick a sentinel char absent from the data so empty quoted
                // fields survive splitting and can be stripped afterward. If no
                // candidate is free (astronomically unlikely), skip the sentinel
                // rather than fall back to NUL, which is a valid data byte.
                let empty_field_sentinel = segments
                    .iter()
                    .any(|(_, _, preserves_empty_field)| *preserves_empty_field)
                    .then(|| {
                        OPERAND_QUOTE_MARK_CANDIDATES
                            .iter()
                            .copied()
                            .find(|candidate| !sentinel_haystack.contains(*candidate))
                    })
                    .flatten();

                let mut expanded_for_split = String::new();
                for (value, is_protected, preserves_empty_field) in segments {
                    if is_protected {
                        // Field splitting scans the whole expanded word. Mark literal and
                        // quoted segments as protected so unquoted expansion delimiters can
                        // still create boundaries between adjacent protected segments.
                        expanded_for_split.push(QUOTED_SEGMENT_START);
                        if preserves_empty_field
                            && let Some(empty_field_sentinel) = empty_field_sentinel
                        {
                            expanded_for_split.push(empty_field_sentinel);
                        }
                        expanded_for_split.push_str(&value);
                        expanded_for_split.push(QUOTED_SEGMENT_END);
                    } else {
                        expanded_for_split.push_str(&value);
                    }
                }
                let mut fields = self.ifs_split(&expanded_for_split)?;
                if let Some(empty_field_sentinel) = empty_field_sentinel {
                    for field in &mut fields {
                        field.retain(|ch| ch != empty_field_sentinel);
                    }
                }
                return Ok(fields);
            }

            // For other words, expand to a single field then apply IFS word splitting
            // when the word is unquoted and contains an expansion.
            // Per POSIX, unquoted variable/command/arithmetic expansion results undergo
            // field splitting on IFS.
            let expanded = self.expand_word_inner(word).await?;

            // IFS splitting applies to unquoted expansions only.
            // Skip splitting for assignment-like words (e.g., result="$1") where
            // the lexer stripped quotes from a mixed-quoted word (produces Token::Word
            // with quoted: false even though the expansion was inside double quotes).
            let is_assignment_word =
                matches!(word.parts.first(), Some(WordPart::Literal(s)) if s.contains('='));
            let has_expansion = !word.quoted
                && !is_assignment_word
                && word.parts.iter().any(Self::is_field_split_expansion);

            if has_expansion {
                self.ifs_split(&expanded)
            } else {
                Ok(vec![Self::strip_quote_markers(&expanded)])
            }
        })
    }

    /// Expansion parts whose unquoted result undergoes IFS field splitting.
    fn is_field_split_expansion(part: &WordPart) -> bool {
        matches!(
            part,
            WordPart::Variable(_)
                | WordPart::CommandSubstitution(_)
                | WordPart::ArithmeticExpansion(_)
                | WordPart::ParameterExpansion { .. }
                | WordPart::ArrayAccess { .. }
                | WordPart::IndirectExpansion { .. }
                | WordPart::PrefixMatch(_)
                | WordPart::Substring { .. }
                | WordPart::ArraySlice { .. }
                | WordPart::Transformation { .. }
        )
    }

    /// Resolve name for parameter expansion, handling array subscripts and special params.
    /// Returns (is_set, expanded_value).
    pub(super) fn resolve_param_expansion_name(&self, name: &str) -> (bool, String) {
        // Check for array subscript pattern: name[@] or name[*]
        let is_star = name.ends_with("[*]");
        if let Some(arr_name) = name
            .strip_suffix("[@]")
            .or_else(|| name.strip_suffix("[*]"))
        {
            // Resolve nameref: if arr_name is a nameref, follow it to the target
            let resolved_arr_name = self.resolve_nameref(arr_name);
            let sep = if is_star {
                self.get_ifs_separator()
            } else {
                " ".to_string()
            };
            let values = self.array_values(resolved_arr_name);
            return (!values.is_empty(), values.join(&sep));
        }

        // Check for array element subscript: name[key]
        if let Some(bracket) = name.find('[')
            && name.ends_with(']')
        {
            let arr_name = &name[..bracket];
            // Resolve nameref: if arr_name is a nameref, follow it to the target
            let resolved_arr_name = self.resolve_nameref(arr_name);
            let key = &name[bracket + 1..name.len() - 1];
            if let Some(arr) = self.scoped.assoc_arrays.get(resolved_arr_name) {
                let expanded_key = self.expand_variable_or_literal(key);
                return match arr.get(&expanded_key) {
                    Some(v) => (true, v.clone()),
                    None => (false, String::new()),
                };
            }
            if let Some(arr) = self.scoped.arrays.get(resolved_arr_name) {
                let idx = self.resolve_indexed_array_subscript(resolved_arr_name, key);
                return match arr.get(&idx) {
                    Some(v) => (true, v.clone()),
                    None => (false, String::new()),
                };
            }
            return match self.scalar_element(resolved_arr_name, key) {
                Some(v) => (true, v),
                None => (false, String::new()),
            };
        }

        // Special parameters @ and *
        if name == "@" || name == "*" {
            if let Some(frame) = self.call_stack.last() {
                let is_set = !frame.positional.is_empty();
                let sep = if name == "*" {
                    self.get_ifs_separator()
                } else {
                    " ".to_string()
                };
                return (is_set, frame.positional.join(&sep));
            }
            return (false, String::new());
        }

        // Regular variable
        let is_set = self.is_variable_set(name);
        let value = self.expand_variable(name);
        (is_set, value)
    }

    /// Return individual elements for multi-element parameter names ($@, $*, arr[@], arr[*]).
    /// Returns None for scalar variables.
    pub(super) fn resolve_param_expansion_elements(&self, name: &str) -> Option<Vec<String>> {
        if name == "@" || name == "*" {
            if let Some(frame) = self.call_stack.last() {
                return Some(frame.positional.clone());
            }
            return Some(Vec::new());
        }
        if let Some(arr_name) = name
            .strip_suffix("[@]")
            .or_else(|| name.strip_suffix("[*]"))
        {
            return Some(self.array_values(arr_name));
        }
        None
    }

    pub(super) fn strip_quote_markers(s: &str) -> String {
        s.chars()
            .filter(|&c| c != QUOTED_SEGMENT_START && c != QUOTED_SEGMENT_END)
            .collect()
    }

    pub(super) fn quote_marker_chars(s: &str) -> Vec<(char, bool)> {
        let mut quoted = false;
        let mut chars = Vec::new();
        for c in s.chars() {
            match c {
                QUOTED_SEGMENT_START => quoted = true,
                QUOTED_SEGMENT_END => quoted = false,
                _ => chars.push((c, quoted)),
            }
        }
        chars
    }

    /// Split a string on IFS characters according to POSIX rules.
    ///
    /// - IFS whitespace (space, tab, newline) collapses; leading/trailing stripped.
    /// - IFS non-whitespace chars are significant delimiters. Two adjacent produce
    ///   an empty field between them.
    /// - `<ws><nws><ws>` = single delimiter (ws absorbed into the nws delimiter).
    /// - Empty IFS → no splitting. Unset IFS → default " \t\n".
    pub(super) fn ifs_split(&self, s: &str) -> Result<Vec<String>> {
        self.ifs_split_limited(s, self.limits.max_word_split_fields)
    }

    /// Split a string on IFS characters, returning an error if resource caps are exceeded.
    pub(super) fn ifs_split_limited(&self, s: &str, limit: usize) -> Result<Vec<String>> {
        // Clamp so callers passing a larger value (e.g. remaining array capacity)
        // cannot bypass the configured max_word_split_fields cap.
        let limit = limit.min(self.limits.max_word_split_fields);
        if limit == 0 {
            return Ok(Vec::new());
        }

        let ifs = self
            .scoped
            .variables
            .get("IFS")
            .cloned()
            .unwrap_or_else(|| " \t\n".to_string());

        if ifs.is_empty() {
            let field = Self::strip_quote_markers(s);
            let bytes = field.len();
            return self.push_ifs_field(Vec::new(), field, limit, bytes);
        }

        let is_ifs = |c: char, quoted: bool| !quoted && ifs.contains(c);
        let is_ifs_ws = |c: char, quoted: bool| !quoted && ifs.contains(c) && " \t\n".contains(c);
        let is_ifs_nws = |c: char, quoted: bool| !quoted && ifs.contains(c) && !" \t\n".contains(c);
        let all_whitespace_ifs = ifs.chars().all(|c| " \t\n".contains(c));
        let chars = Self::quote_marker_chars(s);

        if all_whitespace_ifs {
            // IFS is only whitespace: split on unquoted runs, elide empties.
            let mut fields = Vec::new();
            let mut current = String::new();
            let mut bytes = 0usize;
            for &(c, quoted) in &chars {
                if is_ifs(c, quoted) {
                    if !current.is_empty() {
                        bytes = bytes.saturating_add(current.len());
                        fields = self.push_ifs_field(
                            fields,
                            std::mem::take(&mut current),
                            limit,
                            bytes,
                        )?;
                    }
                } else {
                    current.push(c);
                }
            }
            if !current.is_empty() {
                bytes = bytes.saturating_add(current.len());
                fields = self.push_ifs_field(fields, current, limit, bytes)?;
            }
            return Ok(fields);
        }

        // Mixed or pure non-whitespace IFS.
        let mut fields: Vec<String> = Vec::new();
        let mut current = String::new();
        let mut bytes = 0usize;
        let mut i = 0;

        // Skip leading IFS whitespace
        while i < chars.len() && is_ifs_ws(chars[i].0, chars[i].1) {
            i += 1;
        }
        // Leading non-whitespace IFS produces an empty first field
        if i < chars.len() && is_ifs_nws(chars[i].0, chars[i].1) {
            fields = self.push_ifs_field(fields, String::new(), limit, bytes)?;
            i += 1;
            while i < chars.len() && is_ifs_ws(chars[i].0, chars[i].1) {
                i += 1;
            }
        }

        while i < chars.len() {
            let (c, quoted) = chars[i];
            if is_ifs_nws(c, quoted) {
                // Non-whitespace IFS delimiter: finalize current field
                let field = std::mem::take(&mut current);
                bytes = bytes.saturating_add(field.len());
                fields = self.push_ifs_field(fields, field, limit, bytes)?;
                i += 1;
                // Consume trailing IFS whitespace
                while i < chars.len() && is_ifs_ws(chars[i].0, chars[i].1) {
                    i += 1;
                }
            } else if is_ifs_ws(c, quoted) {
                // IFS whitespace: skip it, then check for non-ws delimiter
                while i < chars.len() && is_ifs_ws(chars[i].0, chars[i].1) {
                    i += 1;
                }
                if i < chars.len() && is_ifs_nws(chars[i].0, chars[i].1) {
                    // <ws><nws> = single delimiter. Push current field.
                    let field = std::mem::take(&mut current);
                    bytes = bytes.saturating_add(field.len());
                    fields = self.push_ifs_field(fields, field, limit, bytes)?;
                    i += 1; // consume the nws char
                    while i < chars.len() && is_ifs_ws(chars[i].0, chars[i].1) {
                        i += 1;
                    }
                } else if i < chars.len() {
                    // ws alone as delimiter (no nws follows)
                    let field = std::mem::take(&mut current);
                    bytes = bytes.saturating_add(field.len());
                    fields = self.push_ifs_field(fields, field, limit, bytes)?;
                }
                // trailing ws at end → ignore (don't push empty field)
            } else {
                current.push(c);
                i += 1;
            }
        }

        if !current.is_empty() {
            bytes = bytes.saturating_add(current.len());
            fields = self.push_ifs_field(fields, current, limit, bytes)?;
        }

        Ok(fields)
    }

    pub(super) fn push_ifs_field(
        &self,
        mut fields: Vec<String>,
        field: String,
        limit: usize,
        bytes: usize,
    ) -> Result<Vec<String>> {
        if fields.len() >= limit {
            return Err(crate::limits::LimitExceeded::Memory(format!(
                "word split field limit ({limit}) exceeded"
            ))
            .into());
        }
        if bytes > self.limits.max_word_split_bytes {
            return Err(crate::limits::LimitExceeded::Memory(format!(
                "word split byte limit ({}) exceeded",
                self.limits.max_word_split_bytes
            ))
            .into());
        }
        fields.push(field);
        Ok(fields)
    }

    /// Expand an operand string from a parameter expansion (sync, lazy).
    /// Only called when the operand is actually needed, providing lazy evaluation.
    /// `${x:-$(cmd)}`: run the operand's command substitutions before the
    /// sync operand expansion, only when the operator uses the operand (so
    /// an unused default never runs). Outputs queue in `operand_substs`.
    async fn prefetch_operand_substs(
        &mut self,
        operator: &ParameterOp,
        operand: &str,
        colon_variant: bool,
        is_set: bool,
        value: &str,
    ) -> Result<()> {
        self.operand_substs.clear();
        if !operand.contains("$(") {
            return Ok(());
        }
        let unset_or_null = !is_set || (colon_variant && value.is_empty());
        let uses_operand = match operator {
            ParameterOp::UseDefault | ParameterOp::AssignDefault | ParameterOp::Error => {
                unset_or_null
            }
            ParameterOp::UseReplacement => !unset_or_null,
            _ => false,
        };
        if !uses_operand {
            return Ok(());
        }
        let (word, _, _) = Self::parse_marked_operand(
            operand,
            self.limits.max_ast_depth,
            self.limits.max_parser_operations,
        );
        for part in &word.parts {
            if let WordPart::CommandSubstitution(commands) = part {
                // THREAT[TM-DOS-088]: same depth accounting as word `$(...)`.
                if self.counters.push_subst(&self.limits).is_err() {
                    return Err(crate::error::Error::Execution(
                        "maximum command substitution depth exceeded".to_string(),
                    ));
                }
                let out = self.execute_cmd_subst(commands).await?;
                self.operand_substs.push_back(out);
            }
        }
        Ok(())
    }

    pub(super) fn expand_operand(&mut self, operand: &str) -> String {
        if operand.is_empty() {
            return String::new();
        }
        // Strip quotes from operand before parsing.
        // For pattern-removal operators, quoted glob chars must stay literal.
        // Track stripped double-quoted spans with a marker that cannot be
        // mistaken for a parsed top-level literal, then consume that marker
        // only from parsed literal parts. Expanded variable data is handled
        // out-of-band so attacker data cannot inject quote-state toggles.
        let (word, quote_mark, force_quoted) = Self::parse_marked_operand(
            operand,
            self.limits.max_ast_depth,
            self.limits.max_parser_operations,
        );
        let mut result = String::new();
        let mut in_marked = false;
        for part in &word.parts {
            match part {
                WordPart::Literal(s) => {
                    Self::push_marked_literal(
                        &mut result,
                        s,
                        quote_mark,
                        &mut in_marked,
                        force_quoted,
                    );
                }
                WordPart::Variable(name) => {
                    let expanded = self.expand_variable(name);
                    Self::push_operand_expansion(&mut result, &expanded, in_marked || force_quoted);
                }
                WordPart::ArithmeticExpansion(expr) => {
                    let val = self.evaluate_arithmetic_with_assign(expr).to_string();
                    Self::push_operand_expansion(&mut result, &val, in_marked || force_quoted);
                }
                WordPart::ParameterExpansion {
                    name,
                    operator,
                    operand: inner_operand,
                    colon_variant,
                } => {
                    let (is_set, value) = self.resolve_param_expansion_name(name);
                    let expanded = self.apply_parameter_op(
                        &value,
                        name,
                        operator,
                        inner_operand,
                        *colon_variant,
                        is_set,
                    );
                    Self::push_operand_expansion(&mut result, &expanded, in_marked || force_quoted);
                }
                WordPart::Length(name) => {
                    let value = self.shell_length(&self.expand_variable(name)).to_string();
                    Self::push_operand_expansion(&mut result, &value, in_marked || force_quoted);
                }
                // Run ahead by `prefetch_operand_substs` (default-family
                // operators only); other operators leave the queue empty.
                WordPart::CommandSubstitution(_) => {
                    if let Some(out) = self.operand_substs.pop_front() {
                        Self::push_operand_expansion(&mut result, &out, in_marked || force_quoted);
                    }
                }
                // TODO: process substitution in sync operand expansion
                _ => {}
            }
        }
        result
    }

    /// Strip unescaped double-quote pairs from operand strings.
    /// In patterns like `${var#./"$other"}`, the `"` around `$other` suppress
    /// globbing but should not appear as literal characters in the pattern.
    /// Escaped quotes (`\"`) and NUL-sentinel-marked chars (`\x00"`) are kept.
    pub(super) fn strip_operand_quotes(operand: &str, quote_mark: Option<char>) -> String {
        Self::strip_operand_quotes_with_count(operand, quote_mark).0
    }

    /// Returns the stripped operand and the number of *unescaped* double quotes
    /// removed. When `quote_mark` is `Some`, each such quote is replaced by the
    /// marker (so the count equals the inserted-mark count); when `None`, the
    /// quote is dropped but still counted so callers can tell whether any real
    /// quote boundaries existed (escaped `\"` and NUL-sentinel quotes excluded).
    pub(super) fn strip_operand_quotes_with_count(
        operand: &str,
        quote_mark: Option<char>,
    ) -> (String, usize) {
        let mut result = String::with_capacity(operand.len());
        let chars: Vec<char> = operand.chars().collect();
        let mut unescaped_quotes = 0;
        let mut i = 0;
        while i < chars.len() {
            if chars[i] == '\x00' && i + 1 < chars.len() {
                // NUL sentinel: next char is literal (from lexer escape processing)
                result.push(chars[i]);
                result.push(chars[i + 1]);
                i += 2;
            } else if chars[i] == '\\' && i + 1 < chars.len() && chars[i + 1] == '"' {
                // Escaped double quote \" → literal " (keep both for parse_word)
                result.push(chars[i]);
                result.push(chars[i + 1]);
                i += 2;
            } else if chars[i] == '"' {
                // Unescaped double quote: skip it (strip the quote character).
                unescaped_quotes += 1;
                if let Some(quote_mark) = quote_mark {
                    result.push(quote_mark);
                }
                i += 1;
            } else {
                result.push(chars[i]);
                i += 1;
            }
        }
        (result, unescaped_quotes)
    }

    pub(super) fn operand_quote_mark(operand: &str) -> Option<char> {
        OPERAND_QUOTE_MARK_CANDIDATES
            .iter()
            .copied()
            .find(|&ch| !operand.contains(ch))
    }

    /// Parse an operand while tracking stripped quote boundaries.
    ///
    /// Returns the parsed `Word`, the marker char chosen to flag quote
    /// boundaries (when one is safe), and `force_quoted`: set only on the
    /// fail-closed path where no safe marker exists but the operand really did
    /// contain unescaped quotes, so expansions must be treated as quoted.
    pub(super) fn parse_marked_operand(
        operand: &str,
        max_depth: usize,
        max_fuel: usize,
    ) -> (Word, Option<char>, bool) {
        // Fast path: no double quotes means there is no quote-state to preserve,
        // so parse once and skip the bounded candidate search entirely. This
        // also avoids attacker-amplified repeated parsing of quote-free operands.
        if !operand.contains('"') {
            let stripped = Self::strip_operand_quotes(operand, None);
            return (
                Parser::parse_word_string_with_limits(&stripped, max_depth, max_fuel),
                None,
                false,
            );
        }

        if let Some(quote_mark) = Self::operand_quote_mark(operand) {
            let stripped = Self::strip_operand_quotes(operand, Some(quote_mark));
            return (
                Parser::parse_word_string_with_limits(&stripped, max_depth, max_fuel),
                Some(quote_mark),
                false,
            );
        }

        // Important decision: marker provenance is lost after parsing. If every
        // bounded candidate appears in the source operand, a source literal can
        // masquerade as an inserted quote-boundary marker. Fail closed instead
        // of reparsing with an unsafe marker.
        // Fail-closed: no safe marker. Only force quoted handling when real
        // unescaped quotes were stripped (not escaped `\"` or NUL-marked quotes).
        let (stripped, unescaped_quotes) = Self::strip_operand_quotes_with_count(operand, None);
        (
            Parser::parse_word_string_with_limits(&stripped, max_depth, max_fuel),
            None,
            unescaped_quotes > 0,
        )
    }

    pub(super) fn push_marked_literal(
        out: &mut String,
        s: &str,
        quote_mark: Option<char>,
        in_marked: &mut bool,
        force_quoted: bool,
    ) {
        for ch in s.chars() {
            if Some(ch) == quote_mark {
                *in_marked = !*in_marked;
                continue;
            }
            Self::push_operand_char(out, ch, *in_marked || force_quoted);
        }
    }

    pub(super) fn push_operand_expansion(out: &mut String, s: &str, in_marked: bool) {
        for ch in s.chars() {
            Self::push_operand_char(out, ch, in_marked);
        }
    }

    pub(super) fn push_operand_char(out: &mut String, ch: char, in_marked: bool) {
        if in_marked
            && matches!(
                ch,
                '*' | '?' | '[' | ']' | '(' | ')' | '|' | '+' | '@' | '!'
            )
        {
            out.push('\\');
        }
        out.push(ch);
    }

    pub(super) fn find_unescaped_char(pattern: &str, target: char) -> Option<usize> {
        let mut escaped = false;
        for (idx, ch) in pattern.char_indices() {
            if escaped {
                escaped = false;
                continue;
            }
            if ch == '\\' {
                escaped = true;
                continue;
            }
            if ch == target {
                return Some(idx);
            }
        }
        None
    }

    pub(super) fn has_unescaped_char(pattern: &str, target: char) -> bool {
        Self::find_unescaped_char(pattern, target).is_some()
    }

    pub(super) fn contains_unescaped_extglob(&self, pattern: &str) -> bool {
        for op in ["@(", "*(", "?(", "+(", "!("] {
            if let Some(pos) = pattern.find(op)
                && !pattern[..pos].ends_with('\\')
            {
                return true;
            }
        }
        false
    }

    pub(super) fn unescape_pattern_literal(pattern: &str) -> String {
        let mut out = String::with_capacity(pattern.len());
        let mut escaped = false;
        for ch in pattern.chars() {
            if escaped {
                out.push(ch);
                escaped = false;
            } else if ch == '\\' {
                escaped = true;
            } else {
                out.push(ch);
            }
        }
        if escaped {
            out.push('\\');
        }
        out
    }

    /// Apply a parameter operator, handling per-element expansion for $@/$*/arr[@].
    ///
    /// Extracted from the async `expand_word_inner` path to keep `Vec<String>`
    /// locals off the async state machine (prevents stack overflow at depth 32).
    /// Per-element results of a pattern, case or transform operator applied
    /// to `@`, `*`, `a[@]` or `a[*]`, plus whether the subscript was `*`.
    /// Sync so the async field expansion keeps its small state machine.
    fn elementwise_fields(&mut self, part: &WordPart) -> Option<(Vec<String>, bool)> {
        let (name, results) = match part {
            WordPart::ParameterExpansion {
                name,
                operator,
                operand,
                colon_variant,
            } if Self::is_elementwise_op(operator) => {
                let elems = self.resolve_param_expansion_elements(name)?;
                let (is_set, _) = self.resolve_param_expansion_name(name);
                let mut out = Vec::with_capacity(elems.len());
                let mut total = 0usize;
                for elem in &elems {
                    let r = self.apply_parameter_op(
                        elem,
                        name,
                        operator,
                        operand,
                        *colon_variant,
                        is_set,
                    );
                    // THREAT[TM-DOS]: same cap as the joined per-element path.
                    total = total.saturating_add(r.len() + 1);
                    if total > Self::MAX_EXPANSION_RESULT_BYTES {
                        return None;
                    }
                    out.push(r);
                }
                (name, out)
            }
            WordPart::Transformation { name, operator }
                if matches!(operator, 'Q' | 'E' | 'P' | 'u' | 'U' | 'L') =>
            {
                let elems = self.resolve_param_expansion_elements(name)?;
                let mut out = Vec::with_capacity(elems.len());
                let mut total = 0usize;
                for elem in &elems {
                    let r = Self::transform_value(elem, *operator);
                    total = total.saturating_add(r.len() + 1);
                    if total > Self::MAX_EXPANSION_RESULT_BYTES {
                        return None;
                    }
                    out.push(r);
                }
                (name, out)
            }
            WordPart::Substring {
                name,
                offset,
                length,
            } if name == "@" || name == "*" => {
                let items = self.positional_slice(offset, length.as_deref());
                (name, items)
            }
            _ => return None,
        };
        Some((results, name == "*" || name.ends_with("[*]")))
    }

    /// `${@:offset:length}`: positional parameters counted from `$0`
    /// (offset 0 includes it); a negative offset counts back from the last.
    fn positional_slice(&mut self, offset: &str, length: Option<&str>) -> Vec<String> {
        let positional = self
            .call_stack
            .last()
            .map(|f| f.positional.clone())
            .unwrap_or_default();
        let mut all = Vec::with_capacity(positional.len() + 1);
        all.push(self.expand_variable("0"));
        all.extend(positional);
        let off = self.evaluate_arithmetic(offset);
        let start = if off < 0 {
            let back = usize::try_from(off.unsigned_abs()).unwrap_or(usize::MAX);
            match all.len().checked_sub(back) {
                Some(s) => s,
                None => return Vec::new(),
            }
        } else {
            usize::try_from(off).unwrap_or(usize::MAX).min(all.len())
        };
        let mut items: Vec<String> = all.into_iter().skip(start).collect();
        if let Some(len) = length {
            let n = self.evaluate_arithmetic(len);
            items.truncate(usize::try_from(n.max(0)).unwrap_or(usize::MAX));
        }
        items
    }

    fn is_elementwise_op(operator: &ParameterOp) -> bool {
        matches!(
            operator,
            ParameterOp::RemovePrefixShort
                | ParameterOp::RemovePrefixLong
                | ParameterOp::RemoveSuffixShort
                | ParameterOp::RemoveSuffixLong
                | ParameterOp::ReplaceFirst { .. }
                | ParameterOp::ReplaceAll { .. }
                | ParameterOp::UpperFirst
                | ParameterOp::UpperAll
                | ParameterOp::LowerFirst
                | ParameterOp::LowerAll
                | ParameterOp::ToggleFirst
                | ParameterOp::ToggleAll
        )
    }

    pub(super) fn apply_param_op_maybe_per_element(
        &mut self,
        value: &str,
        name: &str,
        operator: &ParameterOp,
        operand: &str,
        colon_variant: bool,
        is_set: bool,
    ) -> String {
        let needs_per_element = Self::is_elementwise_op(operator);
        if needs_per_element && let Some(elems) = self.resolve_param_expansion_elements(name) {
            let mut result = String::new();
            for elem in &elems {
                let expanded =
                    self.apply_parameter_op(elem, name, operator, operand, colon_variant, is_set);
                let next_len = result
                    .len()
                    .checked_add(usize::from(!result.is_empty()))
                    .and_then(|len| len.checked_add(expanded.len()));
                let Some(next_len) = next_len else {
                    return value.to_string();
                };
                if next_len > Self::MAX_EXPANSION_RESULT_BYTES {
                    return value.to_string();
                }
                if !result.is_empty() {
                    result.push(' ');
                }
                result.push_str(&expanded);
            }
            return result;
        }
        self.apply_parameter_op(value, name, operator, operand, colon_variant, is_set)
    }

    /// Apply parameter expansion operator.
    /// `colon_variant`: true = check unset-or-empty, false = check unset-only.
    /// `is_set`: whether the variable is defined (distinct from being empty).
    pub(super) fn apply_parameter_op(
        &mut self,
        value: &str,
        name: &str,
        operator: &ParameterOp,
        operand: &str,
        colon_variant: bool,
        is_set: bool,
    ) -> String {
        // colon (:-) => trigger when unset OR empty
        // no-colon (-) => trigger only when unset
        let use_default = if colon_variant {
            !is_set || value.is_empty()
        } else {
            !is_set
        };
        let use_replacement = if colon_variant {
            is_set && !value.is_empty()
        } else {
            is_set
        };

        match operator {
            ParameterOp::UseDefault => {
                if use_default {
                    self.expand_operand(operand)
                } else {
                    value.to_string()
                }
            }
            ParameterOp::AssignDefault => {
                if use_default {
                    let expanded = self.expand_operand(operand);
                    self.set_parameter_expansion_target(name, expanded.clone());
                    expanded
                } else {
                    value.to_string()
                }
            }
            ParameterOp::UseReplacement => {
                if use_replacement {
                    self.expand_operand(operand)
                } else {
                    String::new()
                }
            }
            ParameterOp::Error => {
                if use_default {
                    let expanded = self.expand_operand(operand);
                    let msg = if expanded.is_empty() {
                        format!("bash: {}: parameter null or not set\n", name)
                    } else {
                        format!("bash: {}: {}\n", name, expanded)
                    };
                    self.nounset_error = Some(msg);
                    String::new()
                } else {
                    value.to_string()
                }
            }
            ParameterOp::RemovePrefixShort => {
                // ${var#pattern} - remove shortest prefix match
                let expanded = self.expand_pattern_operand(operand);
                self.remove_pattern(value, &expanded, true, false)
            }
            ParameterOp::RemovePrefixLong => {
                // ${var##pattern} - remove longest prefix match
                let expanded = self.expand_pattern_operand(operand);
                self.remove_pattern(value, &expanded, true, true)
            }
            ParameterOp::RemoveSuffixShort => {
                // ${var%pattern} - remove shortest suffix match
                let expanded = self.expand_pattern_operand(operand);
                self.remove_pattern(value, &expanded, false, false)
            }
            ParameterOp::RemoveSuffixLong => {
                // ${var%%pattern} - remove longest suffix match
                let expanded = self.expand_pattern_operand(operand);
                self.remove_pattern(value, &expanded, false, true)
            }
            ParameterOp::ReplaceFirst {
                pattern,
                replacement,
            } => {
                // ${var/pattern/replacement} - replace first occurrence
                let expanded_rep = self.expand_replacement_operand(replacement);
                let expanded_pat = self.expand_replace_pattern(pattern);
                self.replace_pattern(value, &expanded_pat, &expanded_rep, false)
            }
            ParameterOp::ReplaceAll {
                pattern,
                replacement,
            } => {
                // ${var//pattern/replacement} - replace all occurrences
                let expanded_rep = self.expand_replacement_operand(replacement);
                let expanded_pat = self.expand_replace_pattern(pattern);
                self.replace_pattern(value, &expanded_pat, &expanded_rep, true)
            }
            ParameterOp::UpperFirst => self.change_case(value, operand, CaseChange::Upper, false),
            ParameterOp::UpperAll => self.change_case(value, operand, CaseChange::Upper, true),
            ParameterOp::LowerFirst => self.change_case(value, operand, CaseChange::Lower, false),
            ParameterOp::LowerAll => self.change_case(value, operand, CaseChange::Lower, true),
            ParameterOp::ToggleFirst => self.change_case(value, operand, CaseChange::Toggle, false),
            ParameterOp::ToggleAll => self.change_case(value, operand, CaseChange::Toggle, true),
        }
    }

    /// `${v^pat}`, `${v^^pat}`, `${v,pat}`, `${v,,pat}`: change the case of
    /// the first (or every) character that matches `pat` on its own. An empty
    /// pattern matches any character, as in bash.
    fn change_case(&mut self, value: &str, operand: &str, mode: CaseChange, all: bool) -> String {
        let pattern = if operand.is_empty() {
            String::new()
        } else {
            self.expand_pattern_operand(operand)
        };
        let mut out = String::with_capacity(value.len());
        let mut buf = [0u8; 4];
        for (i, ch) in value.chars().enumerate() {
            let selected = (all || i == 0)
                && (pattern.is_empty() || self.pattern_matches(ch.encode_utf8(&mut buf), &pattern));
            if !selected {
                out.push(ch);
                continue;
            }
            let upper = match mode {
                CaseChange::Upper => true,
                CaseChange::Lower => false,
                CaseChange::Toggle => !ch.is_uppercase(),
            };
            if upper {
                out.extend(ch.to_uppercase());
            } else {
                out.extend(ch.to_lowercase());
            }
        }
        out
    }

    /// Replace pattern in value
    /// THREAT[TM-DOS]: Maximum expansion result size (10MB) to prevent memory
    /// amplification in global pattern replacement.
    pub(crate) const MAX_EXPANSION_RESULT_BYTES: usize = 10 * 1024 * 1024;

    /// Expand a `#`/`%` pattern operand. Bash removes quotes there even in a
    /// double-quoted word, so `'...'` spans count as quoted (literal) text.
    pub(super) fn expand_pattern_operand(&mut self, operand: &str) -> String {
        self.expand_operand(&Self::single_quotes_as_quoted(operand))
    }

    /// Expand the replacement of `${x/pattern/rep}`. Like the pattern,
    /// bash quote-removes `'...'` there even inside double quotes
    /// (`"${y/b/'}'}"` is `a}`).
    pub(super) fn expand_replacement_operand(&mut self, operand: &str) -> String {
        self.expand_operand(&Self::single_quotes_as_quoted(operand))
    }

    /// Expand the pattern of `${x/pattern/rep}`. A leading `#`/`%` in the
    /// source is the anchor; a `#`/`%` that comes from expansion is literal.
    pub(super) fn expand_replace_pattern(&mut self, pattern: &str) -> String {
        let (anchor, raw) = match pattern.chars().next() {
            Some(c @ ('#' | '%')) => (Some(c), &pattern[1..]),
            _ => (None, pattern),
        };
        let expanded = self.expand_pattern_operand(raw);
        match anchor {
            Some(c) => format!("{c}{expanded}"),
            None if expanded.starts_with(['#', '%']) => format!("\\{expanded}"),
            None => expanded,
        }
    }

    /// Rewrite `'...'` spans outside `"..."` as `"..."` spans with every char
    /// NUL-escaped, so operand expansion treats them as quoted literals.
    pub(super) fn single_quotes_as_quoted(operand: &str) -> std::borrow::Cow<'_, str> {
        if !operand.contains('\'') {
            return std::borrow::Cow::Borrowed(operand);
        }
        let mut out = String::with_capacity(operand.len() + 8);
        let mut chars = operand.chars();
        let mut in_dq = false;
        while let Some(c) = chars.next() {
            match c {
                '\x00' | '\\' => {
                    out.push(c);
                    if let Some(n) = chars.next() {
                        out.push(n);
                    }
                }
                '"' => {
                    in_dq = !in_dq;
                    out.push(c);
                }
                '\'' if !in_dq && out.ends_with('$') => {
                    // `$'...'` is ANSI-C quoting; operand parsing decodes it.
                    out.push(c);
                    while let Some(q) = chars.next() {
                        out.push(q);
                        if q == '\\' {
                            if let Some(n) = chars.next() {
                                out.push(n);
                            }
                        } else if q == '\'' {
                            break;
                        }
                    }
                }
                '\'' if !in_dq => {
                    out.push('"');
                    while let Some(q) = chars.next() {
                        match q {
                            '\'' => break,
                            '\x00' => {
                                out.push(q);
                                if let Some(n) = chars.next() {
                                    out.push(n);
                                }
                            }
                            _ => {
                                out.push('\x00');
                                out.push(q);
                            }
                        }
                    }
                    out.push('"');
                }
                _ => out.push(c),
            }
        }
        std::borrow::Cow::Owned(out)
    }

    /// `${var/pattern/rep}` on an expanded pattern: a leading `#` or `%`
    /// anchors at the start or end, `\\c` is a literal `c`, `*`, `?` and `[...]`
    /// are globs. Matches are leftmost-longest, as in bash.
    pub(super) fn replace_pattern(
        &self,
        value: &str,
        pattern: &str,
        replacement: &str,
        global: bool,
    ) -> String {
        if pattern.is_empty() {
            return value.to_string();
        }
        let (anchor, pat) = if let Some(rest) = pattern.strip_prefix('#') {
            (PatternAnchor::Start, rest)
        } else if let Some(rest) = pattern.strip_prefix('%') {
            (PatternAnchor::End, rest)
        } else {
            (PatternAnchor::None, pattern)
        };

        let concat_or_original = |parts: &[&str]| {
            let mut total_len = 0usize;
            for part in parts {
                total_len = total_len.checked_add(part.len())?;
                if total_len > Self::MAX_EXPANSION_RESULT_BYTES {
                    return None;
                }
            }
            let mut result = String::with_capacity(total_len);
            for part in parts {
                result.push_str(part);
            }
            Some(result)
        };

        if pat.is_empty() {
            // ${var/#/rep} prepends, ${var/%/rep} appends.
            let joined = match anchor {
                PatternAnchor::Start => concat_or_original(&[replacement, value]),
                PatternAnchor::End => concat_or_original(&[value, replacement]),
                PatternAnchor::None => None,
            };
            return joined.unwrap_or_else(|| value.to_string());
        }

        let Some(ranges) = self.find_pattern_matches(value, pat, anchor, global) else {
            return value.to_string();
        };
        let mut out = String::new();
        let mut last = 0;
        for (start, end) in ranges {
            out.push_str(&value[last..start]);
            out.push_str(replacement);
            last = end;
            if out.len() > Self::MAX_EXPANSION_RESULT_BYTES {
                return value.to_string();
            }
        }
        out.push_str(&value[last..]);
        if out.len() > Self::MAX_EXPANSION_RESULT_BYTES {
            return value.to_string();
        }
        out
    }

    /// Byte ranges of the matches `replace_pattern` substitutes, or `None`
    /// when the search budget runs out (the value is then left unchanged).
    fn find_pattern_matches(
        &self,
        value: &str,
        pat: &str,
        anchor: PatternAnchor,
        global: bool,
    ) -> Option<Vec<(usize, usize)>> {
        let has_glob = Self::find_unescaped_char(pat, '*').is_some()
            || Self::find_unescaped_char(pat, '?').is_some()
            || Self::find_unescaped_char(pat, '[').is_some();
        let extglob = self.contains_unescaped_extglob(pat);

        if !has_glob && !extglob {
            let literal = Self::unescape_pattern_literal(pat);
            return Some(match anchor {
                PatternAnchor::Start if value.starts_with(&literal) => vec![(0, literal.len())],
                PatternAnchor::End if value.ends_with(&literal) => {
                    vec![(value.len() - literal.len(), value.len())]
                }
                PatternAnchor::Start | PatternAnchor::End => Vec::new(),
                PatternAnchor::None => {
                    let mut found = value.match_indices(literal.as_str());
                    if global {
                        found.map(|(i, m)| (i, i + m.len())).collect()
                    } else {
                        found
                            .next()
                            .map(|(i, m)| vec![(i, i + m.len())])
                            .unwrap_or_default()
                    }
                }
            });
        }

        if !extglob && let Some(re) = Self::glob_pattern_regex(pat, anchor) {
            // THREAT[TM-DOS-127]: the regex crate matches in linear time and
            // the compiled program is size-capped, so a long value or pattern
            // cannot blow up the search.
            let mut ranges = Vec::new();
            let mut pos = 0;
            while pos <= value.len() {
                let Some(m) = re.find_at(value, pos) else {
                    break;
                };
                if m.start() == m.end() && anchor == PatternAnchor::None && !value.is_empty() {
                    // A translated glob only matches empty at the end of the
                    // value, where bash does not substitute.
                    break;
                }
                ranges.push((m.start(), m.end()));
                if !global || anchor != PatternAnchor::None || m.start() == m.end() {
                    break;
                }
                pos = m.end();
            }
            return Some(ranges);
        }

        self.find_pattern_matches_glob(value, pat, anchor, global)
    }

    /// Extglob fallback, bash's own scan: at each position take the longest
    /// match (an empty one counts, then one char is copied); never match at
    /// the end of a non-empty value. THREAT[TM-DOS-127]: capped at
    /// `MAX_GLOB_MATCH_CALLS` attempts.
    fn find_pattern_matches_glob(
        &self,
        value: &str,
        pat: &str,
        anchor: PatternAnchor,
        global: bool,
    ) -> Option<Vec<(usize, usize)>> {
        const MAX_GLOB_MATCH_CALLS: usize = 10_000;
        let bounds: Vec<usize> = value
            .char_indices()
            .map(|(i, _)| i)
            .chain(std::iter::once(value.len()))
            .collect();
        let n = bounds.len() - 1;
        let mut calls = 0usize;
        let mut ranges = Vec::new();
        let mut s = 0;
        loop {
            if (s == n && n > 0) || s > n || (anchor == PatternAnchor::Start && s > 0) {
                break;
            }
            let mut found = None;
            let ends: Vec<usize> = if anchor == PatternAnchor::End {
                vec![n]
            } else {
                (s..=n).rev().collect()
            };
            for e in ends {
                calls += 1;
                if calls > MAX_GLOB_MATCH_CALLS {
                    return None;
                }
                if self.glob_match(&value[bounds[s]..bounds[e]], pat) {
                    found = Some(e);
                    break;
                }
            }
            match found {
                Some(e) => {
                    ranges.push((bounds[s], bounds[e]));
                    if !global || anchor != PatternAnchor::None {
                        break;
                    }
                    s = if e > s { e } else { s + 1 };
                }
                None => s += 1,
            }
        }
        Some(ranges)
    }

    /// Translate a bash glob (with `\\c` escapes) into an anchored-as-needed
    /// regex. `None` when it does not translate or compile; callers then use
    /// the glob fallback.
    fn glob_pattern_regex(pat: &str, anchor: PatternAnchor) -> Option<regex::Regex> {
        let chars: Vec<char> = pat.chars().collect();
        let mut re = String::from("(?s)");
        if anchor == PatternAnchor::Start {
            re.push_str("\\A");
        }
        re.push_str("(?:");
        let mut buf = [0u8; 4];
        let mut i = 0;
        while i < chars.len() {
            match chars[i] {
                '\\' if i + 1 < chars.len() => {
                    re.push_str(&regex::escape(chars[i + 1].encode_utf8(&mut buf)));
                    i += 2;
                }
                '*' => {
                    re.push_str(".*");
                    i += 1;
                }
                '?' => {
                    re.push('.');
                    i += 1;
                }
                '[' => match Self::glob_bracket_regex(&chars, i) {
                    Some((class, next)) => {
                        re.push_str(&class);
                        i = next;
                    }
                    None => {
                        re.push_str("\\[");
                        i += 1;
                    }
                },
                c => {
                    re.push_str(&regex::escape(c.encode_utf8(&mut buf)));
                    i += 1;
                }
            }
        }
        re.push(')');
        if anchor == PatternAnchor::End {
            re.push_str("\\z");
        }
        regex::RegexBuilder::new(&re)
            .size_limit(1 << 20)
            .dfa_size_limit(1 << 20)
            .build()
            .ok()
    }

    /// `[...]` starting at `start` as a regex class, plus the index after it.
    fn glob_bracket_regex(chars: &[char], start: usize) -> Option<(String, usize)> {
        let mut out = String::from("[");
        let mut i = start + 1;
        if matches!(chars.get(i), Some('!' | '^')) {
            out.push('^');
            i += 1;
        }
        let first = i;
        while i < chars.len() {
            let c = chars[i];
            if c == ']' && i > first {
                out.push(']');
                return Some((out, i + 1));
            }
            if c == '[' && chars.get(i + 1) == Some(&':') {
                let rest: String = chars[i + 2..].iter().collect();
                if let Some(end) = rest.find(":]") {
                    let name = &rest[..end];
                    if name.chars().all(|ch| ch.is_ascii_lowercase()) && !name.is_empty() {
                        out.push_str(&format!("[:{name}:]"));
                        i += 2 + name.chars().count() + 2;
                        continue;
                    }
                }
            }
            let c = if c == '\\' && i + 1 < chars.len() {
                i += 1;
                chars[i]
            } else {
                c
            };
            if c == '-' && i > first && chars.get(i + 1).is_some_and(|n| *n != ']') {
                out.push('-');
            } else {
                if matches!(c, '\\' | ']' | '[' | '^' | '-' | '&' | '~') {
                    out.push('\\');
                }
                out.push(c);
            }
            i += 1;
        }
        None
    }

    /// Remove prefix/suffix pattern from value
    pub(super) fn remove_pattern(
        &self,
        value: &str,
        pattern: &str,
        prefix: bool,
        longest: bool,
    ) -> String {
        // Simple pattern matching with * glob
        if pattern.is_empty() {
            return value.to_string();
        }

        // A lone `*` matches the empty string (shortest) or everything
        // (longest), on either side: `${x#*}` and `${x%*}` keep `x` whole.
        if pattern == "*" {
            return if longest {
                String::new()
            } else {
                value.to_string()
            };
        }

        // The literal fast paths below handle one `*`. Anything else (`?`,
        // brackets, extglob, several stars) goes through glob_match.
        if Self::has_unescaped_char(pattern, '[')
            || Self::has_unescaped_char(pattern, '?')
            || self.contains_unescaped_extglob(pattern)
            || Self::find_unescaped_char(pattern, '*')
                .is_some_and(|star| Self::find_unescaped_char(&pattern[star + 1..], '*').is_some())
        {
            return self.remove_pattern_glob(value, pattern, prefix, longest);
        }

        let literal_pattern = Self::unescape_pattern_literal(pattern);

        if prefix {
            // Remove from beginning
            // Check if pattern contains *
            if let Some(star_pos) = Self::find_unescaped_char(pattern, '*') {
                let prefix_part = &pattern[..star_pos];
                let suffix_part = &pattern[star_pos + 1..];
                let prefix_part = Self::unescape_pattern_literal(prefix_part);
                let suffix_part = Self::unescape_pattern_literal(suffix_part);

                if prefix_part.is_empty() {
                    // Pattern is "*suffix" - find suffix and remove everything before it
                    if longest {
                        // Find last occurrence of suffix
                        if let Some(pos) = value.rfind(&suffix_part) {
                            return value[pos + suffix_part.len()..].to_string();
                        }
                    } else {
                        // Find first occurrence of suffix
                        if let Some(pos) = value.find(&suffix_part) {
                            return value[pos + suffix_part.len()..].to_string();
                        }
                    }
                } else if suffix_part.is_empty() {
                    // Pattern is "prefix*" - match prefix and any chars after
                    if let Some(rest) = value.strip_prefix(&prefix_part) {
                        if longest {
                            return String::new();
                        } else {
                            return rest.to_string();
                        }
                    }
                } else {
                    // Pattern is "prefix*suffix" - more complex matching
                    if let Some(rest) = value.strip_prefix(&prefix_part) {
                        if longest {
                            if let Some(pos) = rest.rfind(&suffix_part) {
                                return rest[pos + suffix_part.len()..].to_string();
                            }
                        } else if let Some(pos) = rest.find(&suffix_part) {
                            return rest[pos + suffix_part.len()..].to_string();
                        }
                    }
                }
            } else if let Some(rest) = value.strip_prefix(&literal_pattern) {
                return rest.to_string();
            }
        } else {
            // Remove from end (suffix)
            // Check if pattern contains *
            if let Some(star_pos) = Self::find_unescaped_char(pattern, '*') {
                let prefix_part = &pattern[..star_pos];
                let suffix_part = &pattern[star_pos + 1..];
                let prefix_part = Self::unescape_pattern_literal(prefix_part);
                let suffix_part = Self::unescape_pattern_literal(suffix_part);

                if suffix_part.is_empty() {
                    // Pattern is "prefix*" - find prefix and remove from there to end
                    if longest {
                        // Find first occurrence of prefix
                        if let Some(pos) = value.find(&prefix_part) {
                            return value[..pos].to_string();
                        }
                    } else {
                        // Find last occurrence of prefix
                        if let Some(pos) = value.rfind(&prefix_part) {
                            return value[..pos].to_string();
                        }
                    }
                } else if prefix_part.is_empty() {
                    // Pattern is "*suffix" - match any chars before suffix
                    if let Some(before) = value.strip_suffix(&suffix_part) {
                        if longest {
                            return String::new();
                        } else {
                            return before.to_string();
                        }
                    }
                } else {
                    // Pattern is "prefix*suffix" - more complex matching
                    if let Some(before_suffix) = value.strip_suffix(&suffix_part) {
                        if longest {
                            if let Some(pos) = before_suffix.find(&prefix_part) {
                                return value[..pos].to_string();
                            }
                        } else if let Some(pos) = before_suffix.rfind(&prefix_part) {
                            return value[..pos].to_string();
                        }
                    }
                }
            } else if let Some(before) = value.strip_suffix(&literal_pattern) {
                return before.to_string();
            }
        }

        value.to_string()
    }

    /// Remove prefix/suffix using glob_match for patterns with brackets or extglob.
    ///
    /// THREAT[TM-DOS]: Cap glob_match invocations to prevent O(n^2) CPU
    /// exhaustion on long strings with bracket/extglob patterns.
    pub(super) fn remove_pattern_glob(
        &self,
        value: &str,
        pattern: &str,
        prefix: bool,
        longest: bool,
    ) -> String {
        const MAX_GLOB_MATCH_CALLS: usize = 10_000;
        let chars: Vec<char> = value.chars().collect();
        let mut calls = 0usize;
        if prefix {
            // Try each prefix length; shortest = first match, longest = last match
            let mut last_match = None;
            for i in 0..=chars.len() {
                calls += 1;
                if calls > MAX_GLOB_MATCH_CALLS {
                    break;
                }
                let candidate: String = chars[..i].iter().collect();
                if self.glob_match(&candidate, pattern) {
                    if !longest {
                        return chars[i..].iter().collect();
                    }
                    last_match = Some(i);
                }
            }
            if let Some(i) = last_match {
                return chars[i..].iter().collect();
            }
        } else {
            // Suffix removal: try each suffix length
            let mut last_match = None;
            for i in (0..=chars.len()).rev() {
                calls += 1;
                if calls > MAX_GLOB_MATCH_CALLS {
                    break;
                }
                let candidate: String = chars[i..].iter().collect();
                if self.glob_match(&candidate, pattern) {
                    if !longest {
                        return chars[..i].iter().collect();
                    }
                    last_match = Some(i);
                }
            }
            if let Some(i) = last_match {
                return chars[..i].iter().collect();
            }
        }
        value.to_string()
    }
}

#[cfg(test)]
mod expansion_charge_tests {
    use super::*;

    fn word(quoted: bool, has_unquoted_glob: bool) -> Word {
        Word {
            parts: Vec::new(),
            quoted,
            has_unquoted_glob,
            part_quoted: Vec::new(),
            raw: None,
        }
    }

    /// THREAT[TM-DOS-115]: the budget charge must equal what actually gets
    /// appended. If these drift, a substitution is under-charged and the
    /// live-byte cap stops bounding it.
    #[test]
    fn appended_len_matches_what_append_writes() {
        let values = [
            "",
            "plain text",
            "*?[]{}@!+()|\\",
            "mixed *glob* and text",
            "unicode: héllo → 世界 *",
            "\\\\already\\\\escaped",
            "trailing backslash \\",
        ];
        for quoted in [false, true] {
            for has_unquoted_glob in [false, true] {
                let w = word(quoted, has_unquoted_glob);
                for value in values {
                    let mut appended = String::new();
                    Interpreter::append_expansion_for_word(&mut appended, &w, value);
                    assert_eq!(
                        Interpreter::expansion_appended_len(&w, value),
                        appended.len(),
                        "charge != appended for {value:?} \
                         (quoted={quoted}, has_unquoted_glob={has_unquoted_glob})"
                    );
                }
            }
        }
    }
}

/// Where `${var/pattern/rep}` must match.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum PatternAnchor {
    None,
    Start,
    End,
}
