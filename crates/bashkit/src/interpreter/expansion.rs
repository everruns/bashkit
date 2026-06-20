//! Word and parameter expansion.
//!
//! Split out of interpreter/mod.rs: the core `expand_word` /
//! `expand_word_to_fields` pipeline, parameter-expansion operators
//! (`${x:-y}`, `${x/a/b}`, `${x#p}`, ...), IFS field splitting, operand
//! quoting, and pattern matching helpers. Command-substitution and
//! subshell-snapshot machinery stay in the parent module.

use super::*;

impl Interpreter {
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
            if let Some(arr) = self.scoped.assoc_arrays.get(arr_name) {
                let mut keys: Vec<_> = arr.keys().collect();
                keys.sort();
                let values: Vec<String> =
                    keys.iter().filter_map(|k| arr.get(*k).cloned()).collect();
                result.push_str(&values.join(&sep));
            } else if let Some(arr) = self.scoped.arrays.get(arr_name) {
                let mut indices: Vec<_> = arr.keys().collect();
                indices.sort();
                let values: Vec<_> = indices.iter().filter_map(|i| arr.get(i)).collect();
                result.push_str(&values.into_iter().cloned().collect::<Vec<_>>().join(&sep));
            }
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
        } else {
            let idx = self.resolve_indexed_array_subscript(arr_name, index);
            if let Some(arr) = self.scoped.arrays.get(arr_name)
                && let Some(value) = arr.get(&idx)
            {
                result.push_str(value);
            }
        }
        result
    }

    /// Apply a `${var@operator}` transformation.
    pub(super) fn apply_transformation(&self, name: &str, operator: char) -> String {
        let value = self.expand_variable(name);
        match operator {
            'Q' => format!("'{}'", value.replace('\'', "'\\''")),
            'E' => value
                .replace("\\n", "\n")
                .replace("\\t", "\t")
                .replace("\\\\", "\\"),
            'P' => value.clone(),
            'A' => format!("{}='{}'", name, value.replace('\'', "'\\''")),
            'K' => value.clone(),
            'a' => {
                let mut attrs = String::new();
                if self.is_var_readonly(name) {
                    attrs.push('r');
                }
                if self.env.contains_key(name) {
                    attrs.push('x');
                }
                attrs
            }
            'u' | 'U' => {
                if operator == 'U' {
                    value.to_uppercase()
                } else {
                    let mut chars = value.chars();
                    match chars.next() {
                        Some(first) => first.to_uppercase().collect::<String>() + chars.as_str(),
                        None => String::new(),
                    }
                }
            }
            'L' => value.to_lowercase(),
            _ => value.clone(),
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
            Ok(Self::strip_quote_markers(&expanded))
        })
    }

    /// Quote expansion output that came from a quoted segment of a mixed word.
    /// THREAT[TM-INF-022]: Quoted user-controlled values must stay literal; only
    /// unquoted suffix/prefix glob syntax in the source word may drive expansion.
    pub(super) fn quote_expansion_for_quoted_glob(value: &str) -> String {
        let mut quoted = String::with_capacity(value.len());
        for ch in value.chars() {
            if matches!(
                ch,
                '\\' | '*' | '?' | '[' | ']' | '{' | '}' | '@' | '!' | '+' | '(' | ')' | '|'
            ) {
                quoted.push('\\');
            }
            quoted.push(ch);
        }
        quoted
    }

    pub(super) fn append_expansion_for_word(result: &mut String, word: &Word, value: &str) {
        if word.quoted && word.has_unquoted_glob {
            result.push_str(&Self::quote_expansion_for_quoted_glob(value));
        } else {
            result.push_str(value);
        }
    }

    pub(super) async fn expand_word_inner(&mut self, word: &Word) -> Result<String> {
        let mut result = String::new();
        let mut is_first_part = true;

        for part in &word.parts {
            match part {
                WordPart::Literal(s) => {
                    // Tilde expansion: ~ at start of word expands to $HOME
                    if is_first_part && s.starts_with('~') {
                        let home = self
                            .env
                            .get("HOME")
                            .or_else(|| self.scoped.variables.get("HOME"))
                            .cloned()
                            .unwrap_or_else(|| "/home/user".to_string());

                        if s == "~" {
                            result.push_str(&home);
                        } else if s.starts_with("~/") {
                            result.push_str(&home);
                            result.push_str(&s[1..]);
                        } else {
                            result.push_str(s);
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
                    Self::append_expansion_for_word(&mut result, word, &trimmed);
                }
                WordPart::ArithmeticExpansion(expr) => {
                    let expanded_expr = if expr.contains("$(") {
                        self.expand_command_subs_in_arithmetic(expr).await?
                    } else {
                        expr.to_string()
                    };
                    let value = self.evaluate_arithmetic_with_assign(&expanded_expr);
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
                        let idx: usize =
                            self.evaluate_arithmetic(index_str).try_into().unwrap_or(0);
                        if let Some(arr) = self.scoped.arrays.get(arr_name) {
                            arr.get(&idx).cloned().unwrap_or_default()
                        } else {
                            String::new()
                        }
                    } else {
                        self.expand_variable(name)
                    };
                    result.push_str(&value.chars().count().to_string());
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
                    let resolved = self.resolve_nameref(name);
                    if let Some(arr) = self.scoped.assoc_arrays.get(resolved) {
                        let mut keys: Vec<_> = arr.keys().cloned().collect();
                        keys.sort();
                        Self::append_expansion_for_word(&mut result, word, &keys.join(" "));
                    } else if let Some(arr) = self.scoped.arrays.get(resolved) {
                        let mut indices: Vec<_> = arr.keys().collect();
                        indices.sort();
                        let index_strs: Vec<String> =
                            indices.iter().map(|i| i.to_string()).collect();
                        Self::append_expansion_for_word(&mut result, word, &index_strs.join(" "));
                    }
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
                    if let Some(arr) = self.scoped.arrays.get(name) {
                        let mut indices: Vec<_> = arr.keys().cloned().collect();
                        indices.sort();
                        let values: Vec<_> =
                            indices.iter().filter_map(|i| arr.get(i).cloned()).collect();

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
                        // Nameref without operator: ${!ref} returns the
                        // name the nameref points to (original behavior).
                        if let Some(ref target) = nameref_target {
                            Self::append_expansion_for_word(&mut result, word, target);
                        }
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
                    if let Some(arr) = self.scoped.assoc_arrays.get(resolved) {
                        result.push_str(&arr.len().to_string());
                    } else if let Some(arr) = self.scoped.arrays.get(resolved) {
                        result.push_str(&arr.len().to_string());
                    } else {
                        result.push('0');
                    }
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
                    // Check assoc arrays first
                    if let Some(arr) = self.scoped.assoc_arrays.get(name) {
                        let mut keys: Vec<_> = arr.keys().cloned().collect();
                        keys.sort();
                        let values: Vec<String> =
                            keys.iter().filter_map(|k| arr.get(k).cloned()).collect();
                        if word.quoted && index == "*" {
                            let sep = self.get_ifs_separator();
                            return Ok(vec![values.join(&sep)]);
                        }
                        return Ok(values);
                    }
                    if let Some(arr) = self.scoped.arrays.get(name) {
                        let mut indices: Vec<_> = arr.keys().collect();
                        indices.sort();
                        let values: Vec<String> =
                            indices.iter().filter_map(|i| arr.get(i).cloned()).collect();
                        // "${arr[*]}" joins into single field with IFS; "${arr[@]}" keeps separate
                        if word.quoted && index == "*" {
                            let sep = self.get_ifs_separator();
                            return Ok(vec![values.join(&sep)]);
                        }
                        return Ok(values);
                    }
                    return Ok(Vec::new());
                }
                // "${!arr[@]}" - array keys/indices as separate fields
                if let WordPart::ArrayIndices(name) = &word.parts[0] {
                    let resolved = self.resolve_nameref(name);
                    if let Some(arr) = self.scoped.assoc_arrays.get(resolved) {
                        let mut keys: Vec<_> = arr.keys().cloned().collect();
                        keys.sort();
                        return Ok(keys);
                    }
                    if let Some(arr) = self.scoped.arrays.get(resolved) {
                        let mut indices: Vec<_> = arr.keys().collect();
                        indices.sort();
                        return Ok(indices.iter().map(|i| i.to_string()).collect());
                    }
                    return Ok(Vec::new());
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
                    let part_has_expansion = matches!(
                        part,
                        WordPart::Variable(_)
                            | WordPart::CommandSubstitution(_)
                            | WordPart::ArithmeticExpansion(_)
                            | WordPart::ParameterExpansion { .. }
                            | WordPart::ArrayAccess { .. }
                    );
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
                && word.parts.iter().any(|p| {
                    matches!(
                        p,
                        WordPart::Variable(_)
                            | WordPart::CommandSubstitution(_)
                            | WordPart::ArithmeticExpansion(_)
                            | WordPart::ParameterExpansion { .. }
                            | WordPart::ArrayAccess { .. }
                    )
                });

            if has_expansion {
                self.ifs_split(&expanded)
            } else {
                Ok(vec![Self::strip_quote_markers(&expanded)])
            }
        })
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
            if let Some(arr) = self.scoped.assoc_arrays.get(resolved_arr_name) {
                let is_set = !arr.is_empty();
                let mut keys: Vec<_> = arr.keys().collect();
                keys.sort();
                let values: Vec<String> =
                    keys.iter().filter_map(|k| arr.get(*k).cloned()).collect();
                return (is_set, values.join(&sep));
            }
            if let Some(arr) = self.scoped.arrays.get(resolved_arr_name) {
                let is_set = !arr.is_empty();
                let mut indices: Vec<_> = arr.keys().collect();
                indices.sort();
                let values: Vec<_> = indices.iter().filter_map(|i| arr.get(i)).collect();
                return (
                    is_set,
                    values.into_iter().cloned().collect::<Vec<_>>().join(&sep),
                );
            }
            return (false, String::new());
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
            return (false, String::new());
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
            let resolved = self.resolve_nameref(arr_name);
            if let Some(arr) = self.scoped.assoc_arrays.get(resolved) {
                let mut keys: Vec<_> = arr.keys().collect();
                keys.sort();
                return Some(keys.iter().filter_map(|k| arr.get(*k).cloned()).collect());
            }
            if let Some(arr) = self.scoped.arrays.get(resolved) {
                let mut indices: Vec<_> = arr.keys().collect();
                indices.sort();
                return Some(indices.iter().filter_map(|i| arr.get(i).cloned()).collect());
            }
            return Some(Vec::new());
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
                    let value = self.expand_variable(name).len().to_string();
                    Self::push_operand_expansion(&mut result, &value, in_marked || force_quoted);
                }
                // TODO: handle CommandSubstitution etc. in sync operand expansion
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
    pub(super) fn apply_param_op_maybe_per_element(
        &mut self,
        value: &str,
        name: &str,
        operator: &ParameterOp,
        operand: &str,
        colon_variant: bool,
        is_set: bool,
    ) -> String {
        let needs_per_element = matches!(
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
        );
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
                let expanded = self.expand_operand(operand);
                self.remove_pattern(value, &expanded, true, false)
            }
            ParameterOp::RemovePrefixLong => {
                // ${var##pattern} - remove longest prefix match
                let expanded = self.expand_operand(operand);
                self.remove_pattern(value, &expanded, true, true)
            }
            ParameterOp::RemoveSuffixShort => {
                // ${var%pattern} - remove shortest suffix match
                let expanded = self.expand_operand(operand);
                self.remove_pattern(value, &expanded, false, false)
            }
            ParameterOp::RemoveSuffixLong => {
                // ${var%%pattern} - remove longest suffix match
                let expanded = self.expand_operand(operand);
                self.remove_pattern(value, &expanded, false, true)
            }
            ParameterOp::ReplaceFirst {
                pattern,
                replacement,
            } => {
                // ${var/pattern/replacement} - replace first occurrence
                let expanded_rep = self.expand_operand(replacement);
                self.replace_pattern(value, pattern, &expanded_rep, false)
            }
            ParameterOp::ReplaceAll {
                pattern,
                replacement,
            } => {
                // ${var//pattern/replacement} - replace all occurrences
                let expanded_rep = self.expand_operand(replacement);
                self.replace_pattern(value, pattern, &expanded_rep, true)
            }
            ParameterOp::UpperFirst => {
                // ${var^} - uppercase first character
                let mut chars = value.chars();
                match chars.next() {
                    Some(first) => first.to_uppercase().collect::<String>() + chars.as_str(),
                    None => String::new(),
                }
            }
            ParameterOp::UpperAll => {
                // ${var^^} - uppercase all characters
                value.to_uppercase()
            }
            ParameterOp::LowerFirst => {
                // ${var,} - lowercase first character
                let mut chars = value.chars();
                match chars.next() {
                    Some(first) => first.to_lowercase().collect::<String>() + chars.as_str(),
                    None => String::new(),
                }
            }
            ParameterOp::LowerAll => {
                // ${var,,} - lowercase all characters
                value.to_lowercase()
            }
        }
    }

    /// Replace pattern in value
    /// THREAT[TM-DOS]: Maximum expansion result size (10MB) to prevent memory
    /// amplification in global pattern replacement.
    pub(crate) const MAX_EXPANSION_RESULT_BYTES: usize = 10 * 1024 * 1024;

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

        // Handle # prefix anchor (match at start only)
        if let Some(rest) = pattern.strip_prefix('#') {
            if rest.is_empty() {
                // ${var/#/rep} with empty pattern: prepend replacement
                return concat_or_original(&[replacement, value])
                    .unwrap_or_else(|| value.to_string());
            }
            if let Some(stripped) = value.strip_prefix(rest) {
                return concat_or_original(&[replacement, stripped])
                    .unwrap_or_else(|| value.to_string());
            }
            // Try glob match at prefix
            if rest.contains('*') {
                let matched = self.remove_pattern(value, rest, true, false);
                if matched != value {
                    let prefix_len = value.len() - matched.len();
                    return concat_or_original(&[replacement, &value[prefix_len..]])
                        .unwrap_or_else(|| value.to_string());
                }
            }
            return value.to_string();
        }

        // Handle % suffix anchor (match at end only)
        if let Some(rest) = pattern.strip_prefix('%') {
            if rest.is_empty() {
                // ${var/%/rep} with empty pattern: append replacement
                return concat_or_original(&[value, replacement])
                    .unwrap_or_else(|| value.to_string());
            }
            if let Some(stripped) = value.strip_suffix(rest) {
                return concat_or_original(&[stripped, replacement])
                    .unwrap_or_else(|| value.to_string());
            }
            // Try glob match at suffix
            if rest.contains('*') {
                let matched = self.remove_pattern(value, rest, false, false);
                if matched != value {
                    return concat_or_original(&[&matched, replacement])
                        .unwrap_or_else(|| value.to_string());
                }
            }
            return value.to_string();
        }

        // Handle glob pattern with *
        if pattern.contains('*') {
            // Convert glob to regex-like behavior
            // For simplicity, we'll handle basic cases: prefix*, *suffix, *middle*
            if pattern == "*" {
                // Replace everything
                if replacement.len() > Self::MAX_EXPANSION_RESULT_BYTES {
                    return value.to_string();
                }
                return replacement.to_string();
            }

            if let Some(star_pos) = pattern.find('*') {
                let prefix = &pattern[..star_pos];
                let suffix = &pattern[star_pos + 1..];

                if prefix.is_empty() && !suffix.is_empty() {
                    // *suffix - match anything ending with suffix
                    if let Some(pos) = value.find(suffix) {
                        let after = &value[pos + suffix.len()..];
                        if global {
                            let result = replacement.to_string()
                                + &self.replace_pattern(after, pattern, replacement, true);
                            if result.len() > Self::MAX_EXPANSION_RESULT_BYTES {
                                return value.to_string();
                            }
                            return result;
                        } else {
                            return concat_or_original(&[replacement, after])
                                .unwrap_or_else(|| value.to_string());
                        }
                    }
                } else if !prefix.is_empty() && suffix.is_empty() {
                    // prefix* - match prefix and anything after
                    if value.starts_with(prefix) {
                        if replacement.len() > Self::MAX_EXPANSION_RESULT_BYTES {
                            return value.to_string();
                        }
                        return replacement.to_string();
                    }
                }
            }
            // If we can't match the glob pattern, return as-is
            return value.to_string();
        }

        // Simple string replacement
        if global {
            let result = value.replace(pattern, replacement);
            if result.len() > Self::MAX_EXPANSION_RESULT_BYTES {
                return value.to_string();
            }
            result
        } else {
            let result = value.replacen(pattern, replacement, 1);
            if result.len() > Self::MAX_EXPANSION_RESULT_BYTES {
                return value.to_string();
            }
            result
        }
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

        // Use glob_match for patterns with bracket expressions or extglob
        if Self::has_unescaped_char(pattern, '[') || self.contains_unescaped_extglob(pattern) {
            return self.remove_pattern_glob(value, pattern, prefix, longest);
        }

        let literal_pattern = Self::unescape_pattern_literal(pattern);

        if prefix {
            // Remove from beginning
            if pattern == "*" {
                if longest {
                    return String::new();
                } else if !value.is_empty() {
                    return value.chars().skip(1).collect();
                } else {
                    return value.to_string();
                }
            }

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
            if pattern == "*" {
                if longest {
                    return String::new();
                } else if !value.is_empty() {
                    let mut s = value.to_string();
                    s.pop();
                    return s;
                } else {
                    return value.to_string();
                }
            }

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
