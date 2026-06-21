---
name: check-bilingual
description: Verify translation parity for BWOC docs (English is canonical). Reports any docs/en/*.en.md without a matching translation in each present docs/<lang>/ (th today, any BCP 47 / ISO 639-1 language in future), and — inside a git repo — any EN file changed in the current diff whose translations were not also changed. Use when the user says "check bilingual", "parity check", "EN TH", "check translations", or before a PR touching docs.
---

# /check-bilingual — translation-parity audit

Read-only. English is canonical; translations are `<NAME>.<lang>.md` under
`docs/<lang>/` (lowercase BCP 47 / ISO 639-1 — see `NAMING.en.md`). Despite the
name, this is **language-agnostic**: it checks every `docs/<lang>/` that exists,
not only TH. Covers both the framework root `docs/` and
`modules/agent-template/docs/`.

## Steps

1. **Existence pass.** For every **present** non-EN language directory, check
   that each `docs/en/*.en.md` has a matching `docs/<lang>/<NAME>.<lang>.md`.
   Report missing counterparts as FAIL.

   ```bash
   for base in docs modules/agent-template/docs; do
     [ -d "$base/en" ] || continue
     for langdir in "$base"/*/; do
       [ -d "$langdir" ] || continue   # no subdirs → skip the literal glob
       lang=$(basename "$langdir")
       [ "$lang" = en ] && continue
       for en in "$base"/en/*.en.md; do
         [ -f "$en" ] || continue
         name=$(basename "$en" .en.md)
         tr="$base/$lang/$name.$lang.md"
         [ -f "$tr" ] || echo "MISSING $lang: $tr (counterpart of $en)"
       done
     done
   done
   ```

2. **Diff pass — only if the repo is a git repo** (`git rev-parse --is-inside-work-tree` exits 0). For each `*.en.md` in the diff against `HEAD` or the merge base, verify the matching translation(s) — `*.<lang>.md` for each present language — are also in the diff. Report mismatches as FAIL.

   If the repo isn't initialized, skip this pass and say so.

3. **Stale pass.** For each EN ↔ translation pair, compare modification times (`stat -f %m` on macOS). If EN is newer than a translation by more than the file mtime resolution, flag as WARN — that translation may have drifted.

4. **Do not edit the translation files.** This skill only reports. Offer to draft an update only after the user confirms.

## Reporting

- Summary line: `Translation parity: <N> FAIL, <N> WARN, <N> OK` (note which languages were checked).
- One line per finding with the file path + language.
- Exit early with `OK` if no findings — do not pad the output.

## Apply the principle

Name **Sīlasāmaññatā** — communal conventions. The translation-parity rule is a community contract; one-sided edits silently break it.
