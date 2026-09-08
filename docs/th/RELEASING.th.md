---
title: การปล่อย Release BWOC
aliases:
  - Release Process
tags:
  - group/framework
  - type/process
  - meta/operations
parent: ภาษาไทย
nav_order: 7
---

# การปล่อย Release BWOC

> [!abstract] วิธีปล่อยรีลีสของชุดเครื่องมือ bwoc (`bwoc` CLI + `bwoc-agent` daemon + `bwoc-harness` agentic loop) — Release ใช้ tag เป็นตัวกระตุ้น: push tag แบบ CalVer เช่น `v2026.5.22-0` แล้ว pipeline จะ build ทุก platform และ upload ไปยัง GitHub Release โดยอัตโนมัติ

## Dual versioning — ต้องอ่านก่อน

BWOC ใช้ **เวอร์ชัน 2 ระบบโดยตั้งใจ**:

| Namespace | รูปแบบ | อยู่ที่ไหน | บทบาท |
|---|---|---|---|
| Cargo SemVer | `0.1.405` | `Cargo.toml` workspace + `Software-Version` | จุดตรวจระหว่างพัฒนา (internal dev checkpoint) auto-bump ทุกครั้งที่ Claude Code แก้ `.rs` / `.toml` |
| Release CalVer | `v2026.5.22-0` | Git tag, GitHub Release, ชื่อ asset | **public release identity** Tag กระตุ้น `release.yml` |

นโยบายเต็มดู [`VERSION.md`](../../VERSION.md) §"Versioning Policy — Dual Namespaces" — สรุป: Cargo SemVer คือจุดตรวจระหว่างพัฒนา ชื่อ release สำหรับสาธารณะคือ CalVer

## ก่อน tag

ก่อนกด tag — ผู้ดูแลควรตรวจสอบ:

- [ ] **CI ผ่าน** บน `main` ของ commit ล่าสุด — ดูที่ [Actions → CI](https://github.com/bemindlabs/BWOC-Framework/actions/workflows/ci.yml)
- [ ] **`CHANGELOG.md`** มี section สำหรับ CalVer tag ที่จะกด เปลี่ยน `[Unreleased]` เป็น `[v2026.5.22-0] — 2026-05-22` แล้วสร้าง `[Unreleased]` ใหม่ที่ว่างเปล่าด้านบน
- [ ] **`VERSION.md`** auto-update ทุกครั้งที่แก้ไฟล์อยู่แล้ว ไม่ต้องแก้เอง
- [ ] **ไม่มีการแก้ที่ยัง uncommit** — release artifact ควรสะท้อน tree ที่สะอาด

## กด tag

เลือก CalVer tag สำหรับวันนี้ — `vYYYY.M.D-<patch>` โดย patch เริ่มที่ 0 และเพิ่มขึ้นทุกครั้งที่ re-issue ในวันเดียวกัน:

```bash
git tag v2026.5.22-0
git push origin v2026.5.22-0
```

Tag จะตรงกับ filter ของ workflow (`v[0-9][0-9][0-9][0-9].*`) และกระตุ้น [`.github/workflows/release.yml`](../../.github/workflows/release.yml):

1. **Build แบบ matrix** — 5 target ทำงานพร้อมกัน:
   - `x86_64-unknown-linux-gnu` (Linux x64)
   - `aarch64-unknown-linux-gnu` (Linux ARM64, native บน runner `ubuntu-24.04-arm`)
   - `aarch64-apple-darwin` (macOS Apple Silicon)
   - `x86_64-apple-darwin` (macOS Intel)
   - `x86_64-pc-windows-msvc`
2. **Package** แต่ละตัวเป็น `bwoc-<tag>-<target>.{tar.gz|zip}` มี `bwoc`, `bwoc-agent`, `bwoc-harness`, `README.md`, `LICENSE`, `CHANGELOG.md` — ขั้นตอน package จะ assert ว่ามีครบทั้งสาม binary ไม่ครบคือ build fail (#460)
3. **Sidecar** ไฟล์ `.sha256` ข้าง archive แต่ละตัว
4. **สร้าง GitHub Release อัตโนมัติ** พร้อม note จาก commit range ตั้งแต่ tag ก่อนหน้า
5. **Upload** artifact ทั้งหมด — `fail_on_unmatched_files: true` ทำให้ workflow ล้มเหลวถ้ามี archive ขาด — release ที่ไม่ครบไม่ปล่อย

## Re-issue วันเดียวกัน

เพิ่มเลข patch ไม่ใช่วัน:

```
v2026.5.22-0    # release แรกของวัน
v2026.5.22-1    # re-issue (เช่น artifact เสีย แก้ forward)
v2026.5.22-2    # re-issue ที่สอง
```

วิธีนี้ทำให้วันคงที่ในขณะที่ iteration ชัดเจน

## Prerelease vs stable

CalVer tag **มี** `-<patch>` เสมอ ดังนั้น workflow ตรวจจับ prerelease จากรูปร่าง tag อัตโนมัติไม่ได้ (อย่างที่ SemVer tag เช่น `v0.1.0-rc1` ทำได้) ทุก CalVer release จึงถูกถือว่า **stable** เป็น default — กดสวิตช์ "Set as a pre-release" ของ GitHub Release ด้วยมือสำหรับ build ที่ทดลองจริง ๆ

ในทางปฏิบัติแทบไม่ต้องทำ — same-day patch bump ครอบคลุมเคส "ปล่อยอะไรเร็ว ๆ" ส่วนใหญ่โดยไม่ต้อง label prerelease

## การตัด major

MAJOR release ไม่ใช่ tag ที่ใหญ่ขึ้น แต่คือชุดคำสัญญาที่ถึงกำหนดชำระ ก่อน bump version
ทุกข้อด้านล่างต้องเป็นจริง ส่วนนิยามว่าอะไรทำให้ release เป็น major อยู่ใน
`VERSION.md` §Cargo SemVer bump rules

1. **breaking change ทุกข้อถูกระบุชื่อ** ใน `CHANGELOG.md` ใต้ release นั้น พร้อมบอกว่าอะไรมาแทน
2. **เส้นทาง migration มีอยู่จริงและถูกทดสอบ** — `bwoc migrate` ครอบทุก format บนดิสก์
   ที่เปลี่ยน, `crates/bwoc-cli/tests/migrate_roundtrip.rs` พิสูจน์ว่ามัน round-trip
   โดยไม่ทำ key ที่ไม่ได้ model ไว้หาย และ `docs/{en,th}/MIGRATION-<X.0>.*`
   พาผู้ดูแลเดินผ่านทีละขั้น
3. **schema เดิมยังอ่านได้** — เหลื่อมกันหนึ่ง major
   ([`COMPATIBILITY.th.md`](COMPATIBILITY.th.md#หน้าต่างการรองรับ-support-window))
   binary ใหม่อ่าน artifact เก่าได้ เตือน และบอกชื่อคำสั่ง migrate
4. **deprecation ที่สัญญาไว้ว่าจะลบใน major นี้ ถูกลบจริง** และ warning ที่สัญญาไว้
   สำหรับ major *ถัดไป* ถูกวางไว้แล้ว
5. **`compat` range ของปลั๊กอินถูกประกาศใหม่** เป็น major ใหม่ทั่วทั้ง
   `modules/**/manifest.toml` แบบมีขอบบน ทำในคอมมิตเดียวกับการ bump version:
   range ที่ผูกกับเวอร์ชันที่ยังไม่ลงจะทำให้ปลั๊กอินทุกตัวถูกปฏิเสธบน `main` ระหว่างทาง
6. **ตัวชี้ทุกจุดตรงกัน** — `crates/bwoc-cli/tests/release_pointers.rs` gate
   `Cargo.toml`, `README.md`, `VERSION.md` และ `Formula/bwoc.rb` กับ entry บนสุดของ
   CHANGELOG และ `whats_new.rs` จะ fail ถ้าไม่มี bullet ใน `HIGHLIGHTS` ที่อ้าง
   MAJOR.MINOR ใหม่ ทั้งหมดต้องลงในคอมมิตเดียว ไม่งั้น CI แดงระหว่างทาง
7. **`VERSION.md` §Specification** ตรงกับแถว `| **Version** |` ใน
   `modules/agent-template/AGENTS.md`

## สิ่งที่ยังไม่อยู่ใน pipeline

- **Code signing** — Apple notarization (macOS) และ Windows Authenticode ยังไม่ได้ตั้งค่า binary ปล่อยแบบไม่ signed พร้อม SHA-256 checksum ผู้ใช้จะเห็น prompt "untrusted developer" ตอน launch ครั้งแรก การเพิ่ม signing ต้องให้ maintainer จัดการ cert และเก็บ key ใน GitHub Actions secrets
- **Linux musl** — `aarch64-unknown-linux-gnu` ship แล้ว; `x86_64-unknown-linux-musl` (และ `aarch64-unknown-linux-musl`) เพิ่มเข้า matrix ได้เมื่อมีความต้องการสำหรับ distro ที่ไม่มี glibc (Alpine, distroless container)
- **Scoop manifest / cargo binstall metadata** — distribution อยู่ใน ecosystem ของตัวเอง (Homebrew ไม่อยู่ในรายการนี้แล้ว: `release.yml` มี job `bump-formula` ที่เปิด branch อัปเดต formula หลังทุก tag และ `Formula/bwoc.rb` ติดตั้ง `bwoc`, `bwoc-agent`, `bwoc-harness`)

## Rolling back

ถ้า release ที่ tag ไปแล้วมี artifact เสีย:

1. **อย่าลบ** tag — GitHub Release ยังเก็บ binary ที่เสียอยู่ และผู้ใช้อาจดาวน์โหลดไปแล้ว same-day re-issue มีอยู่เพื่อให้ timeline ตรวจสอบได้
2. กด same-day patch ใหม่ (เช่น `v2026.5.22-1` หลังจาก `v2026.5.22-0`) ที่มีการแก้
3. แก้ release note ของ release ที่เสียให้ชี้ไปยังตัวที่มาทดแทน

การเรียง CalVer แบบ monotonic ทำให้ rollback ง่าย — patch suffix สูงสุดของวันล่าสุดคือ build "ปัจจุบัน" canonical

## ดูเพิ่ม

- [`.github/workflows/release.yml`](../../.github/workflows/release.yml) — workflow ที่เอกสารนี้อธิบาย
- [`.github/workflows/ci.yml`](../../.github/workflows/ci.yml) — gate ของแต่ละ commit ต้องเป็นสีเขียวก่อนกด tag
- [`CHANGELOG.md`](../../CHANGELOG.md) — สิ่งที่ต้อง update ก่อน tag
- [`VERSION.md`](../../VERSION.md) — เวอร์ชันปัจจุบัน, dual-namespace policy, กฎ manual bump
- [`COMPATIBILITY.th.md`](COMPATIBILITY.th.md) — release ทำอะไรพังได้บ้าง และหน้าต่างการรองรับ
- [`ROADMAP.th.md`](ROADMAP.th.md) — phase (ไม่กำหนดเวอร์ชัน)
