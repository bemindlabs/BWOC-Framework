---
title: ระบบดีไซน์
parent: ภาษาไทย
nav_order: 14
---

# Design System — token สำหรับ user interface ของ BWOC

source of truth เดียวคือ **`bwoc-core::design`** (`crates/bwoc-core/src/design.rs`) มี UI สามตัวที่ใช้: `bwoc dashboard` (ratatui), `bwoc chat --tui` (ratatui, crate `bwoc-tui`) และ desktop chat (`bwoc-chat`, egui) ก่อนมี token แต่ละตัว hardcode palette ของตัวเองจน drift — สีเหลืองมีสามความหมายบนจอเดียว, ตัวอักษร "muted" บางจุดแทบมองไม่เห็น และ activity สองสถานะใช้ glyph ตัวเดียวกัน

token เป็น **plain data** (ไม่มี type ของ ratatui/egui) เพื่อให้ `bwoc-core` ยัง dependency-lean และ frontend ใดก็ใช้ได้

## หลักการ

1. **Redundant coding** — สถานะต้องไม่พึ่งสีอย่างเดียว: ทุก status จับคู่ *glyph ที่ต่างกัน* กับ label ชุด glyph ของ activity ต่างกันเป็นคู่ ๆ ทุกตัว มี unit test คุม
2. **Signal economy** (มัตตัญญุตา) — ศูนย์แสดงเป็น `—`; attention indicator ปรากฏเฉพาะเมื่อมีค่า แสดงเฉพาะสิ่งที่สำคัญ
3. **หนึ่งสีหนึ่งความหมายต่อจอ** — selection ห้ามใช้ hue เดียวกับ idle/title (มี test คุม)
4. **เคารพ theme** — terminal UI ใช้ครึ่ง `ansi` ของ token (สี ANSI แบบมีชื่อ → theme ของ terminal ผู้ใช้กำหนดเฉดจริง); เฉพาะ pixel UI (egui) ใช้ครึ่ง `rgb`

## Colour token (`design::color`)

แต่ละ token คือ `ColorToken { ansi, rgb }` — เลือกตาม **บทบาท** ไม่ใช่ตามสี

| Token | บทบาท | ANSI | RGB |
|---|---|---|---|
| `ACCENT` | accent ของแบรนด์/การโต้ตอบ — border ของ pane ที่ active, key label, ตัวนับ | Cyan | `53C2D6` |
| `TITLE` | ชื่อผลิตภัณฑ์ / หัว banner | Yellow | `E0C060` |
| `SELECTION_BG` / `SELECTION_FG` | แถวที่เลือก (ตั้งใจ **ไม่ใช้** เหลือง) | Blue / White | `2D5B9E` / `F5F5F5` |
| `WORKING` | session กำลังทำงานจริง | Green | `9EE093` |
| `IDLE` | session ยังอยู่แต่ไม่มี output ล่าสุด | Yellow | `E0C060` |
| `RUNNING` | process ทำงานอยู่ (ต่างจาก WORKING) | Cyan | `53C2D6` |
| `STALE` | มี marker แต่ process หายไป | Gray | `9A9A9A` |
| `MUTED` | ลดความเด่นแต่ยังอ่านได้ (ต่ำสุดที่ Gray ไม่ใช้ DarkGray) | Gray | `9A9A9A` |
| `SUCCESS` / `WARNING` / `DANGER` | ผลลัพธ์ | Green / Yellow / Red | `9EE093` / `E0C060` / `E09090` |
| `USER` / `SYSTEM` | บทบาทใน chat transcript | Blue / Gray | `6CB6FF` / `9A9A9A` |

## Glyph token (`design::glyph`)

| Token | Glyph | ความหมาย |
|---|---|---|
| `ACTIVITY_WORKING` | `◉` | กำลังทำงานจริง |
| `ACTIVITY_IDLE` | `◑` | ยังอยู่ ไม่มี output ล่าสุด |
| `ACTIVITY_RUNNING` | `●` | process ทำงานอยู่ |
| `ACTIVITY_STALE` | `○` | มี marker แต่ process หาย |
| `ACTIVITY_NONE` | `—` | ไม่มี session |
| `RUNTIME_ALIVE` / `RUNTIME_DEAD` | `●` / `○` | สถานะ daemon |

## ระยะห่างและ typography (`design::space`)

| Token | ค่า | ความหมาย |
|---|---|---|
| `MESSAGE_GAP` | `8.0` | ช่องว่างแนวตั้งระหว่างข้อความใน transcript (egui points) |
| `LINE_HEIGHT_FACTOR` | `1.4` | line height ÷ font size — เผื่อที่ให้สระ/วรรณยุกต์ไทยที่ซ้อนบนล่าง |

## วิธีใช้ token

**ratatui** — map ครึ่ง `ansi` เป็นสีแบบมีชื่อ เพื่อให้ theme ของ terminal มีผล (แต่ละ TUI มี `tone()` mapper ~12 บรรทัด):

```rust
use bwoc_core::design;
fn tone(t: design::ColorToken) -> Color { match t.ansi { Ansi::Cyan => Color::Cyan, /* … */ } }
// เช่น
Style::default().fg(tone(design::color::ACCENT))
```

**egui** — ใช้ครึ่ง `rgb` ตรง ๆ:

```rust
let (r, g, b) = design::color::USER.rgb;
egui::Color32::from_rgb(r, g, b)
```

เปลี่ยน palette = แก้ไฟล์เดียวที่ `design.rs`; invariant test (glyph ต่างกันเป็นคู่, selection ≠ hue ของ idle/title, muted ≠ DarkGray) คุมหลักการไว้

## ดูเพิ่ม

- `crates/bwoc-core/src/design.rs` — token + invariant test
- [`HARNESS.th.md`](HARNESS.th.md) — runtime เบื้องหลัง chat UI
