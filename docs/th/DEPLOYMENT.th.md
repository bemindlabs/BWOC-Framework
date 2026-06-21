---
title: Deployment
parent: ไทย
nav_order: 16
---

# การ Deploy Agent ขึ้นเซิร์ฟเวอร์

คู่มือนี้ครอบคลุมการรัน BWOC agent เป็น session ที่อยู่ยาวบนเครื่องระยะไกล —
โดยเฉพาะกรณีที่พบบ่อยคือ **VPS ที่มีแต่ root** (Hostinger และผู้ให้บริการราคาถูก
หลายเจ้า) ที่ล็อกอินเป็น `root` และยังไม่มี user แบบไม่มีสิทธิ์พิเศษเลย

> [!warning] **agent ที่ทำงานอัตโนมัติต้องไม่รันเป็น `root`** vendor CLI จะปฏิเสธโหมดอัตโนมัติ (bypass-permission) ใต้ root ด้วยเหตุผลนี้พอดี — เช่น `claude --remote-control <name> --dangerously-skip-permissions` จะออกพร้อมข้อความ *"cannot be used with root/sudo privileges for security reasons"* ทางแก้ไม่ใช่การฝืน แต่คือการรัน agent เป็น user เฉพาะที่ไม่มีสิทธิ์พิเศษ ซึ่งเป็นท่าที่ควรทำกับ **ทุก** เครื่อง ไม่ใช่แค่เครื่องที่มีแต่ root

---

## user สำหรับรัน agent (ไม่ใช่ root)

สร้าง user บริการที่ไม่มีสิทธิ์พิเศษหนึ่งตัว ให้เป็นเจ้าของ workspace ของ agent
และเป็นคนรัน session ทำครั้งเดียวในฐานะ `root` 5 ขั้นตอน

### 1. สร้าง user

```bash
useradd -m -s /bin/bash bwoc          # -m = สร้าง home dir; ตั้งชื่ออะไรก็ได้
# (Debian/Ubuntu ใช้แทนได้: adduser --disabled-password --gecos "" bwoc)
```

### 2. ย้าย workspace ไปไว้ใต้ user ใหม่

ถ้า incarnate agent ไว้ในฐานะ root แล้ว ให้ย้าย workspace เข้าไปใน home ของ user
แล้วโอนความเป็นเจ้าของ:

```bash
mv /root/my-workspace /home/bwoc/      # โฟลเดอร์ที่มี .bwoc/ + agents/
chown -R bwoc:bwoc /home/bwoc/my-workspace
```

ถ้าจะเริ่มใหม่เลย? ก็แค่รัน `bwoc init` / `bwoc new` **ในฐานะ user `bwoc`** (ดูวิธี
สวมเป็น user นั้นในขั้นตอนที่ 5) ทุกอย่างจะถูกตั้งเจ้าของให้ถูกต้องตั้งแต่ต้น

### 3. ย้าย credential

identity ของ agent และ backend auth ต้องอ่านได้โดย user ใหม่:

- **กุญแจลงนามของ agent** — `agents/<agent>/.bwoc/agent.key` มันย้ายไปพร้อม
  workspace ในขั้นตอนที่ 2 แล้ว ตรวจให้แน่ใจว่าเป็น **เจ้าของอ่านได้คนเดียว**:
  ```bash
  chmod 600 /home/bwoc/my-workspace/agents/<agent>/.bwoc/agent.key
  ```
  (`bwoc doctor` จะเตือนถ้ากุญแจ group/other อ่านได้ และ `bwoc doctor --auto`
  จะ chmod ให้)
- **Backend auth** — ตั้งค่านี้ **ในฐานะ user `bwoc`** ไม่ใช่ root เพราะ vendor CLI
  เก็บ session/login ไว้ใต้ home ของ *user ที่รัน*:
  - CLI แบบ subscription/login (Claude, Codex): รัน login ของ CLI นั้นครั้งหนึ่ง
    ในฐานะ `bwoc` (เช่น `claude login`) เพื่อให้ token ไปอยู่ใน `~bwoc/`
  - backend แบบ API-key: ใส่ key ไว้ใน environment ของ user `bwoc` (ผ่าน
    `Environment=` ของ systemd unit ในขั้นตอนที่ 5 หรือ shell profile) —
    อย่าใส่ใน workspace หรือไฟล์ที่ทุกคนอ่านได้

### 4. ตรวจสอบในฐานะ user ใหม่

```bash
su - bwoc
cd ~/my-workspace
bwoc doctor            # manifest, symlink, perms ของกุญแจ, ความพร้อมของ model
bwoc list              # agent โผล่มา เป็นของ bwoc
```

### 5. รัน session — เลือกอย่างใดอย่างหนึ่ง

**เฉพาะกิจ** (session interactive/remote-control สั้น ๆ):

```bash
su - bwoc -c 'cd ~/my-workspace/agents/<agent> && claude --remote-control <agent> --dangerously-skip-permissions'
```

รันจาก **โฟลเดอร์ของ agent เอง** (`agents/<agent>/`) — เพราะ `AGENTS.md` +
`config.manifest.json` อยู่ตรงนั้น backend จึงโหลด persona/context ของ agent ได้
และเพราะคำสั่งรันในฐานะ `bwoc` (ไม่ใช่ root) แล้ว โหมด bypass-permission จึงผ่าน

**แบบมีตัวดูแล** (อยู่รอด logout/reboot — แนะนำสำหรับ worker ที่รันยาว) systemd
unit ที่ `/etc/systemd/system/bwoc-agent@.service`:

```ini
[Unit]
Description=BWOC agent %i
After=network-online.target

# `%i` คือชื่อโฟลเดอร์ของ agent ใต้ agents/ (ส่วนหลัง @ ตอน enable unit) —
# `bwoc-agent --serve` อ่าน config.manifest.json จาก CWD ดังนั้น working directory
# ต้องเป็นโฟลเดอร์ของ agent ไม่ใช่ root ของ workspace
[Service]
User=bwoc
Group=bwoc
WorkingDirectory=/home/bwoc/my-workspace/agents/%i
# เฉพาะ backend แบบ API-key — CLI แบบ subscription ใช้ login ที่เก็บไว้ของ user bwoc:
# Environment=ANTHROPIC_API_KEY=...
ExecStart=/usr/local/bin/bwoc-agent --serve
Restart=on-failure
RestartSec=5

[Install]
WantedBy=multi-user.target
```

```bash
systemctl daemon-reload
systemctl enable --now bwoc-agent@<agent>.service
journalctl -u bwoc-agent@<agent> -f        # ตาม log
```

`User=bwoc` คือสิ่งที่ทำให้ปลอดภัย: systemd ลดสิทธิ์ลงเป็น user ที่ไม่มีสิทธิ์พิเศษ
ดังนั้น daemon (และ harness session ที่มันดูแล) จึงไม่ถือสิทธิ์ root เลย

---

## Container (อีกทางหนึ่งของการแยกสภาพแวดล้อม)

Container เป็นอีกวิธีในการได้ runtime แบบ non-root ที่แยกออกมา — repo มี
[`deploy/standalone-agent.Dockerfile`](https://github.com/bemindlabs/BWOC-Framework/blob/main/deploy/standalone-agent.Dockerfile)
ให้เป็นจุดเริ่มต้น รัน container ด้วย `USER` ที่ไม่ใช่ root และ mount workspace เข้าไป
กฎเรื่อง credential เหมือนเดิม (กุญแจเจ้าของอ่านคนเดียว, backend auth อยู่ใน
environment ของ container ไม่ใช่ฝังในอิมเมจ)

---

## เช็กลิสต์ความปลอดภัย

- [ ] session ของ agent รันในฐานะ **user ที่ไม่มีสิทธิ์พิเศษ** ไม่ใช่ `root`
- [ ] `agent.key` เป็น **เจ้าของอ่านคนเดียว** (`chmod 600`) — ตรวจด้วย `bwoc doctor`
- [ ] API key ของ backend อยู่ใน environment ของบริการ **ไม่ใช่** ใน workspace
      หรือไฟล์ที่ทุกคนอ่านได้
- [ ] user ที่ไม่มีสิทธิ์พิเศษ **ไม่มีสิทธิ์ `sudo`** ที่ไม่จำเป็น

---

## Roadmap

helper `bwoc agent run --as-user <user>` เพื่อทำขั้นตอนลดสิทธิ์ + launch ให้อัตโนมัติ
(จะได้ไม่ต้องเขียน glue ของ `su`/systemd เอง) วางแผนไว้เป็น follow-up จนกว่าจะถึงตอนนั้น
รูปแบบที่บันทึกไว้นี้คือเส้นทางที่รองรับ

---

## ที่เกี่ยวข้อง

- [[INCARNATION]] — การสร้าง agent ตั้งแต่แรก
- [[WORKSPACE]] — สิ่งที่ `bwoc init` วางไว้ (โฟลเดอร์ที่ย้ายในขั้นตอนที่ 2)
- `bwoc doctor` — ตรวจ perms ของกุญแจ, manifest, และความพร้อมของ model บนเครื่อง
