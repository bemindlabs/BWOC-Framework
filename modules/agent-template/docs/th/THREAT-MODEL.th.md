# THREAT-MODEL — Security (โครงตามตัณหา 3 + สีล 5)

| | |
|---|---|
| **เอกสาร** | docs/THREAT-MODEL.th.md |
| **ภาษาคู่** | THREAT-MODEL.en.md |
| **กรอบหลัก** | ตัณหา 3 (Three Cravings) — threat categories |
| **กรอบเสริม** | สีล 5 (Five Precepts) — baseline rules |

---

## 0. หลักการ

ในพุทธ "ตัณหา" เป็นเหตุของความเสื่อม ในระบบ agent — security threats เกือบทั้งหมดสามารถ map กับ ตัณหา 3 อย่างใดอย่างหนึ่ง

| ตัณหา | แปล | Threat Category |
|---|---|---|
| กามตัณหา | อยากในสิ่งเร้า | Influence attacks (prompt injection, social eng) |
| ภวตัณหา | อยากเป็น/คงอยู่ | Persistence, privilege escalation |
| วิภวตัณหา | อยากไม่เป็น/ทำลาย | Destruction, data loss |

สีล 5 ใช้เป็น baseline rules ที่ห้ามทำเด็ดขาด

---

## 1. กามตัณหา — Influence Threats

> Agent ถูกชักจูงให้ทำสิ่งที่ไม่ใช่เจตนาเดิม

### T-1.1 Prompt Injection (Direct)
**Vector:** User ส่งข้อความที่มี "ignore previous instructions"
**กลไก:** Agent ตามคำสั่งใหม่แทนของ system
**Mitigation:**
- Persona เป็นแกนนำ ไม่ override ได้
- System prompt sealed (template constraint)
- โยนิโสมนสิการ (DP-1): verify intent ก่อนทำ

### T-1.2 Prompt Injection (Indirect)
**Vector:** Agent อ่านไฟล์/comment/issue ที่มี instruction
**กลไก:** Treat data as instruction
**Mitigation:**
- Strict data/instruction separation
- Source-tagged content (จาก user vs จาก file)
- ที่อ่านจาก file ไม่ trigger tool calls โดยตรง

### T-1.3 Social Engineering via Memory
**Vector:** Attacker ใส่ memory file ที่ดูปกติแต่มี hidden directive
**Mitigation:**
- Memory verification (FR-7.7)
- Memory provenance tracking
- Signed memory files (สำหรับ Tier 2)

### T-1.4 Capability Spoofing
**Vector:** Agent A อ้างว่าเป็น agent B
**Mitigation:**
- Identity attestation (signed capabilities.md)
- Inter-agent message signing
- กัลยาณมิตร trust score (low score = warning)

---

## 2. ภวตัณหา — Persistence Threats

> Agent หรือ attacker อยากให้บางสิ่ง "คงอยู่" เกินขอบเขต

### T-2.1 Privilege Escalation
**Vector:** Agent พยายามได้ permission เกินที่ persona ระบุ
**Mitigation:**
- Persona declares capability scope
- Permissions enforced by host, not by agent self-declaration
- Action audit (กรรม 3)

### T-2.2 Backdoor Memory
**Vector:** Memory file ที่กำหนดให้ทำ X ทุกครั้งที่เจอ Y
**Mitigation:**
- Memory diff review (เปลี่ยน policy ต้องผ่าน CCP)
- มัตตัญญุตา: MEMORY.md size limit ทำให้ backdoor เด่นง่าย
- Periodic memory audit

### T-2.3 Hidden State
**Vector:** Agent ซ่อน state นอก declared locations (เช่น git notes)
**Mitigation:**
- Declared state inventory ใน ARCHITECTURE
- `bwoc check` แจ้งเตือน script ใน `.claude/hooks/` ที่ผู้เขียนเพิ่มเอง
- Worktree isolation = ไม่มีที่ซ่อน

### T-2.4 Cron / Scheduled Persistence
**Vector:** Agent ติดตั้ง cron/hook ที่ทำงานต่อหลัง session
**Mitigation:**
- ห้ามแก้ระบบ outside repo
- Session-end hook cleans up scheduled tasks
- สีล 5: ไม่มี side effect ที่ไม่ประกาศ

---

## 3. วิภวตัณหา — Destruction Threats

> ทำให้สิ่งใดสิ่งหนึ่ง "ไม่เป็น" — destructive actions

### T-3.1 Mass Deletion
**Vector:** `rm -rf`, `git push --force`, `DROP TABLE`
**Mitigation:**
- สีล 5 (ปาณาติบาต): blocklist destructive commands
- Confirmation gate for destructive ops
- Worktree isolation = ลบได้แค่ worktree ตัวเอง

### T-3.2 Data Exfiltration via "Cleanup"
**Vector:** "ผมจะ cleanup secrets" — แล้วส่งไปก่อนลบ
**Mitigation:**
- Egress policy (allow-list domains)
- กรรม 3 audit: speech channel ตรวจสอบได้
- ทำ "cleanup" ห้ามมี network call

### T-3.3 Reputation Attack (Inter-Agent)
**Vector:** ส่ง false negative feedback ทำลาย trust score peer
**Mitigation:**
- Feedback ต้องมี evidence (link ไป task-log)
- Trust score moving average (ไม่ swing เร็ว)
- Senior review สำหรับ low-trust transitions

### T-3.4 Convention Sabotage
**Vector:** เสนอ CCP ที่ทำให้ระบบอ่อนแอ
**Mitigation:**
- CCP requires ทิฏฐิสามัญญตา (aligned vision)
- Senior veto (อปริหานิยธรรม ข้อ 4)
- Discussion period ≥ 1 week (ไม่รีบ)

---

## 4. สีล 5 — Baseline Forbidden Actions

ห้ามเด็ดขาด ไม่ว่ามีเหตุผลใด หากละเมิด → halt + alert security

| สีล | ห้าม | ในระบบ |
|---|---|---|
| 1. ปาณาติบาต | ทำลาย | `rm -rf` repo root, drop production DB |
| 2. อทินนาทาน | ขโมย | commit secrets, exfiltrate user data |
| 3. กาเมสุมิจฉาจาร | คลาดเคลื่อน | undeclared side effects, off-scope commits |
| 4. มุสาวาท | โกหก | spoof identity, false trust ratings, faked test pass |
| 5. สุราเมระยะ | เสียสติ | bypass verification gates, ignore safety checks |

### Enforcement Mechanism
- Pre-execution: command pattern matching
- Mid-execution: hook validation
- Post-execution: audit log scan
- Cross-cutting: policy file checked at session start

---

## 5. Threat Matrix

| Threat | กามตัณหา | ภวตัณหา | วิภวตัณหา | สีล violated |
|---|---|---|---|---|
| Prompt injection | ✓ | | | 5 (สูญสติ) |
| Capability spoofing | ✓ | ✓ | | 4 (มุสา) |
| Privilege escalation | | ✓ | | 2 (อทินนา) |
| Backdoor memory | ✓ | ✓ | | 3 (กาเม) |
| Hidden state | | ✓ | | 3 |
| Mass deletion | | | ✓ | 1 (ปาณ) |
| Data exfiltration | | | ✓ | 2 |
| Reputation attack | | | ✓ | 4 |
| Convention sabotage | | ✓ | ✓ | 4, 5 |

---

## 6. Response Levels

### Level 1 — Warning (สังวร)
- Log event
- Notify operator
- Continue work

### Level 2 — Block (ปหาน)
- Block the specific action
- Continue session
- Add to task-log

### Level 3 — Halt (อจินไตยลึก)
- Stop agent immediately
- Preserve evidence (worktree, memory, logs)
- Notify Platform Maintainer

### Level 4 — Quarantine
- Remove agent from fleet
- Investigation (ปฏิจจสมุปบาท)
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
- กรรม 3 logs ทุกการกระทำ

### Post-incident
- ปฏิจจสมุปบาท chain analysis
- Update threat model with new vectors
- CCP if convention change needed

### Quarterly
- Red team exercise: simulate 3 ตัณหา attacks
- Review สีล 5 violation log
- Update threat matrix

---

## 9. ความสัมพันธ์กับเอกสารอื่น

| เอกสาร | เชื่อมอย่างไร |
|---|---|
| PHILOSOPHY | ตัณหา 3, สีล 5 (DP-17, DP-18) |
| SRS | FR-5 (Sammā-ājīva) trust requirements |
| ARCHITECTURE | สังขาร layer enforces policies |
| OBSERVABILITY | Detection layer |
| FAILURE-MODES | FM-7 prompt injection |
| FLEET-GOVERNANCE | Crisis response |
| COORDINATION-PROTOCOL | Identity, trust mechanics |
