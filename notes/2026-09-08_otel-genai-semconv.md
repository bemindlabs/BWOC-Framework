# 2026-09-08 — OTel GenAI semantic conventions in the harness

Council decision **D1** (team tianting, sangha model, 8/8 unanimous, no dissent)
chose this as the first item after 3.0.0. This is that work — but the scope is
much smaller than the decision's own briefing implied, and the correction is the
first thing worth recording.

## The briefing was wrong, and this note is the correction

The deep-research pass was told BWOC's harness has "telemetry", full stop. It
concluded that "BWOC's undifferentiated telemetry is a real gap", and that phrase
went into the council briefing, where agent-nezha voted on the words "BWOC's
agent loop is currently unrecorded."

**That was false.** `telemetry.rs` has exported an OTLP trace since BWOC-2, with a
three-level hierarchy and real GenAI attributes: `gen_ai.operation.name`,
`gen_ai.usage.input_tokens|output_tokens`, `gen_ai.request.model` per turn, and
`gen_ai.tool.name` per tool call. The council voted on an under-described
starting point.

What survived the correction — the four things that were actually missing — is
what this change fixes. The vote's *outcome* stands; its *premise* did not.

## What changed

1. **Released binaries can now emit at all.** `otel` was a compile-time feature
   outside `default`, and `release.yml` never passed it — so anyone installing
   from a GitHub Release or Homebrew could not turn tracing on without
   rebuilding from source. `release.yml` now builds
   `--features bwoc-harness/otel`.
2. **Span names follow the conventions.** `bwoc.session` → `invoke_agent
   <agent>`, `bwoc.turn` → `chat <model>`, `execute_tool` → `execute_tool
   <tool>`, per the semconv `{operation} {target}` recommendation.
3. **`gen_ai.provider.name` stops lying.** It was hardcoded `"openai"` on every
   path, including native Anthropic and the vendor CLIs. New
   `ProviderClient::provider_name()` derives it from the live client;
   `Telemetry::with_provider` carries it to the record and out onto both the
   session and turn spans.
4. **Docs**: a `§OpenTelemetry` section in `HARNESS.{en,th}.md` with the span
   tree, the env-gate, and the caveats stated plainly.

## Decisions

- **Ship OTel in release builds, keep it off at runtime.** The owner's call, and
  it does not weaken the dep-quarantine rationale ("heavy + network egress"):
  `export_otel_span` returns before constructing an exporter when
  `OTEL_EXPORTER_OTLP_ENDPOINT` is unset, so an unconfigured binary opens no
  socket. Compiling it out did not remove the egress capability from the design;
  it removed the operator's ability to use it. Same opt-in shape as VS Code
  Copilot and Claude Code.
- **Derive the provider from the client, never from config.** The attribute is
  supposed to name the endpoint the tokens came from. Config says what was
  *asked for*; the live client knows what *answered*.
- **A custom open-enum value beats a wrong well-known one.** An OpenAI-shaped
  endpoint that is neither api.openai.com nor a local Ollama reports
  `openai_compatible`, and an unrecognised vendor CLI reports its own command
  name. Borrowing a vendor's name from a URL would be a guess presented as fact —
  which is exactly the defect being fixed.
- **Span names, not a new exporter.** Renaming spans is cheap to reverse, and
  GenAI semconv is still Development-status (split into its own repo at semconv
  v1.42.0). agent-taibai's vote weighted precisely this reversibility.

## Alternatives considered

- **A new OTel SDK integration or a second exporter.** Unnecessary — the
  opentelemetry 0.32 plumbing already existed. Adding one would have been the
  research finding taken at face value instead of read against the source.
- **Omitting `gen_ai.provider.name` when unknown.** Semconv marks it Required.
  An honest custom value satisfies the requirement without asserting a falsehood.
- **Live streaming spans instead of session-finish replay.** A real improvement
  and out of scope here; the replay caveat is documented rather than hidden.

## Status / deferred

Still true after this change, and now written in the docs rather than only in
code comments: spans are replayed at session finish (absolute timestamps
approximate), and tool spans cover their whole turn because per-tool timing is
not recorded.

The other three council options are untouched — ACP (#485) still turns on the
unanswered trust-tier question that every one of the eight agents named as their
reason for voting against it.

## Related

- `research/2026-09-08_ecosystem-gap-analysis.md` — the research, including the
  scope limits that let this overstatement through.
- Council decision D1, `~/workspaces/bwoc/.bwoc/council/`.
