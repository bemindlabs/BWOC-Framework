# 2026-06-08 — bwoc-mqtt broker authentication + channel standard

เพิ่มการรองรับ MQTT broker ที่เปิด authentication ให้ `bwoc-mqtt`, และกำหนด **มาตรฐาน channel (topic) กลาง** สำหรับการ route ข้าม workspace. ปิด issue #244.

## What changed

`crates/bwoc-mqtt/src/lib.rs`:
- `struct Broker` เพิ่ม `username: Option<String>` + `password: Option<String>`
- `parse_broker` รองรับ userinfo `mqtt://user[:pass]@host[:port]` — RFC 3986: `@` ตัวแรกแบ่ง userinfo ออกจาก host (literal `@` ในรหัสผ่านต้อง percent-encode)
- `mqtt_options` เรียก `MqttOptions::set_credentials` เมื่อมี username
- unit tests: เคส userinfo (user+pass / user-only); backward compatible — ไม่มี userinfo → anonymous เหมือนเดิม

เหตุผล: broker ที่ตั้ง `allow_anonymous false` + `password_file` จะปฏิเสธ bwoc-mqtt เดิม (ต่อ anonymous อย่างเดียว) — บล็อกการ link workspace ผ่าน central broker ที่ hardened แล้ว

## Channel (topic) standard — มาตรฐานกลาง

namespace ราก: **`bwoc/`** (สงวนไว้สำหรับ routing ของ BWOC ทั้งหมด)

| Channel | Topic pattern | ใช้ทำอะไร | สถานะ |
|---|---|---|---|
| **Inbox** (direct) | `bwoc/<agentId>/inbox` | ส่ง envelope ตรงถึง agent หนึ่งตัว → ต่อท้าย `inbox.jsonl` ของ agent นั้น | **GA** — เป็น default ของ `topic_for()` และ subscription filter `bwoc/+/inbox` ของ `serve` |
| Presence | `bwoc/<agentId>/presence` | heartbeat / online-offline | สงวน (ยังไม่ทำ) |
| Broadcast | `bwoc/broadcast` | ประกาศถึงทุก agent | สงวน (ยังไม่ทำ) |
| Team tasks | `bwoc/team/<teamId>/tasks` | สัญญาณ shared task list ของทีม | สงวน (ยังไม่ทำ) |

กติกา:
- **wire payload = envelope JSON บรรทัดเดียว** เหมือนที่ `bwoc send` เขียนลง `inbox.jsonl` — delivery ผ่าน MQTT กับ local-FS ได้ผลเหมือนกัน (ดู lib.rs docstring)
- field `to` ใน envelope = ตัวตัดสินผู้รับ (`recipient_from_envelope`) — ต้องตรงกับ topic
- QoS 1 (at-least-once); recipient ที่ workspace ไม่ได้ host → drop พร้อม log
- เพิ่ม channel ใหม่เมื่อ "จำเป็นจริง" เท่านั้น (Mattaññutā) — ตอนนี้ inbox ครอบคลุม use case หลัก

## Decisions

- **creds มาจาก URL userinfo** ไม่ใช่ flag ใหม่ — มาตรฐาน MQTT URL, เข้ากับ routes.toml ที่เก็บ broker URL อยู่แล้ว; ฝั่ง ops เก็บ URL เต็ม (มี creds) ใน `~/.bwoc/secrets.toml` ไม่ commit
- backward compatible — pure addition, ของเดิม (anonymous) ไม่กระทบ

## Status / deferred

- presence / broadcast / team-tasks channels = สงวน namespace ไว้ ยังไม่ implement
- routes.toml schema สำหรับ per-peer transport=mqtt + topic override = งานต่อไป

## Related

- Issue #244 — bwoc-mqtt cannot connect to authenticated brokers
