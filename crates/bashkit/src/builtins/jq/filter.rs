//! Compile jq filters and cache small inline programs within one request.

use std::sync::{Arc, Mutex, OnceLock};

use jaq_core::Compiler;
use jaq_core::load::{Arena, File, Loader};
use jaq_std::input::HasInputs;

use super::compat::{
    ARGS_VAR_NAME, ENV_VAR_NAME, FILENAME_VAR_NAME, JQ_COMPAT_DEFS, LINENO_VAR_NAME,
    PUBLIC_ENV_VAR_NAME, build_global_defs,
};
use super::errors::{format_compile_errors, format_load_errors};
use super::{InputData, Val, altpat, jaq_json, loc, messages, regex_compat};

pub(super) type CompiledFilter = jaq_core::Filter<InputData<Val>>;

const MAX_CACHED_FILTERS: usize = 4;

/// Retains up to four compiled plans per execution. The most recently used
/// entry is last. Keys contain inline source and ordered binding names.
#[derive(Default)]
pub(crate) struct JqFilterCache(Mutex<Vec<CachedFilter>>);

struct CachedFilter {
    source: String,
    var_names: Vec<String>,
    filter: Arc<CompiledFilter>,
}

impl CachedFilter {
    fn matches(&self, source: &str, var_names: &[&str]) -> bool {
        self.source == source && self.var_names == var_names
    }
}

impl JqFilterCache {
    pub(super) fn accepts(source: &str, var_names: &[&str]) -> bool {
        // Bound retained user-controlled source and bindings per entry.
        source.len() <= 1024
            && var_names.len() <= 16
            && var_names.iter().map(|name| name.len()).sum::<usize>() <= 1024
    }

    pub(super) fn get(&self, source: &str, var_names: &[&str]) -> Option<Arc<CompiledFilter>> {
        let mut entries = self.0.lock().ok()?;
        let index = entries
            .iter()
            .rposition(|entry| entry.matches(source, var_names))?;
        let filter = Arc::clone(&entries[index].filter);
        entries[index..].rotate_left(1);
        Some(filter)
    }

    pub(super) fn insert(&self, source: &str, var_names: &[&str], filter: &Arc<CompiledFilter>) {
        let entry = CachedFilter {
            source: source.to_owned(),
            var_names: var_names.iter().map(|name| (*name).to_owned()).collect(),
            filter: Arc::clone(filter),
        };
        let Ok(mut entries) = self.0.lock() else {
            return;
        };
        // Concurrent misses for the same key occupy a single slot.
        let evicted = if let Some(index) = entries.iter().position(|e| e.matches(source, var_names))
        {
            Some(entries.remove(index))
        } else if entries.len() == MAX_CACHED_FILTERS {
            Some(entries.remove(0))
        } else {
            None
        };
        entries.push(entry);
        // Retire the compiled graph after other calls can look up their keys.
        drop(entries);
        drop(evicted);
    }
}

pub(super) fn compile(
    filter_text: &str,
    var_names: &[&str],
) -> std::result::Result<CompiledFilter, String> {
    let prefix = build_global_defs();
    let filter_text = loc::expand_loc(filter_text);
    let filter_text = altpat::expand_alternatives(&filter_text);
    let compat_filter = format!("{prefix}\n{filter_text}");
    let filter_src = compat_filter.as_str();

    // Only the library's immutable, trusted definitions live across requests.
    // The loader still owns and resolves modules separately for every filter.
    static PRELUDE: OnceLock<Vec<jaq_core::load::parse::Def<&'static str>>> = OnceLock::new();
    let defs = PRELUDE.get_or_init(|| {
        let compat = jaq_core::load::parse(JQ_COMPAT_DEFS, |parser| parser.defs())
            .expect("embedded jq compatibility definitions must parse");
        jaq_core::defs()
            .chain(jaq_std::defs())
            .chain(jaq_json::defs())
            .chain(compat)
            .collect()
    });
    let loader = Loader::new(defs.iter().cloned());
    let arena = Arena::default();

    let program = File {
        code: filter_src,
        path: (),
    };

    let modules = match loader.load(&arena, program) {
        Ok(m) => m,
        Err(errs) => return Err(format_load_errors(errs)),
    };

    // Names of all globals: --arg/--argjson/--slurpfile/--rawfile, then the
    // four internal ones (env, ENV, filename, lineno), then $ARGS.
    let mut var_names = var_names.to_vec();
    var_names.push(ENV_VAR_NAME);
    var_names.push(PUBLIC_ENV_VAR_NAME);
    var_names.push(FILENAME_VAR_NAME);
    var_names.push(LINENO_VAR_NAME);
    var_names.push(ARGS_VAR_NAME);

    type D = InputData<Val>;
    // jq's `input` fails with "No more inputs" when the stream is
    // exhausted; jaq-std's yields nothing.
    let input_run: jaq_core::RunPtr<D> = |cv| {
        let next = cv.0.data().inputs().next();
        Box::new(std::iter::once(match next {
            Some(r) => r.map_err(|e| jaq_core::Exn::from(jaq_core::Error::str(e))),
            None => Err(jaq_core::Exn::from(jaq_core::Error::str("No more inputs"))),
        }))
    };
    let input_funs: Vec<jaq_core::native::Fun<D>> = jaq_std::input::funs::<D>()
        .into_vec()
        .into_iter()
        .filter(|(name, _, _)| *name != "input")
        .chain(std::iter::once((
            "input",
            jaq_core::native::v(0),
            input_run,
        )))
        .map(|(name, arity, run)| (name, arity, jaq_core::Native::<D>::new(run)))
        .collect();
    // Replace jaq-std's `regex`-crate-backed natives (matches/split_matches/
    // split_) with our fancy-regex backed versions so patterns with
    // lookahead/lookbehind/atomic groups/backrefs work.
    let regex_funs: Vec<jaq_core::native::Fun<D>> = regex_compat::funs::<D>()
        .into_vec()
        .into_iter()
        .map(|(name, arity, run)| (name, arity, jaq_core::Native::<D>::new(run)))
        .collect();
    // SECURITY (TM-INF-023, #1571): jaq-std's `halt` native only raises
    // `Exn::halt`; it is `jaq_core::unwrap_valr` that turns that into
    // `std::process::exit`. The evaluation loop never calls `unwrap_valr`:
    // it ends the jq command with the halt code instead, so `halt` and
    // `halt_error` behave like jq without touching the host process.
    //
    // jaq-std's `stderr_empty`/`debug_empty` write to the host (`log`);
    // ours buffer the text for this command's stderr (see `messages`).
    let message_funs: Vec<jaq_core::native::Fun<D>> = vec![
        (
            "stderr_empty",
            jaq_core::native::v(0),
            jaq_core::Native::<D>::new(|cv| {
                messages::stderr(&cv.1);
                Box::new(std::iter::empty())
            }),
        ),
        // jq accepts base64 with non-zero trailing bits ("YW" is "a").
        (
            "decode_base64",
            jaq_core::native::v(0),
            jaq_core::Native::<D>::new(|cv| {
                use base64::Engine;
                use base64::engine::{DecodePaddingMode, GeneralPurpose, GeneralPurposeConfig};
                use jaq_std::ValT;
                const LENIENT: GeneralPurpose = GeneralPurpose::new(
                    &base64::alphabet::STANDARD,
                    GeneralPurposeConfig::new()
                        .with_decode_allow_trailing_bits(true)
                        .with_decode_padding_mode(DecodePaddingMode::Indifferent),
                );
                jaq_core::native::bome(cv.1.try_as_utf8_bytes().and_then(|s| {
                    LENIENT
                        .decode(s)
                        .map_err(|e| jaq_core::Error::str(e.to_string()))
                        .map(Val::from_utf8_bytes)
                }))
            }),
        ),
        (
            "debug_empty",
            jaq_core::native::v(0),
            jaq_core::Native::<D>::new(|cv| {
                messages::debug(&cv.1);
                Box::new(std::iter::empty())
            }),
        ),
    ];
    let native_funs = jaq_core::funs::<D>()
        .chain(jaq_std::funs::<D>().filter(|(name, _, _)| {
            !matches!(
                *name,
                "env" | "stderr_empty" | "debug_empty" | "decode_base64"
            ) && !regex_compat::SHADOWED_NATIVE_NAMES.contains(name)
        }))
        .chain(message_funs)
        .chain(input_funs)
        .chain(regex_funs)
        .chain(jaq_json::funs::<D>());

    let compiler = Compiler::default()
        .with_funs(native_funs)
        .with_global_vars(var_names.iter().copied());

    compiler.compile(modules).map_err(format_compile_errors)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cache_keys_on_source_and_binding_order() {
        let cache = JqFilterCache::default();
        let source = "$left, $right";
        let names = ["$left", "$right"];
        let compiled = Arc::new(compile(source, &names).unwrap());
        cache.insert(source, &names, &compiled);

        assert!(Arc::ptr_eq(&cache.get(source, &names).unwrap(), &compiled));
        assert!(cache.get(source, &["$right", "$left"]).is_none());
        assert!(cache.get(source, &["$left"]).is_none());
        assert!(cache.get("$right, $left", &names).is_none());

        let other = Arc::new(compile(".", &[]).unwrap());
        cache.insert(".", &[], &other);
        assert!(Arc::ptr_eq(&cache.get(source, &names).unwrap(), &compiled));
        assert!(Arc::ptr_eq(&cache.get(".", &[]).unwrap(), &other));
    }

    #[test]
    fn cache_hits_and_replacements_update_eviction_order() {
        let cache = JqFilterCache::default();
        let filter = Arc::new(CompiledFilter::default());
        for index in 0..MAX_CACHED_FILTERS {
            cache.insert(&index.to_string(), &[], &filter);
        }

        assert!(cache.get("0", &[]).is_some());
        let replacement = Arc::new(CompiledFilter::default());
        let last_key = (MAX_CACHED_FILTERS - 1).to_string();
        cache.insert(&last_key, &[], &replacement);
        assert!(cache.get("1", &[]).is_some());

        cache.insert("new", &[], &filter);
        assert!(cache.get("2", &[]).is_none());
        assert!(cache.get("0", &[]).is_some());
        assert!(Arc::ptr_eq(
            &cache.get(&last_key, &[]).unwrap(),
            &replacement
        ));
    }

    #[test]
    fn cache_admits_only_small_inline_programs_and_binding_names() {
        assert!(JqFilterCache::accepts(".", &["$item"]));
        assert!(!JqFilterCache::accepts(&"x".repeat(1025), &[]));
        assert!(!JqFilterCache::accepts(".", &["$item"; 17]));
        assert!(!JqFilterCache::accepts(".", &[&"x".repeat(1025)]));
    }
}
