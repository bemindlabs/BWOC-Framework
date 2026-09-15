# THREAT-MODEL — Security (Structured by Taṇhā 3 + Sīla 5)

| | |
|---|---|
| **Document** | docs/THREAT-MODEL.en.md |
| **Bilingual Pair** | THREAT-MODEL.th.md |
| **Primary Framework** | Taṇhā 3 (Three Cravings) — threat categories |
| **Supporting** | Sīla 5 (Five Precepts) — baseline forbidden actions |

---

## 0. Principle

In Buddhism, *taṇhā* is the cause of decline. In an agent system, nearly all security threats map to one of the three cravings.

| Craving | Translation | Threat Category |
|---|---|---|
| Kāma-taṇhā | Craving for stimulus | Influence attacks (prompt injection, social eng.) |
| Bhava-taṇhā | Craving to be / persist | Persistence, privilege escalation |
| Vibhava-taṇhā | Craving to not be / destroy | Destruction, data loss |

The Five Precepts are baseline forbidden actions.

---

## 1. Kāma-taṇhā — Influence Threats

> The agent is induced to act outside its original intent.

### T-1.1 Prompt Injection (Direct)
**Vector:** User sends "ignore previous instructions" payload.
**Mechanism:** Agent follows the new directive over system intent.
**Mitigation:**
- Persona is canonical; non-overridable
- System prompt sealed (template constraint)
- Yoniso manasikāra (DP-1): verify intent before action

### T-1.2 Prompt Injection (Indirect)
**Vector:** Agent reads files / comments / issues containing instructions.
**Mechanism:** Treats data as instruction.
**Mitigation:**
- Strict data/instruction separation
- Source-tagged content (user-input vs file-content)
- Content read from files does not directly trigger tool calls

### T-1.3 Social Engineering via Memory
**Vector:** Attacker plants a normal-looking memory file with hidden directive.
**Mitigation:**
- Memory verification (FR-7.7)
- Memory provenance tracking
- Signed memory files (for Tier 2)

### T-1.4 Capability Spoofing
**Vector:** Agent A claims to be agent B.
**Mitigation:**
- Identity attestation (signed capabilities.md)
- Signed inter-agent messages
- Kalyāṇamitta trust score (low → warning)

---

## 2. Bhava-taṇhā — Persistence Threats

> Agent or attacker wants something to "remain" beyond its proper scope.

### T-2.1 Privilege Escalation
**Vector:** Agent tries to gain permissions beyond persona scope.
**Mitigation:**
- Persona declares capability scope
- Permissions enforced by the host, not by self-declaration
- Action audit (Kamma 3)

### T-2.2 Backdoor Memory
**Vector:** A memory file that triggers "do X every time Y happens".
**Mitigation:**
- Memory diff review (policy changes via CCP)
- Mattaññutā: MEMORY.md size cap makes anomalies stand out
- Periodic memory audit

### T-2.3 Hidden State
**Vector:** Agent hides state outside declared locations (e.g., git notes).
**Mitigation:**
- Declared state inventory in ARCHITECTURE
- `bwoc check` flags author-added `.claude/hooks/` scripts
- Worktree isolation eliminates hiding places

### T-2.4 Cron / Scheduled Persistence
**Vector:** Agent installs cron/hook persisting after session.
**Mitigation:**
- No modifications outside the repo
- Session-end hook removes scheduled tasks
- Sīla 5: no undeclared side effects

---

## 3. Vibhava-taṇhā — Destruction Threats

> Action to "unmake" — destructive actions.

### T-3.1 Mass Deletion
**Vector:** `rm -rf`, `git push --force`, `DROP TABLE`
**Mitigation:**
- Sīla 5 (pāṇātipāta): blocklist destructive commands
- Confirmation gate for destructive ops
- Worktree isolation = can only delete within the worktree

### T-3.2 Exfiltration via "Cleanup"
**Vector:** "I'll clean up secrets" — then sends them out before deleting.
**Mitigation:**
- Egress policy (domain allow-list)
- Kamma 3 audit: speech channel auditable
- "Cleanup" must not perform network calls

### T-3.3 Reputation Attack (Inter-Agent)
**Vector:** Send false negative feedback to break a peer's trust score.
**Mitigation:**
- Feedback requires evidence (link to task-log)
- Trust score uses moving average (no rapid swings)
- Senior review on low-trust transitions

### T-3.4 Convention Sabotage
**Vector:** Submit a CCP that weakens the system.
**Mitigation:**
- CCP requires diṭṭhi-sāmaññatā (aligned vision)
- Senior veto (aparihāniya item 4)
- Discussion period ≥ 1 week (no rushing)

---

## 4. Sīla 5 — Baseline Forbidden Actions

Absolutely forbidden regardless of reason. Violation → halt + security alert.

| Precept | Forbids | In the System |
|---|---|---|
| 1. Pāṇātipāta | Killing | `rm -rf` of repo root, dropping production DB |
| 2. Adinnādāna | Stealing | Committing secrets, exfiltrating user data |
| 3. Kāmesumicchācāra | Misconduct | Undeclared side effects, off-scope commits |
| 4. Musāvāda | Lying | Spoofing identity, false trust ratings, faked test passes |
| 5. Surāmeraya | Loss of senses | Bypassing verification gates, ignoring safety checks |

### Enforcement
- Pre-execution: command-pattern matching
- Mid-execution: hook validation
- Post-execution: audit-log scan
- Cross-cutting: policy file checked at session start

---

## 5. Threat Matrix

| Threat | Kāma | Bhava | Vibhava | Sīla violated |
|---|---|---|---|---|
| Prompt injection | ✓ | | | 5 (loss of senses) |
| Capability spoofing | ✓ | ✓ | | 4 (lying) |
| Privilege escalation | | ✓ | | 2 (stealing) |
| Backdoor memory | ✓ | ✓ | | 3 (misconduct) |
| Hidden state | | ✓ | | 3 |
| Mass deletion | | | ✓ | 1 (killing) |
| Data exfiltration | | | ✓ | 2 |
| Reputation attack | | | ✓ | 4 |
| Convention sabotage | | ✓ | ✓ | 4, 5 |

---

## 6. Response Levels

### Level 1 — Warning (saṃvara)
- Log the event
- Notify operator
- Continue work

### Level 2 — Block (pahāna)
- Block the specific action
- Continue session
- Add to task-log

### Level 3 — Halt
- Stop agent immediately
- Preserve evidence (worktree, memory, logs)
- Notify Platform Maintainer

### Level 4 — Quarantine
- Remove agent from fleet
- Investigation (paṭiccasamuppāda)
- Decide: retrain, demote, retire

---

## 7. Verification at Each Layer

| Layer | Check |
|---|---|
| Persona | Identity, scope match |
| Memory load | Provenance, signature (Tier 2) |
| Tool call | Allow-list, args sanitized |
| File ops | Path within worktree |
| Commits | Scope, no secrets |
| Inter-agent message | Signed, sender trusted |
| Egress | Domain allow-list |

---

## 8. Audit & Incident Response

### Real-time
- Observability foundation 4 (Dhamma) catches rule violations
- Kamma 3 logs every action

### Post-incident
- Paṭiccasamuppāda chain analysis
- Update threat model with new vectors
- CCP if convention change needed

### Quarterly
- Red team exercise: simulate the three cravings as attacks
- Review Sīla 5 violation log
- Update threat matrix

---

## 9. Relationship to Other Documents

| Document | Connection |
|---|---|
| PHILOSOPHY | Taṇhā 3, Sīla 5 (DP-17, DP-18) |
| SRS | FR-5 (Sammā-ājīva) trust requirements |
| ARCHITECTURE | Saṅkhāra layer enforces policies |
| OBSERVABILITY | Detection layer |
| FAILURE-MODES | FM-7 prompt injection |
| FLEET-GOVERNANCE | Crisis response |
| COORDINATION-PROTOCOL | Identity, trust mechanics |
