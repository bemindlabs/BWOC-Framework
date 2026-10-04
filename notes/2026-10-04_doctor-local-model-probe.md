# 2026-10-04 — `bwoc doctor` probes LiteLLM as well as Ollama

On a host whose local model sits behind LiteLLM, `bwoc doctor` warned "Ollama not reachable at localhost:11434", which was misleading. An uncommitted local edit (branch `doctor-vllm-probe`) fixed the warning by changing the probe to this host's LiteLLM port, `localhost:10400`. That would have put one deployment's port into the framework, against backend neutrality (Samānattatā). This change keeps the intent and resolves the endpoint instead.

## What changed
- `check_ollama` is replaced by `check_local_model`.
- The endpoints probed come from `local_model_endpoints`, which mirrors the harness:
  - the `ollama` backend's default, `http://localhost:11434/v1`;
  - the `litellm` backend's `LITELLM_API_BASE`, or its documented default `http://localhost:4000/v1`.
- A TCP connect to each endpoint, via `ToSocketAddrs`, so hostnames and IPv6 work. Any one reachable endpoint is a PASS that names what answered; none reachable is a WARN that lists every endpoint tried.
- `host_port` parses `scheme://[user@]host[:port]/…`, defaulting the port by scheme and handling bracketed IPv6.

## Decisions
- **The constants are duplicated, not imported.** bwoc-cli does not depend on bwoc-harness and should stay lean. A comment ties the two together.
- **One PASS is enough.** Two WARNs on a host that uses only one backend would be noise. Like the old probe, the check stays informational.
- **There's no endpoint knob in doctor itself.** A deployment already configures `LITELLM_API_BASE` for the harness, so doctor reads the same variable.

## Bugs surfaced (not fixed here)
- `HARNESS.en.md` / `HARNESS.th.md` §ollama say the harness honours `$OLLAMA_BASE_URL`. No code reads that variable.

## Related
- The superseded local diff is kept at `~/.claude/projects/…/doctor-vllm-probe-uncommitted-2026-10-04.patch`, off-repo.
