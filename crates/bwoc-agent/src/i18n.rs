//! Localized output for `bwoc-agent` via [Project Fluent].
//!
//! Mirrors `crates/bwoc-cli/src/i18n.rs` (deliberately duplicated for now —
//! two small siblings with no current API drift beat a premature bwoc-core
//! extraction, per Mattaññutā). If/when the modules drift, promote to
//! `bwoc-core::i18n` and have both crates re-export.
//!
//! [Project Fluent]: https://projectfluent.org/

use std::borrow::Cow;

use fluent_bundle::{FluentArgs, FluentBundle, FluentResource, FluentValue};
use unic_langid::LanguageIdentifier;

const EN_AGENT_FTL: &str = include_str!("../locales/en/agent.ftl");
const TH_AGENT_FTL: &str = include_str!("../locales/th/agent.ftl");

/// Build a Fluent bundle for `lang`. Unsupported codes fall back to `en`.
pub fn bundle_for(lang: &str) -> FluentBundle<FluentResource> {
    let (ftl, resolved) = match lang {
        "th" => (TH_AGENT_FTL, "th"),
        _ => (EN_AGENT_FTL, "en"),
    };
    let resource =
        FluentResource::try_new(ftl.to_string()).expect("embedded .ftl is valid at compile time");
    let langid: LanguageIdentifier = resolved.parse().expect("static langid string parses");
    let mut bundle = FluentBundle::new(vec![langid]);
    bundle.set_use_isolating(false);
    bundle
        .add_resource(resource)
        .expect("embedded .ftl is consistent at compile time");
    bundle
}

/// Look up a message by key with named string arguments. Returns a visible
/// `«missing key: ...»` placeholder on failure. Slice-of-tuples shape
/// keeps call sites ergonomic without exposing `FluentArgs` directly.
pub fn t_with(bundle: &FluentBundle<FluentResource>, key: &str, args: &[(&str, &str)]) -> String {
    let mut fargs = FluentArgs::new();
    for (name, value) in args {
        fargs.set(*name, FluentValue::from(*value));
    }
    t_inner(bundle, key, Some(&fargs))
}

fn t_inner(bundle: &FluentBundle<FluentResource>, key: &str, args: Option<&FluentArgs>) -> String {
    let Some(msg) = bundle.get_message(key) else {
        return format!("«missing key: {key}»");
    };
    let Some(pattern) = msg.value() else {
        return format!("«no value for key: {key}»");
    };
    let mut errors = vec![];
    let cow: Cow<str> = bundle.format_pattern(pattern, args, &mut errors);
    cow.into_owned()
}

/// Lang resolution chain: `BWOC_LANG` env > `LANG` env (parsed) > "en".
/// Matches bwoc-cli's `resolve_lang` (minus the `--lang` flag, since
/// bwoc-agent doesn't take CLI args).
pub fn resolve_lang() -> String {
    if let Ok(v) = std::env::var("BWOC_LANG") {
        if !v.is_empty() {
            return v;
        }
    }
    if let Ok(raw) = std::env::var("LANG") {
        if let Some(tag) = raw.split(['.', '@']).next() {
            if let Some(lang) = tag.split('_').next() {
                if !lang.is_empty() {
                    return lang.to_ascii_lowercase();
                }
            }
        }
    }
    "en".to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn en_bundle_returns_liveness_alive() {
        let b = bundle_for("en");
        let s = t_with(&b, "liveness-alive", &[("agent_id", "demo")]);
        assert!(s.contains("I am alive"), "got: {s:?}");
        assert!(s.contains("demo"), "got: {s:?}");
    }

    #[test]
    fn th_bundle_returns_localized_alive() {
        let b = bundle_for("th");
        let s = t_with(&b, "liveness-alive", &[("agent_id", "demo")]);
        assert!(s.contains("ฉันยังมีชีวิตอยู่"), "got: {s:?}");
        assert!(s.contains("demo"), "got: {s:?}");
    }

    #[test]
    fn missing_key_returns_visible_marker() {
        // The documented user-visible fallback: an unknown key renders as a
        // `«missing key: …»` marker instead of panicking or returning empty.
        let b = bundle_for("en");
        let s = t_with(&b, "bwoc-no-such-key", &[]);
        assert!(s.contains("missing key"), "got: {s:?}");
        assert!(s.contains("bwoc-no-such-key"), "got: {s:?}");
    }

    #[test]
    fn unknown_lang_falls_back_to_en() {
        let b = bundle_for("zz");
        let s = t_with(&b, "liveness-alive", &[("agent_id", "demo")]);
        assert!(s.contains("I am alive"), "got: {s:?}");
    }
}
