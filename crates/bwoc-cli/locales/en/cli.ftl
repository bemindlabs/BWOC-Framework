# BWOC CLI — English strings.
# Format: Project Fluent (https://projectfluent.org/).
# Phase 1 v2.0 scaffold; message keys land as commands are implemented.

scaffold-banner = bwoc (Phase 1 v2.0 scaffold) — lang={ $lang }
not-implemented = command not yet implemented in Phase 1
default-help-hint = bwoc (Phase 1 v2.0) — try `bwoc --help`

# bwoc new — incarnation report + interactive prompt
new-report-incarnated = Incarnated agent: { $agent_id }
new-report-target = Target:           { $path }
new-report-registered = Registered in workspace: { $path } (appended to .bwoc/agents.toml)
new-report-not-registered = No workspace found in ancestors — agent not registered in any agents.toml
new-report-next-steps-header = Next steps:
new-report-step-check = 1. cd { $path } && bwoc check . (verify backend neutrality)
new-report-step-edit-agents = 2. Edit AGENTS.md Section 1 — fill {"{{"}placeholders{"}}"} that aren't manifest fields.
new-report-step-edit-persona = 3. Edit persona/README.md — define identity, domains, boundaries.
new-report-step-git = 4. git init && git add -A && git commit -m 'feat(agent): incarnate'
new-prompt-format = { $key } ({ $desc }):{" "}
new-prompt-format-with-default = { $key } ({ $desc }) [default: { $default }]:{" "}
new-detect-stack = Detected project: { $stack } — defaults will be filled for lintCmd / formatCmd / testCmd / buildCmd (press Enter to accept each).
new-detect-unknown = No project stack detected — please type each of lintCmd / formatCmd / testCmd / buildCmd manually.
new-model-picker-header = Common { $backend } models (pick a number, or type a custom model name):
new-role-picker-header = Common agent roles (pick a number, or type a custom role):

# bwoc dashboard — TUI labels
dash-pane-agents = agents
dash-pane-detail = detail
dash-pane-dashboard = dashboard
dash-workspace-label = Workspace: { $path }
dash-workspace-none = Workspace: (none — pass --workspace, set BWOC_WORKSPACE, or run `bwoc init`)
dash-empty-select = (select an agent to see details)
dash-empty-no-agents = (no agents registered — `bwoc new <name>` to incarnate the first)
dash-empty-no-workspace = (no workspace found — exit and run `bwoc init` first)
dash-load-error = failed to read agents: { $error }
dash-retry-hint = press `r` to retry
dash-detail-manifest = manifest:
dash-detail-label-id = id
dash-detail-label-path = path
dash-detail-label-backend = backend
dash-detail-label-incarnated = incarnated
dash-detail-label-role = role
dash-detail-label-model = model
dash-detail-label-fallback = fallback
dash-detail-label-memory = memory
dash-detail-label-version = version
dash-detail-label-health = health
dash-footer-navigate = navigate
dash-footer-refresh = refresh
dash-footer-quit = quit
new-model-picker-default-hint = (default: 1)

# bwoc check — header + PASS/WARN/FAIL labels + summaries
check-header = BWOC Agent Neutrality Check
check-target = Target: { $target }
check-label-pass = PASS
check-label-warn = WARN
check-label-fail = FAIL
check-summary-success = 0 violations, { $warnings } warning(s)
check-summary-success-tail = Neutrality check passed.
check-summary-failure = { $violations } violation(s), { $warnings } warning(s)
check-summary-failure-tail = Fix violations before merging.

# bwoc workspace validate — header + PASS/FAIL labels + summaries
validate-header = Workspace validation: { $path }
validate-label-pass = PASS
validate-label-fail = FAIL
validate-summary-success = { $passes } pass(es), 0 violation(s) — workspace is complete.
validate-summary-failure = { $passes } pass(es), { $violations } violation(s) — fix violations before operating on this workspace.

# bwoc workspace info — header + field labels + per-agent row
info-header = Workspace: { $path }
info-label-name = name
info-label-version = version
info-label-created = created
info-label-backend = backend
info-label-lang = lang
info-label-agents-dir = agents_dir
info-label-agents = agents
info-agent-row = { $id } ({ $status }) @ { $path }

# bwoc spawn — exec status (stderr)
spawn-exec-status = bwoc spawn: exec '{ $backend }' in { $path }

# bwoc list — agent registry display
list-empty = (no agents in workspace { $path })
list-col-id = ID
list-col-status = STATUS
list-col-backend = BACKEND
list-col-path = PATH

# bwoc init — success path
# (Fluent identifiers use `-`, not `.`, so we prefix instead of dotting.)
init-success-title = Initialized BWOC workspace at: { $path }
init-created-workspace-toml =   + .bwoc/workspace.toml
init-created-agents-toml =   + .bwoc/agents.toml
init-created-agents-dir =   + agents/   (incarnated agents land here)
init-created-projects-dir =   + projects/ (your work — apps/repos the agents help build)
init-created-notes-dir =   + notes/    (implementation logs — YYYY-MM-DD_<title>.md)
init-next-steps-header = Next steps:
init-next-step-validate =   bwoc workspace validate { $path }
init-next-step-new =   bwoc new <agent-name> ...        (incarnate the first agent here)

# bwoc status --banner — manifest-driven liveness replay (mirrors bwoc-agent startup output)
status-banner-alive = I am alive: { $agent_id }
status-banner-role = role:     { $role }
status-banner-model = model:    { $model }
status-banner-fallback = fallback: { $fallback }
status-banner-memory = memory:   { $memory_path }
status-banner-version = version:  { $version }

# bwoc gates — Padhana-4 verification gates
gates-title = Padhana-4 verification gates — { $agent }
gates-samvara = Samvara    (guard: static analysis / lint)
gates-pahana = Pahana     (abandon ill: format check)
gates-bhavana = Bhavana    (cultivate good: tests)
gates-anurakkhana = Anurakkhana (sustain: build before push)
gates-dry-note = Commands were NOT executed. They come from config.manifest.json (untrusted input). Review them, then re-run with --run to execute (sh -c, scrubbed env, cwd = agent dir).
gates-summary-ok = All gates configured and passed.
gates-summary-dry = All gates configured (not run).
gates-summary-fail = { $count } gate(s) failed or unconfigured.
