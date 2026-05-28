# 2026-05-28 — gcloud plugins: option-injection hardening (#91)

Follow-up nit from the #86 review: the gcloud workflow plugins passed
operator/agent-supplied values straight into `gcloud` argv, so a `-`-leading
value (e.g. `--format=...`) could be parsed by `gcloud` as a **flag** rather
than the intended positional.

## What changed

Inserted a `--` end-of-options separator before each user-supplied argument in
the plugin shell-outs:

- `gcloud-auth/gcloud.sh` — `gcloud auth login --brief -- "$account"`
- `gcloud-project/gcloud.sh` — `gcloud projects describe --format=json -- "$project"`
  and `gcloud config set project -- "$project"`

After `--`, gcloud treats the value as a positional, never an option.

## Why it was low severity

No shell injection / RCE was possible (args were quoted; the request is JSON
over stdin). On the production path it was already mitigated: the only caller,
`bwoc gcloud` (`gcloud.rs`), validates project ids via `is_valid_project_id`
(`6–30`, `[a-z0-9-]`, lowercase-first) before dispatch. The exposure was only
when the scripts run standalone (which they support for testing). This closes
that gap at the source.

## Verification

- All three user-arg call sites carry `--`; `bash -n` + shellcheck clean.
- Confirmed `gcloud` accepts `--` for each subcommand: `projects describe`
  parses past it; `config set project -- <val>` sets the value (tested in an
  isolated `CLOUDSDK_CONFIG`); `auth login --brief -- --help` treats `--help`
  as the account, not the help flag — i.e. the injection is neutralized.
- `gcloud-auth status` still emits a valid envelope (`.ok == true`).

## Related

- Closes #91. Hardens the plugins shipped in #86.
- `modules/plugins/workflow/gcloud-auth/gcloud.sh`, `gcloud-project/gcloud.sh`.
