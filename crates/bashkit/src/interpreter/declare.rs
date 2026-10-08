//! Declaration builtins: `declare`/`typeset`, `local`, `export`, `readonly`.
//!
//! Important decisions:
//! - One engine serves all four builtins (bash shares `declare_internal`):
//!   each builtin only restricts its option letters and fixes an attribute
//!   (`export` adds `-x`, `readonly` adds `-r`, `local` makes the name local).
//! - Compound values arrive unexpanded. The parser turns `name=(...)` in an
//!   argument into a `WordPart::CompoundAssignment`; argument expansion
//!   replaces it with a placeholder (`COMPOUND_MARK`) and parks the element
//!   words in `pending_compound_args`, which the engine takes. A quoted
//!   `'(...)'` value is a plain string unless `-a`/`-A` asks for an array,
//!   as in bash.
//! - Listing (`declare -p`, `export -p`, plain `declare`) formats values the
//!   way bash does: `"..."` with `\" \\ \$ \``` escaped, or `$'...'` when the
//!   value holds control characters; associative arrays in bash's hash order.

use super::*;

/// Marks the placeholder that stands for a compound argument (`a=(...)`).
/// A private-use character, so ordinary text never contains it.
pub(super) const COMPOUND_MARK: char = '\u{F8FF}';

/// Which builtin is running the declaration engine.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(super) enum DeclKind {
    Declare,
    Local,
    Export,
    Readonly,
}

impl DeclKind {
    fn allowed_options(self) -> &'static str {
        match self {
            DeclKind::Declare | DeclKind::Local => "aAfFgiIlnprtux",
            DeclKind::Export => "fnp",
            DeclKind::Readonly => "aAfp",
        }
    }

    fn usage(self, cmd: &str) -> String {
        match self {
            DeclKind::Declare => format!(
                "{cmd}: usage: {cmd} [-aAfFgiIlnrtux] [name[=value] ...] or {cmd} -p [-aAfFilnrtux] [name ...]\n"
            ),
            DeclKind::Local => "local: usage: local [option] name[=value] ...\n".to_string(),
            DeclKind::Export => {
                "export: usage: export [-fn] [name[=value] ...] or export -p\n".to_string()
            }
            DeclKind::Readonly => {
                "readonly: usage: readonly [-aAf] [name[=value] ...] or readonly -p\n".to_string()
            }
        }
    }
}

/// Attribute letters given with `-` (set) or `+` (clear).
#[derive(Default, Clone, Copy)]
struct AttrFlags {
    indexed: bool,
    assoc: bool,
    integer: bool,
    nameref: bool,
    readonly: bool,
    export: bool,
    lower: bool,
    upper: bool,
}

impl AttrFlags {
    fn any(&self) -> bool {
        self.indexed
            || self.assoc
            || self.integer
            || self.nameref
            || self.readonly
            || self.export
            || self.lower
            || self.upper
    }
}

#[derive(Default)]
struct DeclOpts {
    on: AttrFlags,
    off: AttrFlags,
    print: bool,
    functions: bool,
    function_names: bool,
    global: bool,
}

/// The value side of one `name=value` operand.
enum DeclValue {
    Str(String),
    Compound(Vec<Word>),
}

/// Kind of the live binding of a (nameref-resolved) name.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(super) enum VarKind {
    Scalar,
    Indexed,
    Assoc,
    Unset,
}

/// Is `s` usable as a nameref target: a name or `name[subscript]`.
pub(super) fn valid_nameref_target(s: &str) -> bool {
    match s.find('[') {
        Some(b) => is_valid_var_name(&s[..b]) && s.ends_with(']') && s.len() > b + 2,
        None => is_valid_var_name(s),
    }
}

/// Position of the `=` that ends an assignment operand's name, skipping
/// `=` inside a `[subscript]`.
fn operand_eq_pos(arg: &str) -> Option<usize> {
    let mut depth = 0usize;
    for (i, c) in arg.char_indices() {
        match c {
            '[' => depth += 1,
            ']' => depth = depth.saturating_sub(1),
            '=' if depth == 0 => return Some(i),
            _ => {}
        }
    }
    None
}

/// Does `s` need `$'...'` quoting (it holds control characters)?
pub(super) fn ansic_should_quote(s: &str) -> bool {
    s.chars().any(|c| c.is_control())
}

/// Quote `s` as `$'...'` the way bash's `ansic_quote` does.
pub(super) fn ansic_quote(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 3);
    out.push_str("$'");
    for c in s.chars() {
        match c {
            '\x07' => out.push_str("\\a"),
            '\x08' => out.push_str("\\b"),
            '\x1b' => out.push_str("\\E"),
            '\x0c' => out.push_str("\\f"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            '\x0b' => out.push_str("\\v"),
            '\\' => out.push_str("\\\\"),
            '\'' => out.push_str("\\'"),
            c if c.is_control() => {
                let mut buf = [0u8; 4];
                for b in c.encode_utf8(&mut buf).bytes() {
                    out.push_str(&format!("\\{b:03o}"));
                }
            }
            c => out.push(c),
        }
    }
    out.push('\'');
    out
}

/// Double-quote `s`, escaping `"`, `\`, `$` and `` ` `` (bash `sh_double_quote`).
pub(super) fn double_quote(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    out.push('"');
    for c in s.chars() {
        if matches!(c, '"' | '\\' | '$' | '`') {
            out.push('\\');
        }
        out.push(c);
    }
    out.push('"');
    out
}

/// Quote a value for `declare -p`.
pub(super) fn declare_quote(s: &str) -> String {
    if ansic_should_quote(s) {
        ansic_quote(s)
    } else {
        double_quote(s)
    }
}

/// Bash `sh_contains_shell_metas`.
fn contains_shell_metas(s: &str) -> bool {
    let chars: Vec<char> = s.chars().collect();
    for (i, &c) in chars.iter().enumerate() {
        match c {
            ' ' | '\t' | '\n' | '\'' | '"' | '\\' | '|' | '&' | ';' | '(' | ')' | '<' | '>'
            | '!' | '{' | '}' | '*' | '[' | '?' | ']' | '^' | '$' | '`' => return true,
            '~' if i == 0 || matches!(chars[i - 1], '=' | ':') => return true,
            '#' if i == 0 => return true,
            _ => {}
        }
    }
    false
}

/// Quote a value for plain `declare`/`set` listing.
fn set_quote(s: &str) -> String {
    if ansic_should_quote(s) {
        ansic_quote(s)
    } else if contains_shell_metas(s) {
        format!("'{}'", s.replace('\'', "'\\''"))
    } else {
        s.to_string()
    }
}

/// FNV-1 32-bit, the hash bash uses for associative arrays.
fn bash_hash(s: &str) -> u32 {
    let mut h: u32 = 2_166_136_261;
    for b in s.bytes() {
        h = h.wrapping_mul(16_777_619);
        h ^= u32::from(b);
    }
    h
}

/// Keys of an associative array in the order bash iterates them: by hash
/// bucket (1024 buckets). Keys sharing a bucket are ordered by key, since
/// insertion order is not tracked.
pub(super) fn assoc_keys_bash_order(arr: &HashMap<String, String>) -> Vec<&String> {
    let mut keys: Vec<&String> = arr.keys().collect();
    keys.sort_by(|a, b| {
        (bash_hash(a) & 1023)
            .cmp(&(bash_hash(b) & 1023))
            .then_with(|| b.cmp(a))
    });
    keys
}

/// Format an indexed array body: `([0]="a" [1]="b")`.
fn format_indexed_body(arr: &HashMap<usize, String>) -> String {
    let mut idx: Vec<&usize> = arr.keys().collect();
    idx.sort();
    let items: Vec<String> = idx
        .iter()
        .map(|i| format!("[{}]={}", i, declare_quote(&arr[*i])))
        .collect();
    format!("({})", items.join(" "))
}

/// Format an associative array body: `([k]="v" )` (bash keeps a trailing space).
fn format_assoc_body(arr: &HashMap<String, String>) -> String {
    if arr.is_empty() {
        return "()".to_string();
    }
    let mut out = String::from("(");
    for k in assoc_keys_bash_order(arr) {
        let key = if contains_shell_metas(k) || k.is_empty() {
            double_quote(k)
        } else {
            k.clone()
        };
        out.push_str(&format!("[{}]={} ", key, declare_quote(&arr[k])));
    }
    out.push(')');
    out
}

/// Split a `[key]=value` compound element into key and value words.
/// Returns `None` for an ordinary element.
pub(super) fn split_keyed_word(word: &Word) -> Option<(Word, Word, bool)> {
    let quoted_at = |i: usize| word.part_quoted.get(i).copied().unwrap_or(word.quoted);
    match word.parts.first() {
        Some(WordPart::Literal(s)) if s.starts_with('[') && !quoted_at(0) => {}
        _ => return None,
    }
    let mut depth = 0usize;
    for (pi, part) in word.parts.iter().enumerate() {
        let WordPart::Literal(s) = part else {
            continue;
        };
        if quoted_at(pi) {
            continue;
        }
        let start = if pi == 0 { 1 } else { 0 };
        if pi == 0 {
            depth = 1;
        }
        for (ci, c) in s.char_indices().skip(start) {
            match c {
                '[' => depth += 1,
                ']' => {
                    depth -= 1;
                    if depth == 0 {
                        let rest = &s[ci + 1..];
                        let (append, value_head) = if let Some(v) = rest.strip_prefix("+=") {
                            (true, v)
                        } else if let Some(v) = rest.strip_prefix('=') {
                            (false, v)
                        } else {
                            return None;
                        };
                        let key_head = if pi == 0 { &s[1..ci] } else { &s[..ci] };
                        let mut key_parts = Vec::new();
                        let mut key_quoted = Vec::new();
                        for (i, p) in word.parts.iter().enumerate().take(pi) {
                            if i == 0 {
                                if let WordPart::Literal(l) = p {
                                    key_parts.push(WordPart::Literal(l[1..].to_string()));
                                }
                            } else {
                                key_parts.push(p.clone());
                            }
                            key_quoted.push(quoted_at(i));
                        }
                        if pi == 0 || !key_head.is_empty() {
                            key_parts.push(WordPart::Literal(key_head.to_string()));
                            key_quoted.push(false);
                        }
                        let mut value_parts = Vec::new();
                        let mut value_quoted = Vec::new();
                        if !value_head.is_empty() {
                            value_parts.push(WordPart::Literal(value_head.to_string()));
                            value_quoted.push(false);
                        }
                        for (i, p) in word.parts.iter().enumerate().skip(pi + 1) {
                            value_parts.push(p.clone());
                            value_quoted.push(quoted_at(i));
                        }
                        let make = |parts: Vec<WordPart>, quoted: Vec<bool>| Word {
                            quoted: !quoted.is_empty() && quoted.iter().all(|q| *q),
                            parts,
                            has_unquoted_glob: false,
                            part_quoted: quoted,
                            raw: None,
                        };
                        return Some((
                            make(key_parts, key_quoted),
                            make(value_parts, value_quoted),
                            append,
                        ));
                    }
                }
                _ => {}
            }
        }
    }
    None
}

impl Interpreter {
    /// Kind of the live binding of `name` (no nameref resolution).
    pub(super) fn var_kind(&self, name: &str) -> VarKind {
        if self.scoped.assoc_arrays.contains_key(name) {
            VarKind::Assoc
        } else if self.scoped.arrays.contains_key(name) {
            VarKind::Indexed
        } else if self.scoped.variables.contains_key(name) || self.env.contains_key(name) {
            VarKind::Scalar
        } else {
            VarKind::Unset
        }
    }

    /// Is `name` local to any active function frame?
    pub(super) fn is_local_anywhere(&self, name: &str) -> bool {
        self.call_stack
            .iter()
            .any(|f| f.saved_vars.contains_key(name))
    }

    /// Does `name` exist as far as `declare -p` is concerned (set, or
    /// declared with attributes but no value)?
    fn declared(&self, name: &str) -> bool {
        self.var_kind(name) != VarKind::Unset
            || self.scoped.var_attrs.contains_key(name)
            || self.scoped.namerefs.contains_key(name)
            || self.is_local_anywhere(name)
    }

    /// Attribute letters of `name` in bash's order (`aAinrxlu`), or `-`.
    fn attr_letters(&self, name: &str) -> String {
        let attrs = self.var_attrs_get(name);
        let mut s = String::new();
        match self.var_kind(name) {
            VarKind::Indexed => s.push('a'),
            VarKind::Assoc => s.push('A'),
            _ => {}
        }
        if attrs.contains(VarAttrs::INTEGER) {
            s.push('i');
        }
        if self.scoped.namerefs.contains_key(name) {
            s.push('n');
        }
        if attrs.contains(VarAttrs::READONLY) {
            s.push('r');
        }
        if attrs.contains(VarAttrs::EXPORT) || self.env.contains_key(name) {
            s.push('x');
        }
        if attrs.contains(VarAttrs::LOWER) {
            s.push('l');
        }
        if attrs.contains(VarAttrs::UPPER) {
            s.push('u');
        }
        if s.is_empty() {
            s.push('-');
        }
        s
    }

    /// One `declare -p` line for `name`, or `None` when it does not exist.
    pub(super) fn format_declare_line(&self, name: &str) -> Option<String> {
        if !self.declared(name) {
            return None;
        }
        let flags = self.attr_letters(name);
        let novalue = self.var_attrs_get(name).contains(VarAttrs::NOVALUE);
        let body = if let Some(target) = self.scoped.namerefs.get(name) {
            (!target.is_empty()).then(|| double_quote(target))
        } else {
            match self.var_kind(name) {
                VarKind::Indexed => {
                    let arr = &self.scoped.arrays[name];
                    (!(arr.is_empty() && novalue)).then(|| format_indexed_body(arr))
                }
                VarKind::Assoc => {
                    let arr = &self.scoped.assoc_arrays[name];
                    (!(arr.is_empty() && novalue)).then(|| format_assoc_body(arr))
                }
                VarKind::Scalar => self
                    .scoped
                    .variables
                    .get(name)
                    .or_else(|| self.env.get(name))
                    .map(|v| declare_quote(v)),
                VarKind::Unset => None,
            }
        };
        Some(match body {
            Some(b) => format!("declare -{flags} {name}={b}\n"),
            None => format!("declare -{flags} {name}\n"),
        })
    }

    /// All variable names visible to listings, sorted, hidden ones dropped.
    fn listable_names(&self) -> Vec<String> {
        let mut names: std::collections::BTreeSet<String> = std::collections::BTreeSet::new();
        names.extend(self.scoped.variables.keys().cloned());
        names.extend(self.env.keys().cloned());
        names.extend(self.scoped.arrays.keys().cloned());
        names.extend(self.scoped.assoc_arrays.keys().cloned());
        names.extend(self.scoped.var_attrs.keys().cloned());
        names.extend(self.scoped.namerefs.keys().cloned());
        names
            .into_iter()
            // THREAT[TM-INF-017]: never list internal marker variables.
            .filter(|n| !is_hidden_variable(n) && !is_internal_variable(n))
            .collect()
    }

    /// Does `name` carry every attribute set in `want`?
    fn has_attrs(&self, name: &str, want: &AttrFlags) -> bool {
        let attrs = self.var_attrs_get(name);
        let kind = self.var_kind(name);
        (!want.indexed || kind == VarKind::Indexed)
            && (!want.assoc || kind == VarKind::Assoc)
            && (!want.integer || attrs.contains(VarAttrs::INTEGER))
            && (!want.nameref || self.scoped.namerefs.contains_key(name))
            && (!want.readonly || attrs.contains(VarAttrs::READONLY))
            && (!want.export || attrs.contains(VarAttrs::EXPORT) || self.env.contains_key(name))
            && (!want.lower || attrs.contains(VarAttrs::LOWER))
            && (!want.upper || attrs.contains(VarAttrs::UPPER))
    }

    /// Plain `declare` (and `set`) listing: `name=value`, set-style quoting.
    pub(super) fn format_set_listing(&self) -> String {
        let mut out = String::new();
        for name in self.listable_names() {
            if self.scoped.namerefs.contains_key(&name) {
                if let Some(t) = self.scoped.namerefs.get(&name)
                    && !t.is_empty()
                {
                    out.push_str(&format!("{name}={}\n", set_quote(t)));
                }
                continue;
            }
            match self.var_kind(&name) {
                VarKind::Indexed => {
                    out.push_str(&format!(
                        "{name}={}\n",
                        format_indexed_body(&self.scoped.arrays[&name])
                    ));
                }
                VarKind::Assoc => {
                    out.push_str(&format!(
                        "{name}={}\n",
                        format_assoc_body(&self.scoped.assoc_arrays[&name])
                    ));
                }
                VarKind::Scalar => {
                    let v = self
                        .scoped
                        .variables
                        .get(&name)
                        .or_else(|| self.env.get(&name))
                        .cloned()
                        .unwrap_or_default();
                    out.push_str(&format!("{name}={}\n", set_quote(&v)));
                }
                VarKind::Unset => {}
            }
        }
        out
    }

    /// Resolve a nameref chain to its final target. `Err` on a cycle.
    pub(super) fn resolve_nameref_strict(&self, name: &str) -> std::result::Result<String, ()> {
        if self.scoped.namerefs.is_empty() {
            return Ok(name.to_string());
        }
        let mut current = name.to_string();
        let mut seen = std::collections::HashSet::new();
        seen.insert(current.clone());
        loop {
            // `r[1]` where r is a nameref to `a` means `a[1]`.
            let (base, sub) = match current.find('[') {
                Some(b) => (current[..b].to_string(), Some(current[b..].to_string())),
                None => (current.clone(), None),
            };
            match self.scoped.namerefs.get(&base) {
                Some(target) if !target.is_empty() => {
                    let next = match (&sub, target.find('[')) {
                        (Some(s), None) => format!("{target}{s}"),
                        _ => target.clone(),
                    };
                    if !seen.insert(next.clone()) {
                        return Err(());
                    }
                    current = next;
                }
                _ => return Ok(current),
            }
        }
    }

    /// Entry point for `declare`/`typeset`/`local`/`export`/`readonly`.
    pub(super) async fn execute_declaration_builtin(
        &mut self,
        cmd: &str,
        kind: DeclKind,
        args: &[String],
        redirects: &[Redirect],
    ) -> Result<ExecResult> {
        let compounds = std::mem::take(&mut self.pending_compound_args);
        let result = self.run_declaration(cmd, kind, args, compounds).await?;
        self.apply_redirections(result, redirects).await
    }

    async fn run_declaration(
        &mut self,
        cmd: &str,
        kind: DeclKind,
        args: &[String],
        compounds: Vec<Vec<Word>>,
    ) -> Result<ExecResult> {
        // Options: only before the first operand.
        let mut opts = DeclOpts::default();
        let mut i = 0;
        while i < args.len() {
            let a = &args[i];
            if a == "--" {
                i += 1;
                break;
            }
            let (set, rest) = if let Some(r) = a.strip_prefix('-') {
                (true, r)
            } else if let Some(r) = a.strip_prefix('+') {
                (false, r)
            } else {
                break;
            };
            if rest.is_empty() || rest.contains('=') {
                break;
            }
            for c in rest.chars() {
                if !kind.allowed_options().contains(c) {
                    let sign = if set { '-' } else { '+' };
                    return Ok(ExecResult::err(
                        format!(
                            "bash: {cmd}: {sign}{c}: invalid option\n{}",
                            kind.usage(cmd)
                        ),
                        2,
                    ));
                }
                let flags = if set { &mut opts.on } else { &mut opts.off };
                match c {
                    'a' => flags.indexed = true,
                    'A' => flags.assoc = true,
                    'i' => flags.integer = true,
                    'n' if kind == DeclKind::Export => opts.off.export = true,
                    'n' => flags.nameref = true,
                    'r' => flags.readonly = true,
                    'x' => flags.export = true,
                    'l' => flags.lower = true,
                    'u' => flags.upper = true,
                    'p' => opts.print = true,
                    'f' => opts.functions = true,
                    'F' => opts.function_names = true,
                    'g' => opts.global = true,
                    _ => {} // t, I: accepted, no effect here
                }
            }
            i += 1;
        }
        let names = &args[i..];

        if kind == DeclKind::Local && self.local_frame_index().is_none() {
            // Bash quirk: outside a function, `local` fails, but its
            // `name=(...)` operands were already assigned as globals.
            for arg in names {
                if let Some(eq) = operand_eq_pos(arg)
                    && let Some(words) = arg[eq + 1..]
                        .strip_prefix(COMPOUND_MARK)
                        .and_then(|r| r.strip_suffix(COMPOUND_MARK))
                        .and_then(|n| n.parse::<usize>().ok())
                        .and_then(|n| compounds.get(n))
                {
                    let (lhs, append) = match arg[..eq].strip_suffix('+') {
                        Some(l) => (l, true),
                        None => (&arg[..eq], false),
                    };
                    if is_valid_var_name(lhs)
                        && !is_internal_variable(lhs)
                        && !self.is_var_readonly(lhs)
                    {
                        let assoc = self.var_kind(lhs) == VarKind::Assoc;
                        let words = words.clone();
                        self.assign_array_words(lhs, &words, append, assoc).await?;
                    }
                }
            }
            return Ok(ExecResult::err(
                "bash: local: can only be used in a function\n".to_string(),
                1,
            ));
        }

        if opts.functions || opts.function_names {
            return Ok(self.declaration_functions(cmd, kind, &opts, names));
        }

        if names.is_empty() || opts.print {
            return Ok(self.declaration_listing(cmd, kind, &opts, names));
        }

        let mut stderr = String::new();
        let mut status = 0;
        for arg in names {
            if let Some(msg) = self
                .declare_operand(cmd, kind, &opts, arg, &compounds)
                .await?
            {
                stderr.push_str(&msg);
                status = 1;
            }
        }
        Ok(ExecResult {
            stderr: stderr.into(),
            exit_code: status,
            ..Default::default()
        })
    }

    fn declaration_functions(
        &self,
        cmd: &str,
        kind: DeclKind,
        opts: &DeclOpts,
        names: &[String],
    ) -> ExecResult {
        // `-F` lists names (`declare -f NAME` when listing all, the bare
        // name for an operand); `-f` prints bodies as bash's print_cmd does.
        let show = |name: &str, listing_all: bool| match self.scoped.functions.get(name) {
            Some(_) if opts.function_names && listing_all => format!("declare -f {name}\n"),
            Some(_) if opts.function_names => format!("{name}\n"),
            Some(f) => format!("{}\n", crate::parser::function_string(name, &f.body)),
            None => String::new(),
        };
        let mut out = String::new();
        let mut err = String::new();
        let mut status = 0;
        if names.is_empty() {
            if matches!(kind, DeclKind::Declare | DeclKind::Local) {
                let mut fnames: Vec<&String> = self.scoped.functions.keys().collect();
                fnames.sort();
                for f in fnames {
                    out.push_str(&show(f, true));
                }
            }
        } else {
            for name in names {
                if !self.scoped.functions.contains_key(name.as_str()) {
                    status = 1;
                    if matches!(kind, DeclKind::Export | DeclKind::Readonly) {
                        err.push_str(&format!("bash: {cmd}: {name}: not a function\n"));
                    }
                } else if matches!(kind, DeclKind::Declare | DeclKind::Local) {
                    out.push_str(&show(name, false));
                }
            }
        }
        ExecResult {
            stdout: out.into(),
            stderr: err.into(),
            exit_code: status,
            ..Default::default()
        }
    }

    fn declaration_listing(
        &self,
        cmd: &str,
        kind: DeclKind,
        opts: &DeclOpts,
        names: &[String],
    ) -> ExecResult {
        let mut out = String::new();
        if !names.is_empty() {
            let mut err = String::new();
            let mut status = 0;
            for name in names {
                match self.format_declare_line(name) {
                    Some(line) => out.push_str(&line),
                    None => {
                        err.push_str(&format!("bash: {cmd}: {name}: not found\n"));
                        status = 1;
                    }
                }
            }
            return ExecResult {
                stdout: out.into(),
                stderr: err.into(),
                exit_code: status,
                ..Default::default()
            };
        }
        let mut want = opts.on;
        match kind {
            DeclKind::Local => {
                if let Some(idx) = self.local_frame_index() {
                    let mut locals: Vec<&String> = self.call_stack[idx].saved_vars.keys().collect();
                    locals.sort();
                    for name in locals {
                        if let Some(line) = self.format_declare_line(name) {
                            out.push_str(&line);
                        }
                    }
                }
                return ExecResult::ok(out);
            }
            DeclKind::Export => {
                if opts.off.export {
                    return ExecResult::ok(String::new());
                }
                want.export = true;
            }
            DeclKind::Readonly => want.readonly = true,
            DeclKind::Declare => {
                if !opts.print && !want.any() {
                    return ExecResult::ok(self.format_set_listing());
                }
            }
        }
        for name in self.listable_names() {
            if self.has_attrs(&name, &want)
                && let Some(line) = self.format_declare_line(&name)
            {
                out.push_str(&line);
            }
        }
        ExecResult::ok(out)
    }

    /// Process one operand. `Ok(Some(msg))` is a failed operand (status 1).
    async fn declare_operand(
        &mut self,
        cmd: &str,
        kind: DeclKind,
        opts: &DeclOpts,
        arg: &str,
        compounds: &[Vec<Word>],
    ) -> Result<Option<String>> {
        // Split NAME[=VALUE] / NAME+=VALUE.
        let (lhs, value, append) = match operand_eq_pos(arg) {
            Some(eq) => {
                let lhs = &arg[..eq];
                let (lhs, append) = match lhs.strip_suffix('+') {
                    Some(l) => (l, true),
                    None => (lhs, false),
                };
                let raw = &arg[eq + 1..];
                let value = match raw
                    .strip_prefix(COMPOUND_MARK)
                    .and_then(|r| r.strip_suffix(COMPOUND_MARK))
                    .and_then(|n| n.parse::<usize>().ok())
                    .and_then(|n| compounds.get(n))
                {
                    Some(words) => DeclValue::Compound(words.clone()),
                    None => DeclValue::Str(raw.to_string()),
                };
                (lhs, Some(value), append)
            }
            None => (arg, None, false),
        };
        let display_arg = || -> String {
            match operand_eq_pos(arg) {
                Some(eq) if arg[eq + 1..].starts_with(COMPOUND_MARK) => {
                    format!("{}(...)", &arg[..=eq])
                }
                _ => arg.to_string(),
            }
        };

        // Validate the name (`name` or `name[subscript]`).
        let (base, subscript) = match lhs.find('[') {
            Some(b)
                if lhs.ends_with(']') && matches!(kind, DeclKind::Declare | DeclKind::Local) =>
            {
                (&lhs[..b], Some(lhs[b + 1..lhs.len() - 1].to_string()))
            }
            _ => (lhs, None),
        };
        if !is_valid_var_name(base) {
            return Ok(Some(format!(
                "bash: {cmd}: `{}': not a valid identifier\n",
                display_arg()
            )));
        }
        // THREAT[TM-INJ-012/014/015]: internal marker names are never declared.
        if is_internal_variable(base) {
            return Ok(None);
        }

        let in_function = self.local_frame_index().is_some();
        let make_local = !opts.global
            && match kind {
                DeclKind::Local => true,
                DeclKind::Declare => in_function,
                _ => false,
            };

        // -n: declare a name reference.
        if opts.on.nameref && kind != DeclKind::Export {
            return Ok(self.declare_nameref(cmd, base, value, make_local, opts));
        }
        // +n: turn a nameref back into a plain variable holding the target.
        if opts.off.nameref
            && let Some(target) = self.scoped.namerefs.get(base).cloned()
        {
            self.remove_nameref(base);
            self.insert_variable_checked(base.to_string(), target);
            return Ok(None);
        }

        // Act on the nameref target unless the name is being made local.
        let target = if make_local && !self.is_local_in_current_frame(base) {
            lhs.to_string()
        } else {
            match self.resolve_nameref_strict(lhs) {
                Ok(t) => t,
                Err(()) => {
                    return Ok(Some(format!(
                        "bash: {cmd}: warning: {base}: circular name reference\n"
                    )));
                }
            }
        };
        let (name, subscript) = match target.find('[') {
            Some(b) if target.ends_with(']') => (
                target[..b].to_string(),
                Some(target[b + 1..target.len() - 1].to_string()),
            ),
            _ => (target.clone(), subscript),
        };
        let name = name.as_str();

        // `declare -g x=v` under a local `x`: the global binding is the one
        // the outermost local saved.
        if opts.global
            && subscript.is_none()
            && let Some(DeclValue::Str(v)) = &value
            && let Some(idx) = self
                .call_stack
                .iter()
                .position(|f| f.saved_vars.contains_key(name))
        {
            let saved = self.call_stack[idx]
                .saved_vars
                .get(name)
                .cloned()
                .unwrap_or_default();
            if saved.attrs.is_some_and(|a| a.contains(VarAttrs::READONLY)) {
                return Ok(Some(format!("bash: {cmd}: {name}: readonly variable\n")));
            }
            let new_value = if append {
                saved.value.clone().unwrap_or_default() + v
            } else {
                v.clone()
            };
            // THREAT[TM-DOS-060]: the saved value stays charged; budget the
            // size change like any other variable write.
            let old_len = saved.value.as_ref().map_or(0, String::len);
            let is_new = saved.value.is_none();
            let old_key_len = if is_new { 0 } else { name.len() };
            if let Err(error) = self.memory_budget.check_variable_insert(
                name.len(),
                new_value.len(),
                is_new,
                old_key_len,
                old_len,
                &self.memory_limits,
            ) {
                self.memory_limit_error.get_or_insert(error);
                return Ok(None);
            }
            self.memory_budget.record_variable_insert(
                name.len(),
                new_value.len(),
                is_new,
                old_key_len,
                old_len,
            );
            if let Some(slot) = self.call_stack[idx].saved_vars.get_mut(name) {
                slot.value = Some(new_value);
            }
            return Ok(None);
        }

        if make_local && !self.is_local_in_current_frame(name) {
            if self.is_var_readonly(name) {
                return Ok(Some(format!("bash: {cmd}: {name}: readonly variable\n")));
            }
            self.make_local(name);
        }

        // THREAT[TM-INJ-019/020/021]: refuse to change readonly variables and
        // surface the error so callers cannot mistake a silent skip for success.
        if self.is_var_readonly(name)
            && (value.is_some() || opts.off.readonly || opts.off.export && kind == DeclKind::Export)
        {
            return Ok(Some(format!("bash: {cmd}: {name}: readonly variable\n")));
        }

        let is_compound = matches!(value, Some(DeclValue::Compound(_)));
        let (mut want_indexed, mut want_assoc) = (opts.on.indexed, opts.on.assoc);
        if kind == DeclKind::Readonly && !is_compound {
            // `readonly -a/-A` only shapes a compound value.
            want_indexed = false;
            want_assoc = false;
        }
        if subscript.is_some() && !want_assoc && self.var_kind(name) != VarKind::Assoc {
            want_indexed = true;
        }
        let kind_now = self.var_kind(name);
        let conversion = if want_assoc && kind_now == VarKind::Indexed {
            Some("indexed to associative")
        } else if want_indexed && kind_now == VarKind::Assoc {
            Some("associative to indexed")
        } else {
            None
        };
        if let Some(what) = conversion {
            let msg = format!("bash: {cmd}: {name}: cannot convert {what} array\n");
            if is_compound {
                // With an array value bash abandons the whole command line.
                return Err(crate::error::Error::LineAbort(msg));
            }
            return Ok(Some(msg));
        }

        // Shape: create or convert to the requested array type.
        if want_assoc && kind_now != VarKind::Assoc {
            let mut arr = HashMap::new();
            if let Some(v) = self.remove_scalar_value(name) {
                arr.insert("0".to_string(), v);
            }
            let empty = arr.is_empty();
            self.insert_assoc_array_checked(name.to_string(), arr);
            if empty && value.is_none() {
                self.add_var_attr(name, VarAttrs::NOVALUE);
            }
        } else if want_indexed && !matches!(kind_now, VarKind::Indexed | VarKind::Assoc) {
            let mut arr = HashMap::new();
            if let Some(v) = self.remove_scalar_value(name) {
                arr.insert(0, v);
            }
            let empty = arr.is_empty();
            self.insert_array_checked(name.to_string(), arr);
            if empty && value.is_none() {
                self.add_var_attr(name, VarAttrs::NOVALUE);
            }
        } else if kind_now == VarKind::Unset
            && value.is_none()
            && !self.scoped.var_attrs.contains_key(name)
            && !self.is_local_in_current_frame(name)
        {
            self.add_var_attr(name, VarAttrs::NOVALUE);
        }

        // Attributes that shape the assigned value go on first.
        self.apply_declared_attrs(name, kind, opts);

        match value {
            None => {}
            Some(DeclValue::Compound(words)) => {
                if subscript.is_some() {
                    return Ok(Some(format!(
                        "bash: {cmd}: {name}: cannot assign list to array member\n"
                    )));
                }
                let assoc = self.var_kind(name) == VarKind::Assoc;
                self.assign_array_words(name, &words, append, assoc).await?;
            }
            Some(DeclValue::Str(v)) => {
                if let Some(sub) = subscript {
                    self.assign_element(name, &sub, v, append).await?;
                } else if (opts.on.indexed || opts.on.assoc)
                    && v.starts_with('(')
                    && v.ends_with(')')
                    && kind != DeclKind::Readonly
                {
                    // `declare -a x='(a b)'` re-parses the string as a list.
                    let words = self.parse_compound_text(name, &v)?;
                    let assoc = self.var_kind(name) == VarKind::Assoc;
                    self.assign_array_words(name, &words, append, assoc).await?;
                } else if append {
                    self.append_scalar(name, v);
                } else {
                    self.set_variable(name.to_string(), v);
                }
            }
        }

        // Readonly last, so the value above could still be written.
        if opts.on.readonly || kind == DeclKind::Readonly {
            self.add_var_attr(name, VarAttrs::READONLY);
        }
        if self.var_attrs_get(name).contains(VarAttrs::EXPORT)
            && let Some(v) = self.scoped.variables.get(name).cloned()
        {
            self.insert_env_checked(name.to_string(), v);
        }
        Ok(None)
    }

    /// Apply `-i -l -u -x` / `+i +l +u +x` to `name`.
    fn apply_declared_attrs(&mut self, name: &str, kind: DeclKind, opts: &DeclOpts) {
        if opts.on.integer {
            self.add_var_attr(name, VarAttrs::INTEGER);
        }
        if opts.off.integer {
            self.remove_var_attr(name, VarAttrs::INTEGER);
        }
        // -l and -u replace each other; given together neither applies.
        if opts.on.lower && opts.on.upper {
            self.remove_var_attr(name, VarAttrs::LOWER);
            self.remove_var_attr(name, VarAttrs::UPPER);
        } else if opts.on.lower {
            self.add_var_attr(name, VarAttrs::LOWER);
            self.remove_var_attr(name, VarAttrs::UPPER);
        } else if opts.on.upper {
            self.add_var_attr(name, VarAttrs::UPPER);
            self.remove_var_attr(name, VarAttrs::LOWER);
        }
        if opts.off.lower {
            self.remove_var_attr(name, VarAttrs::LOWER);
        }
        if opts.off.upper {
            self.remove_var_attr(name, VarAttrs::UPPER);
        }
        if (opts.on.export || kind == DeclKind::Export) && !opts.off.export {
            self.add_var_attr(name, VarAttrs::EXPORT);
        }
        if opts.off.export {
            // Keep the value as a shell variable, drop it from the environment.
            if !self.scoped.variables.contains_key(name)
                && let Some(v) = self.env.get(name).cloned()
            {
                self.insert_variable_checked(name.to_string(), v);
            }
            self.remove_var_attr(name, VarAttrs::EXPORT);
            self.env_mut().remove(name);
        }
    }

    /// `declare -n name[=target]`.
    fn declare_nameref(
        &mut self,
        cmd: &str,
        name: &str,
        value: Option<DeclValue>,
        make_local: bool,
        opts: &DeclOpts,
    ) -> Option<String> {
        let target = match value {
            Some(DeclValue::Str(t)) => Some(t),
            Some(DeclValue::Compound(_)) => {
                return Some(format!(
                    "bash: {cmd}: {name}: reference variable cannot be an array\n"
                ));
            }
            None => None,
        };
        if let Some(t) = &target {
            if !valid_nameref_target(t) {
                return Some(format!(
                    "bash: {cmd}: `{t}': invalid variable name for name reference\n"
                ));
            }
            if t == name && !make_local {
                return Some(format!(
                    "bash: {cmd}: {name}: nameref variable self references not allowed\n"
                ));
            }
        }
        if make_local && !self.is_local_in_current_frame(name) {
            if self.is_var_readonly(name) {
                return Some(format!("bash: {cmd}: {name}: readonly variable\n"));
            }
            self.make_local(name);
        }
        if target.is_some() && self.is_var_readonly(name) {
            return Some(format!("bash: {cmd}: {name}: readonly variable\n"));
        }
        let target = match target {
            Some(t) => t,
            None => match self.scoped.namerefs.get(name) {
                Some(existing) => existing.clone(),
                None => match self.scoped.variables.get(name).cloned() {
                    // An existing value becomes the target when it is a name.
                    Some(v) if valid_nameref_target(&v) && v != name => {
                        self.remove_scalar_value(name);
                        v
                    }
                    Some(v) => {
                        return Some(format!(
                            "bash: {cmd}: `{v}': invalid variable name for name reference\n"
                        ));
                    }
                    None => String::new(),
                },
            },
        };
        self.remove_scalar_value(name);
        self.remove_var_attr(name, VarAttrs::NOVALUE);
        self.set_nameref(name, target);
        if opts.on.readonly {
            self.add_var_attr(name, VarAttrs::READONLY);
        }
        None
    }

    /// `name+=value` on a scalar (or element 0 of an array).
    pub(super) fn append_scalar(&mut self, name: &str, value: String) {
        let existing = self.expand_variable(name);
        if self.var_attrs_get(name).contains(VarAttrs::INTEGER) {
            let base = if existing.is_empty() {
                "0".to_string()
            } else {
                existing
            };
            self.set_variable(name.to_string(), format!("{base}+({value})"));
        } else {
            self.set_variable(name.to_string(), existing + &value);
        }
    }

    /// Apply the integer / case attributes of array `name` to an element
    /// value; `old` is the element being appended to.
    fn transform_element(&mut self, attrs: VarAttrs, old: Option<&str>, value: String) -> String {
        if attrs.contains(VarAttrs::INTEGER) {
            let expr = match old {
                Some(o) if !o.is_empty() => format!("{o}+({value})"),
                _ => value,
            };
            return self.evaluate_arithmetic_with_assign(&expr).to_string();
        }
        let value = match old {
            Some(o) => format!("{o}{value}"),
            None => value,
        };
        if attrs.contains(VarAttrs::LOWER) {
            value.to_lowercase()
        } else if attrs.contains(VarAttrs::UPPER) {
            value.to_uppercase()
        } else {
            value
        }
    }

    /// `name[sub]=value` / `name[sub]+=value` with the array's attributes.
    pub(super) async fn assign_element(
        &mut self,
        name: &str,
        sub: &str,
        value: String,
        append: bool,
    ) -> Result<()> {
        let attrs = self.var_attrs_get(name);
        if self.var_kind(name) == VarKind::Assoc {
            let key = self.expand_assoc_key(sub).await?;
            let old = if append {
                self.scoped
                    .assoc_arrays
                    .get(name)
                    .and_then(|a| a.get(&key))
                    .cloned()
            } else {
                None
            };
            let v = self.transform_element(attrs, old.as_deref(), value);
            self.set_assoc_element_checked(name.to_string(), key, v);
        } else {
            self.promote_scalar_to_indexed(name);
            let idx = self.resolve_indexed_array_subscript(name, sub);
            let old = if append {
                Some(
                    self.scoped
                        .arrays
                        .get(name)
                        .and_then(|a| a.get(&idx))
                        .cloned()
                        .unwrap_or_default(),
                )
            } else {
                None
            };
            let v = self.transform_element(attrs, old.as_deref(), value);
            self.set_indexed_element_checked(name, idx, v);
        }
        self.remove_var_attr(name, VarAttrs::NOVALUE);
        Ok(())
    }

    /// Budgeted write of one indexed element.
    pub(super) fn set_indexed_element_checked(&mut self, name: &str, idx: usize, value: String) {
        let old_len = self
            .scoped
            .arrays
            .get(name)
            .and_then(|a| a.get(&idx))
            .map(String::len);
        let is_new = old_len.is_none();
        if !self.admit_array_write(usize::from(is_new), value.len(), old_len.unwrap_or(0)) {
            return;
        }
        self.arrays_mut()
            .entry(name.to_string())
            .or_default()
            .insert(idx, value);
    }

    /// A scalar written through a subscript becomes element 0 of an array.
    pub(super) fn promote_scalar_to_indexed(&mut self, name: &str) {
        if self.scoped.arrays.contains_key(name) || self.scoped.assoc_arrays.contains_key(name) {
            return;
        }
        let Some(v) = self.remove_scalar_value(name) else {
            return;
        };
        let mut arr = HashMap::new();
        arr.insert(0, v);
        self.insert_array_checked(name.to_string(), arr);
    }

    /// Parse `(...)` text into element words (`declare -a x='(a b)'`).
    fn parse_compound_text(&self, name: &str, text: &str) -> Result<Vec<Word>> {
        let src = format!("{name}={text}");
        let parser = Parser::with_limits(
            &src,
            self.limits.max_ast_depth,
            self.limits.max_parser_operations,
        )
        .with_execution_budget(self.execution_budget.clone());
        let script = parser.parse()?;
        for cmd in &script.commands {
            if let Command::Simple(simple) = cmd
                && let Some(a) = simple.assignments.first()
                && let AssignmentValue::Array(words) = &a.value
            {
                return Ok(words.clone());
            }
        }
        Ok(Vec::new())
    }

    /// Expand compound-array element words and store them in `name`
    /// (`a=(x [3]=y)`, `m=([k]=v)`, `m=(k1 v1 k2 v2)`). Shared by
    /// assignment statements and the declaration builtins.
    pub(super) async fn assign_array_words(
        &mut self,
        name: &str,
        words: &[Word],
        append: bool,
        assoc: bool,
    ) -> Result<()> {
        let attrs = self.var_attrs_get(name);
        if assoc {
            let mut map: HashMap<String, String> = if append {
                self.scoped
                    .assoc_arrays
                    .get(name)
                    .cloned()
                    .unwrap_or_default()
            } else {
                HashMap::new()
            };
            let old_entries = self.scoped.assoc_arrays.get(name).map_or(0, |a| a.len());
            let max_entries = old_entries.saturating_add(
                self.memory_limits
                    .max_array_entries
                    .saturating_sub(self.memory_budget.array_entries),
            );
            let mut pending_key: Option<String> = None;
            for word in words {
                if map.len() >= max_entries {
                    break;
                }
                if let Some((kw, vw, kappend)) = split_keyed_word(word) {
                    let key = self.expand_word(&kw).await?;
                    let val = self.expand_word(&vw).await?;
                    let old = if kappend {
                        Some(map.get(&key).cloned().unwrap_or_default())
                    } else {
                        None
                    };
                    let v = self.transform_element(attrs, old.as_deref(), val);
                    map.insert(key, v);
                    continue;
                }
                let remaining = max_entries.saturating_sub(map.len()).saturating_mul(2);
                for field in self.expand_element_fields(word, remaining).await? {
                    match pending_key.take() {
                        Some(k) => {
                            let v = self.transform_element(attrs, None, field);
                            map.insert(k, v);
                        }
                        None => pending_key = Some(field),
                    }
                }
            }
            if let Some(k) = pending_key {
                map.insert(k, String::new());
            }
            self.remove_scalar_value(name);
            self.arrays_mut().remove(name);
            self.insert_assoc_array_checked(name.to_string(), map);
        } else {
            let mut map: HashMap<usize, String> = if append {
                self.promote_scalar_to_indexed(name);
                self.scoped.arrays.get(name).cloned().unwrap_or_default()
            } else {
                HashMap::new()
            };
            let old_entries = self.scoped.arrays.get(name).map_or(0, |a| a.len());
            let max_entries = old_entries.saturating_add(
                self.memory_limits
                    .max_array_entries
                    .saturating_sub(self.memory_budget.array_entries),
            );
            let mut next = map.keys().max().map_or(0, |k| k + 1);
            'words: for word in words {
                // Bash brace-expands indexed elements before spotting
                // `[i]=`, so `([2]=v{1,2})` stores the plain words
                // `[2]=v1 [2]=v2`; assoc elements never brace-expand.
                let braces = self.brace_expand_word(word).is_some();
                if let Some((kw, vw, kappend)) = split_keyed_word(word).filter(|_| !braces) {
                    let key_text = self.expand_word(&kw).await?;
                    let raw = match self.try_evaluate_arithmetic_with_assign(&key_text) {
                        Ok(v) => v,
                        Err(msg) => {
                            return Err(crate::error::Error::LineAbort(self.arith_diag("", &msg)));
                        }
                    };
                    let idx = if raw < 0 {
                        let len = map.keys().max().map_or(0, |m| m + 1) as i64;
                        let i = len + raw;
                        if i < 0 {
                            return Err(crate::error::Error::LineAbort(format!(
                                "bash: {name}[{key_text}]: bad array subscript\n"
                            )));
                        }
                        i as usize
                    } else {
                        raw as usize
                    };
                    let val = self.expand_word(&vw).await?;
                    let old = if kappend {
                        Some(map.get(&idx).cloned().unwrap_or_default())
                    } else {
                        None
                    };
                    let v = self.transform_element(attrs, old.as_deref(), val);
                    if map.len() >= max_entries && !map.contains_key(&idx) {
                        break;
                    }
                    map.insert(idx, v);
                    next = idx + 1;
                    continue;
                }
                let remaining = max_entries.saturating_sub(map.len());
                if remaining == 0 {
                    break;
                }
                for field in self.expand_element_fields(word, remaining).await? {
                    if map.len() >= max_entries {
                        break 'words;
                    }
                    let v = self.transform_element(attrs, None, field);
                    map.insert(next, v);
                    next += 1;
                }
            }
            self.remove_scalar_value(name);
            self.assoc_arrays_mut().remove(name);
            self.insert_array_checked(name.to_string(), map);
        }
        self.remove_var_attr(name, VarAttrs::NOVALUE);
        Ok(())
    }

    /// Expand one unkeyed compound element into fields: word splitting for
    /// unquoted expansions (bounded by `limit`), `"${a[@]}"` splats, brace
    /// expansion and globbing for unquoted literals.
    async fn expand_element_fields(&mut self, word: &Word, limit: usize) -> Result<Vec<String>> {
        // Brace expansion runs first on the unexpanded word, as for command
        // arguments: `a=("x"{1,2})` gives `x1 x2`, `a=('{a,b}')` stays literal.
        let Some(braced) = self.brace_expand_word(word) else {
            return self.expand_element_word(word, limit).await;
        };
        let mut fields = Vec::new();
        for w in &braced {
            let left = limit.saturating_sub(fields.len());
            if left == 0 {
                break;
            }
            fields.extend(self.expand_element_word(w, left).await?);
        }
        fields.truncate(limit);
        Ok(fields)
    }

    /// One brace-expanded compound element: splitting, splats, then globbing
    /// for words with unquoted literal text.
    async fn expand_element_word(&mut self, word: &Word, limit: usize) -> Result<Vec<String>> {
        let is_unquoted_expansion = !word.quoted
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
        let is_splat = word.quoted
            && word.parts.len() == 1
            && matches!(
                &word.parts[0],
                WordPart::ArrayAccess { index, .. } if index == "@"
            )
            || word.quoted
                && word.parts.len() == 1
                && matches!(&word.parts[0], WordPart::Variable(n) if n == "@");
        if is_unquoted_expansion {
            // Split fields are pathname-expanded like command words:
            // `p='*.txt'; a=($p)` holds the matching files.
            let expanded = self.expand_word(word).await?;
            let mut fields = Vec::new();
            for field in self.ifs_split_limited(&expanded, limit)? {
                match self.expand_glob_item(&field, false).await {
                    Ok(items) => fields.extend(items),
                    Err(_) => fields.push(field),
                }
                if fields.len() >= limit {
                    break;
                }
            }
            fields.truncate(limit);
            return Ok(fields);
        }
        if is_splat {
            return self.expand_word_to_fields(word).await;
        }
        let value = self.expand_word(word).await?;
        let literal_only = word.parts.iter().all(|p| matches!(p, WordPart::Literal(_)));
        let globbable = if word.quoted {
            word.has_unquoted_glob
        } else {
            literal_only
        };
        if !globbable {
            return Ok(vec![value]);
        }
        let mut fields = match self
            .expand_glob_item(&value, word.quoted && word.has_unquoted_glob)
            .await
        {
            Ok(items) => items,
            Err(_) => vec![value],
        };
        fields.truncate(limit);
        Ok(fields)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn quoting_matches_bash() {
        assert_eq!(declare_quote("a\"b$c`d\\e"), "\"a\\\"b\\$c\\`d\\\\e\"");
        assert_eq!(declare_quote("x\ny"), "$'x\\ny'");
        assert_eq!(ansic_quote("a\x01\x7f'"), "$'a\\001\\177\\''");
        assert_eq!(set_quote("abc"), "abc");
        assert_eq!(set_quote("a b"), "'a b'");
        assert_eq!(set_quote("it's"), "'it'\\''s'");
        assert_eq!(set_quote("a\tb c"), "$'a\\tb c'");
    }

    #[test]
    fn assoc_order_follows_bash_hash_buckets() {
        let m: HashMap<String, String> = ["k", "j", "a", "b", "zz", "x1"]
            .iter()
            .map(|k| (k.to_string(), String::new()))
            .collect();
        let keys: Vec<&str> = assoc_keys_bash_order(&m)
            .into_iter()
            .map(String::as_str)
            .collect();
        assert_eq!(keys, ["x1", "k", "j", "b", "a", "zz"]);
    }

    #[test]
    fn keyed_elements_split_into_key_and_value() {
        let script = Parser::new(r#"a=([k]="hello world" [i+1]+=x plain)"#)
            .parse()
            .unwrap();
        let Command::Simple(cmd) = &script.commands[0] else {
            panic!("expected a simple command");
        };
        let AssignmentValue::Array(words) = &cmd.assignments[0].value else {
            panic!("expected an array assignment");
        };
        assert_eq!(words.len(), 3);
        let (k, v, append) = split_keyed_word(&words[0]).expect("keyed");
        assert_eq!(
            (k.to_string(), v.to_string(), append),
            ("k".into(), "hello world".into(), false)
        );
        let (k, _, append) = split_keyed_word(&words[1]).expect("keyed");
        assert_eq!((k.to_string(), append), ("i+1".into(), true));
        assert!(split_keyed_word(&words[2]).is_none());
    }

    #[test]
    fn nameref_targets() {
        assert!(valid_nameref_target("a"));
        assert!(valid_nameref_target("a[1]"));
        assert!(!valid_nameref_target("1x"));
        assert!(!valid_nameref_target("@"));
        assert!(!valid_nameref_target("a b"));
    }
}
