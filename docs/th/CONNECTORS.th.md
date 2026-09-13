---
title: Connectors
parent: ไทย
nav_order: 13
---

# Chat Connectors (ตัวเชื่อมแชต)

**Chat connectors** ให้คนคุยกับ BWOC agent จากแอปแชตที่ใช้ทุกวัน —
**Telegram**, **Discord**, **LINE** หรือ **iMessage** — แบบ DM ส่วนตัวได้ทุกแพลตฟอร์ม
ส่วนห้องกลุ่มร่วมกันขึ้นกับแพลตฟอร์ม (iMessage รองรับเฉพาะ DM ในตอนนี้)
เป็นโครงสร้างฝั่ง operator: โค้ดเครือข่ายอยู่ใน crate เดียว (`bwoc-connect`) เพื่อให้
`bwoc` CLI, runtime ของ agent และ core บางเบา (dep-quarantine).

> [!abstract] connector คือ "แค่ frontend แชตอีกตัวหนึ่ง" แต่ละข้อความขาเข้ากลายเป็น user turn หนึ่งครั้งของ session `bwoc-harness --chat` (โปรโตคอลเดียวกับ `bwoc chat --tui`) แล้วส่งคำตอบของ agent กลับไป — streaming, การขออนุญาต และ compaction ได้มาฟรีจากโปรโตคอลร่วมนี้

---

## สี่แพลตฟอร์ม

| แพลตฟอร์ม | รับข้อความ | ส่ง | สตรีม | รันได้ที่ |
|---|---|---|---|---|
| **Telegram** | long-poll (`getUpdates`) | `sendMessage` | ✅ แก้ข้อความเดิม | ทุกที่ |
| **Discord** | gateway websocket | REST `createMessage` | ✅ แก้ข้อความเดิม | ทุกที่ |
| **LINE** | **webhook** ขาเข้า (HTTPS) | reply-token / push | ✗ (ไม่มี edit API) | ทุกที่ (ต้องมี URL สาธารณะ) |
| **iMessage** | poll `chat.db` แบบอ่านอย่างเดียว | `osascript` → Messages.app | ✗ (ไม่มี edit API) | **macOS เท่านั้น** (ล็อกอิน iMessage อยู่) |

ทั้งสี่ใช้แกน routing เดียวกัน (`run_bridge`): กรอง allow-list, แยก DM/กลุ่ม,
เชื่อมกลุ่ม→ทีม, และใช้ session ซ้ำต่อห้องแชต การเพิ่มแพลตฟอร์มคือการ implement
`Transport` ตัวใหม่ ไม่ใช่เขียน routing ใหม่

> [!note] **iMessage ใช้ได้เฉพาะ macOS และฟรี** ไม่มี server API — มันสั่งงาน **Messages.app บนเครื่อง** ที่ล็อกอินอยู่ (`osascript` เพื่อส่ง) และอ่าน `~/Library/Messages/chat.db` (อ่านอย่างเดียว) เพื่อรับ รันได้เฉพาะบน agent host ที่เป็น macOS; OS อื่นตัว connector จะ error ทันที เป็น MVP แบบ DM ก่อน ไม่สตรีม — ดีไซน์ใน `notes/2026-06-07_imessage-connector-design.md`

---

## การตั้งค่า

แต่ละ agent เลือกเปิดใช้ด้วยไฟล์ต่อแพลตฟอร์มในโฟลเดอร์ของตัวเอง:
`agents/agent-<name>/connectors/<platform>.toml`

```toml
# connectors/telegram.toml  (หรือ discord.toml)
schema_version = 3                     # ไม่บังคับ; ถ้าไม่ใส่ = legacy 2 (ยังโหลดได้)
enabled    = true
allow_from = [123456789, 987654321]   # user id ของแพลตฟอร์ม; ปิดโดยปริยาย

[group]                                # ไม่บังคับ — เชื่อมห้องกลุ่มเข้ากับทีม
team         = "tianting"              # team id ของ Saṅgha
mention_only = true                    # ตอบเฉพาะเมื่อถูก @mention

[bot]                                  # ไม่บังคับ — ถ้าไม่มี = ไม่ใช้อะไรข้างล่างเลย
rate_limit_per_min = 20                # ต่อผู้ส่ง (ปริยาย 20; 0 = ปิด เฉพาะคนใน allow-list)
max_input_chars    = 4000              # ข้อความที่ยาวเกินจะได้คำเตือน (ปริยาย 4000)
public             = false             # true = โหมดสาธารณะแบบจำกัด (ดูด้านล่าง)

[bot.commands]                         # คำตอบตายตัว; ไม่ถึง agent
"/start" = "Hi — I'm the tianting agent. Ask me anything."
"/help"  = "DM me, or @mention me in a group."
```

`schema_version` ใช้ได้กับไฟล์ connector ทุกไฟล์ revision ปัจจุบันคือ 3 ถ้าไม่ใส่
ไฟล์จะถูกอ่านเป็น revision 2 แบบ legacy คือยังโหลดได้ แต่มีคำเตือนที่ระบุ
`bwoc migrate` ซึ่งจะประทับ `schema_version = 3` ลงในไฟล์ ถ้าไฟล์ประกาศ revision ที่
ใหม่กว่าที่ build นี้รู้จัก connector จะไม่ยอมเริ่มทำงาน และ error จะระบุ
`schema_version` (fail closed)

**`[bot]`** ตรวจทุกข้อความที่จะไปถึง agent ตามลำดับนี้ ก่อนเริ่ม session ใด ๆ:

1. **เพดานอัตรา:** นับแยกต่อผู้ส่งในหน้าต่างเลื่อน 1 นาที เก็บในหน่วยความจำ
   ข้อความแรกที่เกินเพดานได้คำเตือนสั้น ๆ หนึ่งครั้ง ข้อความถัดไปถูกทิ้งเงียบ ๆ
   จนกว่าหน้าต่างจะว่าง
2. **คำสั่ง:** ถ้า token แรกตรงกับ key ใน `[bot.commands]` ทุกตัวอักษร บอตจะส่งข้อความ
   ตายตัวนั้นแล้วจบ ตัด suffix `@botname` ของ Telegram ออกก่อนเทียบ ไม่มี
   template และไม่รันอะไรทั้งสิ้น
3. **เพดานความยาว:** ข้อความที่ยาวเกินได้คำเตือนสั้น ๆ และไม่ถูกส่งต่อ

`AGENTS.md` ของ agent คือ persona ของบอต `[bot]` จึงไม่มีช่อง persona
(หนึ่ง agent = หนึ่งบอต)

```toml
# connectors/line.toml  — id ของ LINE เป็น string จึงเก็บ allow-list ไว้ตรงนี้
enabled = true

[line]
allow_user_ids = ["U1234..."]          # LINE user id; ปิดโดยปริยาย
bind           = "0.0.0.0:8080"         # ที่อยู่ของ webhook server ขาเข้า
path           = "/webhook"             # path ของ webhook (วาง HTTPS proxy ไว้ข้างหน้า)
```

```toml
# connectors/imessage.toml  — macOS เท่านั้น; handle เป็น string (เบอร์/อีเมล)
enabled = true

[imessage]
allow_handles = ["+15551234567", "friend@icloud.com"]  # ปิดโดยปริยาย
# db_path มีค่าปริยาย ~/Library/Messages/chat.db; แก้เฉพาะเมื่อจำเป็น
# poll_interval_secs = 2
```

> [!warning] **ปิดโดยปริยาย; `public = true` เปิดโหมดสาธารณะแบบจำกัด** allow-list ที่ว่างหรือไม่มี = **ไม่อนุญาตใครเลย** และผู้ส่งที่ไม่อยู่ในรายการจะถูกเพิกเฉยทั้งหมด (Sīla เหนือความครบถ้วน) ทางเดียวที่จะเปิดบอตคือตั้ง `[bot] public = true` เมื่อเปิดแล้ว ผู้ส่งที่ไม่อยู่ใน allow-list จะได้รับบริการ **เฉพาะ** ทาง DM หรือเมื่อ @mention บอตในกลุ่ม ข้อความอื่นในกลุ่มของคนเหล่านี้จะถูกทิ้งและไม่ถูกบันทึก ทุก turn แบบนี้ถูกจำกัดดังนี้:
>
> - session ถูกล็อกให้ใช้ได้เฉพาะ **เครื่องมืออ่านอย่างเดียว** (โหมด `plan` ของ harness: `read_file`, `list_dir`, `grep`, `memory_read`) การเขียน, `run_command`, git, การมอบงาน และเครื่องมือ MCP ถูกปฏิเสธ ถ้า harness ไม่ยืนยันโหมด session จะไม่ถูกสร้าง
> - เพดานอัตราและความยาวบังคับใช้เสมอ แม้ตั้งเป็น `0` สำหรับผู้ส่งสาธารณะ `0` จะกลับไปใช้ค่าปริยาย
> - session รันใน **workdir ของตัวเอง** คือ `<agent>/.bwoc/public/<platform>-<chat_id>/` ซึ่งมีแค่สำเนาของ `AGENTS.md` และ `config.manifest.json` (ตัด `deepMemoryCmd` ออก) กับไฟล์ session ของตัวเอง ไม่มี memories, connectors, skills หรือแชตอื่น และไม่เคยเข้า team chat เครื่องมือไฟล์ถูกจำกัดให้อยู่ในนั้นหลัง resolve symlink แล้ว link จึงพาออกไปข้างนอกไม่ได้
> - turn ยังถูกแท็ก `Principal::Platform` ซึ่งไม่น่าเชื่อถือ
>
> คนใน allow-list ทำงานเหมือนเดิมทุกอย่าง

### Token

Token **ไม่เคย** เก็บในไฟล์ config แต่ resolve ตามลำดับ:

1. **OS keyring** (macOS / Windows) — service `bwoc/<platform>`, account =
   basename ของโฟลเดอร์ agent
2. **Environment variable** — เส้นทางสำหรับ headless server (และเป็นเส้นทางเดียว
   บน Linux ซึ่งไม่มี keyring backend):
   - `TELEGRAM_BOT_TOKEN`
   - `DISCORD_BOT_TOKEN`
   - `LINE_CHANNEL_ACCESS_TOKEN` **และ** `LINE_CHANNEL_SECRET` (secret ใช้ตรวจ
     `X-Line-Signature` ของ webhook)

keyring ที่หายไปหรือถูกล็อกไม่ทำให้พังเด็ดขาด — มันจะตกไปใช้ env var แทน

**iMessage ไม่ใช้ token** เพราะมันสั่งงาน Messages.app บนเครื่อง แทนที่จะใช้ credential
มันต้องการ macOS **TCC grant** สองอย่างแบบครั้งเดียว: **Full Disk Access** (อ่าน
`chat.db`) และ **Automation → Messages** (ส่งผ่าน `osascript`) ถ้าขาดอย่างใดอย่างหนึ่ง
connector จะ error พร้อมข้อความชัดเจน แทนที่จะ poll เงียบ ๆ โดยไม่ได้อะไร

---

## การรัน

daemon ของ agent เป็นคน spawn และดูแล connector — คุณไม่ต้องรัน `bwoc-connect` เอง:

```bash
bwoc-agent --serve        # ในโฟลเดอร์ agent; ตรวจ connectors/*.toml ที่เปิดใช้
                          # spawn bridge, respawn เมื่อ crash, kill ตอน shutdown
bwoc status               # แสดงบรรทัด "Connectors" ต่อ bridge ที่รัน (platform · สถานะ · pid)
```

daemon ดูแลไบนารี `bwoc-connect` เป็น child — แบบเดียวกับ `bwoc-harness` — ดังนั้น
dependency เครือข่ายจึงไม่เข้า build ของ CLI / agent / core

---

## กลุ่มและทีม

เมื่อตั้ง `[group] team = "<id>"` ห้องกลุ่ม/ซูเปอร์กรุ๊ปจะเชื่อมกับ `chat.jsonl`
ร่วมแบบ append-only ของทีม Saṅgha นั้น (แกน team-chat ของ HV3-3a):

- ข้อความที่ **@mention บอต** (หรือทุกข้อความเมื่อ `mention_only = false`) จะถูก
  เสิร์ฟด้วย session แบบ `--team-chat` ซึ่ง inject ข้อความ peer ล่าสุดของห้อง แล้ว
  broadcast คำตอบกลับไปที่ห้อง
- ข้อความที่ **ไม่ mention** จะถูก append เข้า team chat เป็น peer context (แท็ก
  `tg:`/`dc:`/`ln:<id>`) เพื่อให้ agent เห็นบทสนทนาตอนถูกเรียกครั้งถัดไป — ไม่มีการตอบ
- ข้อความกลุ่มที่ **ไม่มี** team binding จะถูกเพิกเฉย

---

## การสตรีม

Telegram และ Discord **สตรีมคำตอบสด**: bridge ส่งข้อความ placeholder ตอน token แรก
แล้วแก้ข้อความเดิมขณะคำตอบยาวขึ้น โดย debounce ~1 ครั้ง/วินาที (ต่ำกว่าลิมิตการแก้ของ
ทั้งสองแพลตฟอร์ม) พร้อมแก้ครั้งสุดท้ายที่แสดงข้อความเต็มแน่นอน ส่วน LINE ไม่มี API
แก้ข้อความ จึงส่งคำตอบ **ครั้งเดียว** ตอนจบ turn — ทั้งหมดทำอัตโนมัติ (transport บอก
ผ่าน `supports_edit`)

---

## ท่าทีด้านความปลอดภัย

- **allow-list ปิดโดยปริยาย** คุมว่าใครถึง agent ได้ `[bot] public = true` เป็น
  ทางเดียวที่จะเปิด และ turn เหล่านั้นได้ session แยกที่อ่านอย่างเดียวและถูกจำกัด
  (ดูคำเตือนใต้ *การตั้งค่า*)
- session ของ harness ที่ถูกบริดจ์เป็น **non-TTY** ดังนั้นเครื่องมือโหมด `ask`
  จะ fail-safe เป็น **deny** และ `PermissionRequest` ถูกปฏิเสธอัตโนมัติ — ผู้ใช้แชต
  ระยะไกลอนุมัติ tool call ไม่ได้เด็ดขาด
- **แยก session ต่อแชต** แต่ละแชตที่ถูกบริดจ์มีไฟล์บทสนทนาของตัวเอง
  `<agent>/.bwoc/chat-sessions/<platform>-<chat_id>.json` (id ที่ไม่ใช่
  `[A-Za-z0-9_-]` ล้วนจะถูก hash) สองแชต — หรือ DM กับกลุ่ม — จึงไม่ปน context หรือ
  ประวัติกัน ส่วน `bwoc chat --tui` ยังใช้ `.bwoc/chat-session.json` เหมือนเดิม
- ทุก turn ที่ถูกบริดจ์ถูกแท็กตัวตนผู้ส่งบนแพลตฟอร์ม (`Principal::Platform`) ซึ่ง
  **ไม่น่าเชื่อถือ (untrusted)** เสมอ
- webhook ของ **LINE** ถูกตรวจลายเซ็น (`X-Line-Signature` = base64(HMAC-SHA256(
  channel secret, body)), แบบ constant-time) คำขอที่ไม่มีลายเซ็น/ปลอมถูกปฏิเสธ
- **iMessage** เปิด `chat.db` แบบ **อ่านอย่างเดียว** และ agent พูดในนาม **Apple ID ของ
  เครื่องเอง** — ไม่มีบัญชีบอตแยก คำตอบจึงดูเหมือนมาจากตัวคุณ การ automate Messages
  **ผิด ToS ของ Apple** (automation ส่วนตัวเท่านั้น) ให้มองว่าเป็นสะพานส่วนตัว ไม่ใช่
  แพลตฟอร์มบอตสาธารณะ

---

## ข้อจำกัด & ที่เลื่อนไว้

- **หนึ่งแพลตฟอร์มต่อ agent daemon** (config ที่เปิดใช้ตัวแรก)
- ข้อความ text เท่านั้น — ยังไม่รองรับสื่อ/ไฟล์แนบ
- คำตอบ LINE ที่เกินอายุ reply token แบบใช้ครั้งเดียว (~turn ที่ช้า) จะตกไปใช้ push
  ซึ่งนับโควตารายเดือนของ LINE; ตอบเร็วยังฟรี
- **Discord gateway RESUME** เลื่อนไว้ — การ reconnect ทำ re-IDENTIFY (ใช้งานได้
  เพียงไม่ใช่เส้นทาง resume ที่เบากว่า)
- **iMessage** เป็นแบบ DM ก่อนและ **ไม่สตรีม** (AppleScript แก้ข้อความที่ส่งไปแล้วไม่ได้)
  ห้องกลุ่มและการแก้/สตรีม (เส้นทาง private API ของ BlueBubbles / `imessage-rs`) เลื่อนไว้
  บน macOS รุ่นใหม่ตัวข้อความอาจอยู่ใน `attributedBody` แทน `text`; ตัว poll จะถอดรหัส
  แบบ best-effort และข้ามแถวที่ถอดไม่ได้ แทนที่จะส่งข้อความเพี้ยนออกมา

## ที่เกี่ยวข้อง

- [[PLUGINS]] — framework plugins (แกนส่วนขยายคนละแบบ)
- [[HARNESS]] — session `bwoc-harness --chat` ที่ connector ขับเคลื่อน
- `notes/2026-06-07_connect-subsystem-complete.md` และโน้ตต่อแพลตฟอร์ม
