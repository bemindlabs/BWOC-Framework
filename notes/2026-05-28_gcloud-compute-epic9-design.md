# 2026-05-28 — gcloud compute slice (EPIC-9 framing)

> **Status: FROZEN (architect sign-off 2026-05-28) + implemented.** Sets the
> spec frame for `BWOC-EPIC-9` (the first write-capable GCP slice). The three
> open questions on decisions 3–4 were resolved with the recommended answers
> (see "Resolved"); the slice is implemented in this change.

This is the slice the [EPIC-8 note §6](2026-05-28_gcloud-workflow-plugin-architecture.md)
deferred the **write-verb risk matrix** to: *"when the first GCP write-capable
slice lands (compute lifecycle, most likely), this note's decision 4 expands
with a write-verb risk matrix… the matrix template is reusable across `workflow`
plugins."* EPIC-9 authors that template.

The throughline: **compute is where gcloud first issues a remote-API write, so
it earns the trust by starting with the *reversible* lifecycle verbs and the
strictest targeting discipline — not by reaching for the irreversible ones.**
A misfired `instances.stop` interrupts a workload but is recoverable with
`start`; `instances.delete` is not, so it is **out of EPIC-9** (decision 1).

## Decisions

### 1. Scope — reversible lifecycle only (`start` / `stop`) + reads

`workflow/gcloud-compute` v1 verbs:

| Verb | Kind | Notes |
|---|---|---|
| `list` | read | instances in a zone/project; mirrors `gcloud-project list` |
| `describe` | read | one instance descriptor |
| `start` | **write** | resume a stopped instance |
| `stop` | **write** | stop a running instance (graceful) |

**Explicitly deferred to a compute-v2 (or never):** `instances.delete` (irreversible
data loss on the boot disk unless `--keep-disks`), `instances.reset` (hard power-cycle,
data-loss risk for in-flight work), `create` (cost + sprawl), instance-group / MIG ops.
Rationale: EPIC-9's job is to validate the write pattern on the *lowest-blast-radius*
writes. `delete`/`reset` are a categorically larger commitment and belong behind their
own review once `start`/`stop` are exercised in the field.

### 2. Plugin + auth — reuse the EPIC-8 foundation, add nothing new

- New plugin `modules/plugins/workflow/gcloud-compute/` with its own
  `manifest.toml`, `gcloud.sh` entry, `auth.toml` (**shape only, no values**),
  and EN/TH `SPEC.md` pair.
- **Sources `../gcloud-auth/gcloud.sh`** for credential resolution +
  `gcloud_assert_authenticated` — exactly as `gcloud-project` does. No
  re-implementation of ADC / SA / env precedence (EPIC-8 decision 2).
- Stays `workflow` kind — no new kind (EPIC-8 decision 1; the kind boundary is
  the lifecycle hook, not the vendor).
- `bwoc gcloud compute {list,describe,start,stop}` dispatches it, alongside the
  existing `auth`/`project`/`status` subcommands.

### 3. Write-verb risk matrix (the reusable template — NEW)

Every `workflow` write verb is classified on four axes; the **confirmation tier**
is a function of *reversibility* × *blast radius*. This table is the template
storage (EPIC-10), serverless (EPIC-11), and IAM (EPIC-12) instantiate.

| Verb | Mutation | Reversibility | Blast radius | Idempotent? | Confirmation tier |
|---|---|---|---|---|---|
| `instances.start` | stopped → running | trivial (`stop`) | cost (resumes compute billing) | yes (running→running = no-op) | **T1 — confirm** |
| `instances.stop` | running → stopped | trivial (`start`) | availability (interrupts running workload) + cost-down | yes (stopped→stopped = no-op) | **T2 — confirm + echo full target** |

**Confirmation tiers (the reusable scale):**

- **T0 — none.** Read verbs. No prompt, `--json` always allowed.
- **T1 — confirm.** Reversible write, cost-only impact. `y/N` prompt; `--json`
  requires `--yes` (EPIC-8 / `set-default` precedent).
- **T2 — confirm + echo full target.** Reversible write with availability/data
  impact. Same gate as T1, **plus** the prompt must echo the resolved
  `project / zone / instance` so the operator confirms *which* resource — the
  dominant compute footgun is acting on the wrong instance, not the verb itself.
- **T3 — typed-name confirm.** *(template, not used in EPIC-9)* Irreversible
  (`delete`, object `delete`). Require re-typing the resource name, not just `y`.
- **T4 — refuse-by-default + explicit opt-in.** *(template, not used in EPIC-9)*
  Security-visible (IAM `bindings.add/remove`). Gated behind an explicit flag
  *and* T3, EPIC-12 only.

EPIC-9 uses **T0 / T1 / T2**. `start` = T1; `stop` = T2.

### 4. Targeting discipline — the real blast-radius control

The verb is rarely the danger; the *target* is. So:

- **`--instance <name>` and `--zone <z>` are required** for `start`/`stop` — **no
  implicit default instance.** (A default project for *reads* is fine; a default
  *instance* for a stop is not.)
- The confirmation prompt echoes the **resolved** `project / zone / instance`
  (project may come from `gcloud config`); `--json --yes` callers must pass all
  three explicitly so there is no ambient target.
- **Input validation before dispatch** (mirrors `is_valid_project_id`): instance
  names and zones validated against GCE's charset (`[a-z]([-a-z0-9]*[a-z0-9])?`,
  ≤ 63) so a `-`-leading value can't reach `gcloud` even before the `--` guard.
- **`--` option-injection guard** in the shell-out (#92 precedent), unconditionally.
- Read verbs (`list`/`describe`) stay T0 and `--json`-clean.

### 5. Skill exposure — reads only; writes are operator-CLI-only

Following EPIC-8 decision 5 (`login` excluded from `gcloud-ops` because it is
operator-driven), **`start`/`stop` are NOT exposed through any skill.** An agent
should never autonomously stop a VM. A future read-only `compute-inventory`
addition to `gcloud-ops` (`list`/`describe`) is fine; the write verbs remain
behind the operator-run `bwoc gcloud compute` CLI with their T1/T2 gates.

## Resolved (architect sign-off 2026-05-28)

1. **`stop` confirmation strength** → **T2** (confirm + echo resolved target).
   `stop` is reversible via `start`, so typed-name T3 would over-prompt
   (Mattaññutā); the dominant risk is wrong-target, which T2's echoed
   `project/zone/instance` addresses.
2. **Scope of v1** → **`start`/`stop` only.** `instances.reset` excluded (hard
   power-cycle, in-flight data-loss risk — its own future slice).
3. **Default project for writes** → **allow the `gcloud config` default, always
   echoed** in the confirmation; `--instance` and `--zone` are required.

## Alternatives considered

- **Start with `delete` because it's the "real" compute op** — rejected. The
  whole point of EPIC-9 is to validate the write pattern on the recoverable
  verbs first (THREAT-MODEL / Surameraya — no heedlessness). `delete` earns its
  own slice once the gates are proven.
- **One mega `gcloud-compute` covering all of compute** — deferred, not rejected:
  v1 is lifecycle only; instance-groups/disks/images are separate verbs added
  incrementally, each re-using this plugin's auth surface (EPIC-8 decision 2).
- **Expose `stop`/`start` in `gcloud-ops` for agent self-service** — rejected
  (decision 5). Autonomous VM lifecycle control is exactly the agent capability
  the read-mostly foundation was designed to withhold until there's a concrete,
  scoped operator need.

## Status / deferred

- Decisions 1, 2, 5 follow EPIC-8 / jira precedent and are stable.
- **Decisions 3–4 frozen** with the resolved answers above; implemented in this change.
- Live verification is gated on an operator-provided GCP sandbox with at least
  one stoppable test instance (surfacing point: the plugin's smoke-test gate).
- The risk-matrix tiers T3/T4 are authored here as the **template** for
  EPIC-10/11/12 but are not exercised by EPIC-9.

## Related

- Tracks #96 (EPIC-9). Builds on the EPIC-8 foundation (#86, shipped 2.11.0).
- [EPIC-8 gcloud foundation note](2026-05-28_gcloud-workflow-plugin-architecture.md) — §6 deferred this matrix; §7 the slice roadmap.
- [jira plugin note §4](2026-05-27_jira-plugin-architecture.md) — the write-verb / confirmation precedent this matrix generalizes.
- Future siblings: #97 (storage), #98 (serverless), #99 (IAM).
