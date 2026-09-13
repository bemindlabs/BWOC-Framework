# Showcases

Full reference agents — incarnated, with persona / memories / skills slots filled in — that demonstrate concrete BWOC patterns.

**Status: placeholder.** None have shipped. A showcase needs a real use case, a human neutrality review, and EN/TH parity, so this directory stays empty rather than holding something thin.

Planned:

| Agent | Domain | Demonstrates |
|---|---|---|
| `documentor-agent/` | Documentation writing | PHILOSOPHY-grounded persona, write-then-review workflow, central memory references |
| `code-reviewer-agent/` | Code review | Multi-mindset approach, strict backend neutrality, deep-memory tooling |
| `onboarding-agent/` | New-hire orientation | Long-form persona, indexed memory, "session continuity" pattern |
| `migration-agent/` | Database/data migrations | Multi-phase agent (one per migration step), interconnect handoff |

## How to contribute a showcase

1. Build the agent in your own workspace
2. Verify it passes `bwoc check`
3. Strip any personal/proprietary content from persona and memories
4. Submit a PR adding `examples/showcases/<your-agent>/` with the full incarnated tree + a `README.md` explaining the use case and decisions

See [`CONTRIBUTING.md`](../../CONTRIBUTING.md) for the workflow.

## See instead

- [`examples/howto/`](../howto/) — runnable recipes
- [`modules/agent-template/`](../../modules/agent-template/) — the template every agent starts from
