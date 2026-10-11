# BWOC CLI — สตริงภาษาไทย
# รูปแบบ: Project Fluent (https://projectfluent.org/)
# โครงเริ่มต้น Phase 1 v2.0; คีย์จะถูกเพิ่มเมื่อแต่ละคำสั่งถูก implement

scaffold-banner = bwoc (โครง Phase 1 v2.0) — lang={ $lang }
not-implemented = คำสั่งนี้ยังไม่ implement ใน Phase 1
default-help-hint = bwoc (Phase 1 v2.0) — ลองใช้ `bwoc --help`

# bwoc new — รายงาน incarnation + prompt
new-report-incarnated = สร้าง agent: { $agent_id }
new-report-target = เป้าหมาย:         { $path }
new-report-registered = ลงทะเบียนใน workspace: { $path } (เพิ่มไปยัง .bwoc/agents.toml)
new-report-not-registered = ไม่พบ workspace ใน ancestors — agent ไม่ได้ถูกลงทะเบียนใน agents.toml ใด ๆ
new-report-next-steps-header = ขั้นต่อไป:
new-report-step-check = 1. cd { $path } && bwoc check . (ตรวจสอบ neutrality)
new-report-step-edit-agents = 2. แก้ AGENTS.md Section 1 — กรอก {"{{"}placeholders{"}}"} ที่ไม่ใช่ manifest field
new-report-step-edit-persona = 3. แก้ persona/README.md — กำหนด identity, domain, boundary
new-report-step-git = 4. git init && git add -A && git commit -m 'feat(agent): incarnate'
new-prompt-format = { $key } ({ $desc }):{" "}
new-prompt-format-with-default = { $key } ({ $desc }) [ค่าเริ่มต้น: { $default }]:{" "}
new-detect-stack = ตรวจพบ project: { $stack } — จะเติมค่าเริ่มต้นให้ lintCmd / formatCmd / testCmd / buildCmd (กด Enter เพื่อใช้ค่าเริ่มต้น)
new-detect-unknown = ไม่พบ project stack — กรุณาพิมพ์ค่าของ lintCmd / formatCmd / testCmd / buildCmd ด้วยตนเอง
new-model-picker-header = model ที่ใช้บ่อยสำหรับ { $backend } (เลือกตัวเลข หรือพิมพ์ชื่อ model เอง):
new-role-picker-header = บทบาท agent ที่ใช้บ่อย (เลือกตัวเลข หรือพิมพ์บทบาทเอง):

# bwoc dashboard — label ของ TUI
dash-pane-agents = agent
dash-pane-detail = รายละเอียด
dash-pane-dashboard = dashboard
dash-workspace-label = Workspace: { $path }
dash-workspace-none = Workspace: (ไม่พบ — ระบุ --workspace, ตั้ง BWOC_WORKSPACE, หรือรัน `bwoc init`)
dash-empty-select = (เลือก agent เพื่อดูรายละเอียด)
dash-empty-no-agents = (ยังไม่มี agent — รัน `bwoc new <ชื่อ>` เพื่อสร้าง agent แรก)
dash-empty-no-workspace = (ไม่พบ workspace — ออก แล้วรัน `bwoc init` ก่อน)
dash-load-error = อ่าน agents ไม่ได้: { $error }
dash-retry-hint = กด `r` เพื่อลองอีกครั้ง
dash-detail-manifest = manifest:
dash-detail-label-id = id
dash-detail-label-path = path
dash-detail-label-backend = backend
dash-detail-label-incarnated = สร้างเมื่อ
dash-detail-label-role = บทบาท
dash-detail-label-model = model
dash-detail-label-fallback = fallback
dash-detail-label-memory = memory
dash-detail-label-version = เวอร์ชัน
dash-detail-label-health = สุขภาพ
dash-footer-navigate = นำทาง
dash-footer-refresh = รีเฟรช
dash-footer-quit = ออก
new-model-picker-default-hint = (ค่าเริ่มต้น: 1)

# bwoc check — หัวข้อ + label PASS/WARN/FAIL + สรุป
check-header = ตรวจสอบความเป็นกลางต่อ Backend ของ BWOC Agent
check-target = เป้าหมาย: { $target }
check-label-pass = ผ่าน
check-label-warn = เตือน
check-label-fail = ไม่ผ่าน
check-summary-success = 0 ละเมิด, { $warnings } คำเตือน
check-summary-success-tail = การตรวจสอบ neutrality ผ่าน
check-summary-failure = { $violations } ละเมิด, { $warnings } คำเตือน
check-summary-failure-tail = แก้ violation ก่อน merge

# bwoc workspace validate — หัวข้อ + label PASS/FAIL + สรุป
validate-header = ตรวจสอบ workspace: { $path }
validate-label-pass = ผ่าน
validate-label-fail = ไม่ผ่าน
validate-summary-success = { $passes } ผ่าน, 0 ละเมิด — workspace ครบถ้วน
validate-summary-failure = { $passes } ผ่าน, { $violations } ละเมิด — แก้ก่อนใช้งาน workspace นี้

# bwoc workspace info — หัวข้อ + label + แถวต่อ agent
info-header = Workspace: { $path }
info-label-name = ชื่อ
info-label-version = เวอร์ชัน
info-label-created = สร้างเมื่อ
info-label-backend = backend
info-label-lang = ภาษา
info-label-agents-dir = agents_dir
info-label-agents = agent
info-agent-row = { $id } ({ $status }) @ { $path }

# bwoc spawn — สถานะการ exec (stderr)
spawn-exec-status = bwoc spawn: exec '{ $backend }' ใน { $path }

# bwoc list — แสดง registry ของ agent
list-empty = (ไม่มี agent ใน workspace { $path })
list-col-id = ID
list-col-status = สถานะ
list-col-backend = Backend
list-col-path = Path

# bwoc init — เส้นทางสำเร็จ
# (Fluent identifier ใช้ `-` ไม่ใช้ `.` จึงใช้ prefix แทน)
init-success-title = สร้าง BWOC workspace ที่: { $path }
init-created-workspace-toml =   + .bwoc/workspace.toml
init-created-agents-toml =   + .bwoc/agents.toml
init-created-agents-dir =   + agents/   (agent ที่ incarnate แล้วจะอยู่ที่นี่)
init-created-projects-dir =   + projects/ (งานของคุณ — app/repo ที่ agent ช่วยสร้าง)
init-created-notes-dir =   + notes/    (บันทึก implementation — YYYY-MM-DD_<title>.md)
init-next-steps-header = ขั้นต่อไป:
init-next-step-validate =   bwoc workspace validate { $path }
init-next-step-new =   bwoc new <agent-name> ...        (incarnate agent แรกที่นี่)

# bwoc status --banner — replay liveness banner จาก manifest (เหมือนที่ daemon พิมพ์ตอน startup)
status-banner-alive = ฉันยังมีชีวิตอยู่: { $agent_id }
status-banner-role = role:     { $role }
status-banner-model = model:    { $model }
status-banner-fallback = fallback: { $fallback }
status-banner-memory = memory:   { $memory_path }
status-banner-version = version:  { $version }

# bwoc gates — ด่านตรวจสอบ ปธาน 4
gates-title = ด่านตรวจสอบ ปธาน 4 — { $agent }
gates-samvara = สังวร      (ระวัง: วิเคราะห์โค้ด / lint)
gates-pahana = ปหาน      (ละสิ่งไม่ดี: ตรวจรูปแบบ)
gates-bhavana = ภาวนา     (เจริญสิ่งดี: ทดสอบ)
gates-anurakkhana = อนุรักขนา (รักษา: build ก่อน push)
gates-dry-note = ยังไม่ได้รันคำสั่งใด ๆ — คำสั่งมาจาก config.manifest.json (ข้อมูลที่ไม่น่าเชื่อถือ) โปรดตรวจสอบก่อน แล้วรันซ้ำด้วย --run เพื่อสั่งทำงาน (sh -c, env ที่ผ่านการกรอง, cwd = โฟลเดอร์เอเจนต์)
gates-summary-ok = ทุกด่านตั้งค่าครบและผ่าน
gates-summary-dry = ทุกด่านตั้งค่าครบ (ยังไม่ได้รัน)
gates-summary-fail = { $count } ด่านไม่ผ่านหรือยังไม่ได้ตั้งค่า
