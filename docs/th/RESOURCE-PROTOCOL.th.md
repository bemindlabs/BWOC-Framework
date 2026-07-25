---
title: Resource Protocol — แชร์ Compute & Memory ข้ามฟลีต
aliases:
  - Resource Protocol
  - BRP
  - Fleet Resource Sharing
tags:
  - group/protocol
  - type/design
  - meta/framework
status: draft (v2026.7.25 — ship slice A–C: snapshot + gate-check + client advertise/discover + gateway broker (ครึ่ง discovery); claim/lease + offload เลื่อนไป slice D)
canonical-source: DN 16 (Mahāparinibbāna Sutta) §1.4 — Aparihāniya-dhamma 7 ข้อ 6 (เคารพทรัพยากรส่วนรวม)
parent: ไทย
nav_order: 11
---

# Resource Protocol — แชร์ Compute & Memory ข้ามฟลีต

> [!abstract] **lease** (สัญญายืม) ที่เซ็นชื่อและมีอายุจำกัด บนทรัพยากรที่มี type ชัดเจน โดยมี `bwoc-gateway` relay เป็นตัวกลางข้ามฟลีต เครื่องเบา (โน้ตบุ๊ก) ยืม compute (GPU/CPU), working memory (RAM, shared KV/context), หรือ knowledge (federate `.bwoc/memory` / RAG แบบอ่านอย่างเดียว) จากเครื่องหนัก (เซิร์ฟเวอร์ GPU) — ภายใต้ **sharing gate** แบบ opt-in ฝั่ง provider เพื่อไม่ให้ทรัพยากรของใครถูกใช้โดยไม่ยินยอม. นี่คือ [Fleet Governance](FLEET-GOVERNANCE.th.md) ข้อ 6 — *เคารพทรัพยากรส่วนรวม* — ที่ทำให้ใช้งานได้จริง

## ทำไมต้องมี

เครื่องบ้านของ agent มักไม่ใช่เครื่องที่เหมาะกับทุกงาน. agent บนโน้ตบุ๊กที่ต้องรัน inference 14B, encode วิดีโอ, หรือถือ dataset 40 GB ใน RAM มี *เจตนา* อยู่ในเครื่องแต่ไม่มี *ซิลิคอน*. ที่ไหนสักแห่งในฟลีตมีเซิร์ฟเวอร์ GPU นั่งว่าง. วันนี้การย้ายงานไปที่นั่นต้อง SSH เฉพาะกิจ, hardcode hostname, และไม่มีการบันทึกว่าใครใช้อะไร

framework มีชิ้นส่วนสำหรับ *dispatch* งานข้ามเครื่องอยู่แล้ว — A2A (`bwoc a2a`, เรียกงาน agent-to-agent), `bwoc remote` (รันผ่าน host `bwocd`), และ [gateway](https://github.com/bemindlabs/bwoc-gateway) (relay envelope ที่เซ็นชื่อ ทะลุ NAT). ที่ขาดคือ layer *เหนือ* dispatch: **เครื่องไหนมีทรัพยากรว่างตอนนี้ ด้วยเงื่อนไขอะไร แล้วยืมยังไงไม่ให้เหยียบเท้าใคร.** นั่นคือ Resource Protocol

ข้อจำกัดการออกแบบ v1 สามข้อ:

1. **ยินยอมก่อน ปฏิเสธเป็นค่าเริ่มต้น.** host ไม่แชร์อะไรจนกว่า operator จะ opt-in (`[resource] share = true`) และประกาศ cap. การยืมไม่เกิดขึ้นโดยปริยาย (Sīla — sharing gate รูปแบบเดียวกับ financial-write และ IAM gate)
2. **Lease ไม่ใช่ session.** ทุกการให้มีอายุจำกัด และถูก release หรือ expire อย่างชัดเจน ไม่มี lease ไหนอยู่เกิน `ttl` (Anattā — ไม่ยึดการถือครอง; Aniccatā — ทุกอย่างถูกทวงคืน)
3. **Broker โง่และไม่ถูกไว้ใจ.** gateway จับคู่ offer กับ claim และ relay envelope ที่เซ็นชื่อ ไม่เคยรันงานและไม่เคยเห็น credential plaintext. ความไว้ใจอยู่ที่ลายเซ็น ed25519 ไม่ใช่ที่ relay (หลักการเดียวกับ relay design เดิมของ gateway)

## Actors

| Actor | ใคร | บทบาท |
|---|---|---|
| **Provider** | host `bwocd` ที่ยอมแชร์ (เซิร์ฟเวอร์ GPU) | โฆษณา **snapshot** ทรัพยากร, ประเมิน **claim** ตาม sharing gate, host ทรัพยากรที่ถูก lease |
| **Consumer** | agent/host ที่ต้องการทรัพยากร (โน้ตบุ๊ก) | discover offer, claim, ใช้ lease, release |
| **Broker** | `bwoc-gateway` relay | เก็บ registry ของ offer ที่ยัง live, จับคู่ query `discover`, forward `claim` ไป provider, relay traffic ของ lease. ไม่รันอะไร |

## Resource kinds

หนึ่ง protocol หนึ่ง lease lifecycle สาม resource kind ที่มี type:

| Kind | ยืมอะไร | เบื้องหลัง | Slice |
|---|---|---|---|
| `compute` | **งาน** ที่ต้องใช้ GPU/CPU ที่ consumer ไม่มี (LLM inference, video/3D gen, build หนัก). RAM ที่งานต้องใช้เป็น *constraint* บน compute claim ไม่ใช่ kind แยก | provider รันงาน (ผ่าน A2A / `bwocd` task exec) ภายใต้ lease แล้วส่งผลกลับ | C |
| `kv` | **key/value store** ที่แชร์ — working state / context ที่ agent สองตัวคนละเครื่องอ่านเขียนร่วมกัน (scratchpad กระจาย, handle KV-cache, map ประสานงาน) | provider host store ที่ scoped ตาม namespace + lease; consumer อ่านเขียนผ่าน lease | C |
| `knowledge` | federate knowledge-memory ของฟลีต **แบบอ่านอย่างเดียว** — query `.bwoc/memory` / notes / RAG index ของทุก host ที่เข้าถึงได้แล้วรวมคำตอบ | provider ตอบ query ต่อ knowledge ในเครื่อง ไม่มี state ถูกยืม อ่านอย่างเดียว | C |

`compute` เป็น kind หลักและ ship ก่อน (หลัง broker). `kv` และ `knowledge` reuse lifecycle advertise → discover → claim → lease → release เดียวกันเป๊ะ ต่างแค่ payload `spec`/`use`

## Lease lifecycle

```
 provider                         broker (gateway)                    consumer
    |                                   |                                 |
    |-- RES.ADVERTISE {snapshot,ttl} -->|   (heartbeat ทุก N วิ)          |
    |                                   |<-- RES.DISCOVER {kind,min} ------|
    |                                   |--- offers[] ------------------->|
    |                                   |<-- RES.CLAIM {offer_id,spec} ----|
    |<-- RES.CLAIM (forwarded) ---------|                                 |
    |  [sharing gate: accept/deny]      |                                 |
    |-- RES.LEASE {lease_id,ep,exp} --->|--- RES.LEASE ------------------>|
    |                                   |                                 |
    |<===== USE (job / kv / query, auth ด้วย lease token) ===============>|
    |                                   |                                 |
    |                                   |<-- RES.RELEASE {lease_id} -------|
    |  [reclaim] (หรือ auto-expire @ exp)|                                 |
```

- **ADVERTISE** — provider โพสต์ [snapshot](#resource-snapshot) ปัจจุบัน + `ttl`; broker เก็บไว้แบบ live และทิ้งเมื่อ `ttl` หมดโดยไม่ refresh (provider ที่ crash จะ self-evict). เป็น heartbeat ไม่ใช่ one-shot
- **DISCOVER** — consumer ถาม broker หา offer ของ `kind` ที่ผ่าน `min_spec` (เช่น `gpu.vram_free ≥ 24 GB`). broker คืน offer ที่ live และ match เรียง best-fit ก่อน ไม่มี side effect
- **CLAIM** — consumer claim offer เจาะจงด้วย `spec` ที่เป็นรูปธรรม (งาน / kv namespace / query scope ที่แน่นอน). broker forward ไป provider. provider ประเมิน [sharing gate](#sharing-gate) แล้วออก lease หรือปฏิเสธพร้อมเหตุผล (Dhammānupassanā — การปฏิเสธบอก *ทำไม* ไม่ใช่ drop เงียบ)
- **LEASE** — เมื่อรับ provider mint `Lease { lease_id, kind, endpoint, granted_to, spec, expires_at }` ที่เซ็นชื่อ. `lease_id` (+ ลายเซ็น provider เหนือมัน) คือ bearer credential สำหรับ USE
- **USE** — consumer ทำงานกับทรัพยากรที่ lease โดยตรง (endpoint ของ provider) หรือ relay ผ่าน gateway เมื่อไม่มีเส้นทางตรง. ทุก USE request พก lease token; provider ปฏิเสธ lease ที่ expire หรือไม่รู้จัก
- **RELEASE / EXPIRE** — consumer release เอง หรือ provider reclaim ที่ `expires_at`. lease ที่ release/expire ตายแล้ว; USE ต่อ fail closed

## Resource snapshot

หน่วยที่ provider โฆษณา. สร้างในเครื่อง ไม่มี network:

```json
{
  "host": "bemind",
  "agent_id": "agent-busaba",
  "gpus": [
    { "index": 0, "model": "NVIDIA RTX A6000", "vram_total_mb": 49140, "vram_free_mb": 40320, "util_pct": 12 }
  ],
  "cpu_cores": 128,
  "cpu_load1": 8.4,
  "ram_total_mb": 128000,
  "ram_free_mb": 96000,
  "services": ["ollama", "wan-i2v"],
  "sampled_at": "2026-07-25T07:00:00Z"
}
```

- ฟิลด์ **GPU** มาจาก `nvidia-smi --query-gpu=index,name,memory.total,memory.free,utilization.gpu --format=csv,noheader,nounits`; ไม่มี `nvidia-smi` ⇒ `gpus: []` (host CPU-only ก็ยังโฆษณา `compute` ได้)
- **CPU / RAM** — `cpu_cores` จาก `std::thread::available_parallelism`; `ram_total_mb` / `ram_free_mb` + `cpu_load1` (load average 1 นาที) จาก Linux `/proc` (`/proc/meminfo` `MemAvailable`, `/proc/loadavg`) ใน slice A. `ram_free_mb` คือ memory ที่ available (reclaim ได้) ไม่ใช่แค่ที่ยังไม่ใช้. host ที่ไม่ใช่ Linux รายงาน `0` / "unavailable" จนกว่า backend platform (`sysctl` / `sysinfo`) จะมา
- **`agent_id` และ `services` เป็นฟิลด์ตอน advertise** ไม่ใช่ส่วนของ local probe. `bwoc resource snapshot` ใน slice A ปล่อยเฉพาะ subset ที่ probe จาก host — `host`, `gpus`, `cpu_cores`, `cpu_load1`, `ram_total_mb`, `ram_free_mb`, `sampled_at`. `agent_id` (มาจาก workspace) และ `services` (allow-list ที่ operator ประกาศ ของ capability ที่ตั้งชื่อซึ่ง host เปิด เช่น endpoint `ollama` — advisory สำหรับ filter ตอน discover) ถูกแนบเมื่อ snapshot ถูก *advertise* (slice B)
- snapshot เป็น **คำบรรยาย ไม่ใช่คำสัญญา.** คำสัญญาที่ผูกมัดคือ lease ที่ provider mint ตอน claim ซึ่งประเมินต่อ state สด — snapshot เก่าไม่มีทางให้เกินจริงได้

## Sharing gate

provider ไม่แชร์ **อะไรเลย** จนกว่า operator จะ opt-in. ใน `.bwoc/workspace.toml`:

```toml
[resource]
share = true                      # สวิตช์หลัก refuse-by-default
gateway = "wss://gw.bemind.tech"  # broker ที่จะ advertise ไป

[resource.caps]
max_vram_mb   = 40000             # ไม่ lease compute claim ที่ต้องการ VRAM ว่างเกินนี้
max_ram_mb    = 64000             # cap RAM ที่ compute/kv lease เดียวจองได้
max_cpu_cores = 96                # cap core ต่อ lease
max_leases    = 4                 # จำนวน lease พร้อมกันที่ host นี้จะถือ
allow         = ["agent-anna", "agent-qianliyan"]  # ว่าง ⇒ อนุญาต peer ในฟลีตที่ enrolled ใด ๆ
kinds         = ["compute", "knowledge"]           # kind ที่ host นี้เปิดให้
```

การประเมิน gate ทุก CLAIM (ต้องผ่านทุกข้อ ไม่งั้น deny):

1. `share = true` — opt-in หลัก. ไม่มี/false ⇒ ทุก claim ถูก deny
2. `kind` ของ claim อยู่ใน `caps.kinds`
3. consumer อยู่ใน `caps.allow` (หรือ `allow` ว่าง ⇒ peer ที่ enrolled ใด ๆ ตาม enrollment เดิมของ gateway)
4. `spec` ของ claim พอดีกับ cap (`vram ≤ max_vram_mb`, `ram ≤ max_ram_mb`, `cores ≤ max_cpu_cores`) **และ** snapshot สดมีว่างจริง
5. การให้จะไม่เกิน `max_leases`

การ deny คืนเหตุผลที่มี type (`not_sharing`, `kind_not_offered`, `not_allowed`, `over_cap`, `insufficient_free`, `lease_limit`) — ไม่ใช่ drop เงียบ. นี่คือ [Fleet Governance §6](FLEET-GOVERNANCE.th.md) (*cetiya* — เคารพทรัพยากรส่วนรวม): cap ของ operator คือกฎของศาลเจ้า และ gate บังคับใช้มัน

## Wire format

ข้อความ resource เป็น **signed envelope** ของ `bwoc-gateway` — รูปแบบเดียวกับที่ relay รับส่งอยู่ ที่ auth ด้วย ed25519 พร้อม body resource:

```json
{
  "v": 1,
  "type": "RES.CLAIM",
  "sender": "agent-anna",
  "recipient": "agent-busaba",
  "sent_at": "2026-07-25T07:00:01Z",
  "nonce": "…",
  "body": { "offer_id": "…", "kind": "compute", "spec": { "gpu_vram_mb": 24000, "job": { … } } },
  "signature": "<hex ed25519 เหนือ canonical bytes>"
}
```

- `type ∈ { RES.ADVERTISE, RES.DISCOVER, RES.OFFERS, RES.CLAIM, RES.LEASE, RES.DENY, RES.RELEASE }`
- การเซ็น/canonicalization เหมือนกับ message relay ของ gateway (reuse `cc-signing`); broker verify ลายเซ็น sender ก่อน mutate registry. consumer verify ลายเซ็น **ของ provider** บน `RES.LEASE` ที่คืนมา ก่อนไว้ใจ endpoint
- route ใหม่ของ gateway (slice B): `POST /v1/resource/advertise`, `POST /v1/resource/discover`, `POST /v1/resource/claim`, `POST /v1/resource/release`. registry ของ broker อยู่ใน memory และ evict ตาม TTL; เป็น cache ของ offer ที่ live ไม่ใช่ system of record

## CLI surface

```
bwoc resource snapshot                      # print ResourceSnapshot ของ host นี้ (READ; local; ไม่มี network)  ── slice A
bwoc resource advertise [--ttl 30]          # เริ่ม heartbeat ADVERTISE ไป gateway ที่ตั้งไว้                     ── slice B
bwoc resource discover --kind compute \      # query broker หา offer ที่ match
                        --gpu-vram 24000
bwoc resource claim <offer-id> --spec <json> # CLAIM → LEASE (sharing-gate ฝั่ง provider ทำงาน)                 ── slice C
bwoc resource release <lease-id>            # RELEASE lease ที่ถือ
bwoc resource status                        # local: lease ที่ active ของฉัน (ถือ + ให้)
bwoc resource kv get|set <ns> <key> [val]   # USE lease `kv`                                                    ── slice C
```

read (`snapshot`, `discover`, `status`) ฟรี. `advertise` mutate มุมมองของ host นี้บน broker และต้องมี `[resource] share = true`. `claim` ใช้ทรัพยากรของ host อื่น — *provider* เป็นคน gate; consumer แค่ต้องมี key ที่ enrolled

## Security model

- **ยินยอมสองทาง.** sharing gate ฝั่ง provider คุมสิ่งที่ออกไป; การ verify ลายเซ็น lease ฝั่ง consumer คุมสิ่งที่ไว้ใจ
- **ไม่มี secret plaintext บน wire.** credential ของงาน `compute` เอง (เช่น API key ที่งานต้องใช้) ไม่ถูกใส่ใน claim; มัน resolve บน provider จาก `.bwoc/secrets` ของ provider เอง หรือส่งมา out-of-band. protocol พก *รูปทรง* ทรัพยากร ไม่ใช่ secret
- **Fail closed.** lease ไม่รู้จัก, lease expire, ลายเซ็น verify ไม่ได้, หรือ gate ไม่ผ่าน ทั้งหมด deny. ไม่มีเส้นทาง "allow on error"
- **Blast radius จำกัด.** lease ให้ทรัพยากรหนึ่งชิ้นของหนึ่ง kind ภายใต้ cap ที่ประกาศ; ไม่ใช่ shell. งาน `compute` รันใน task-exec sandbox เดิมของ provider — lease ไม่ขยายมัน
- **ตรวจสอบย้อนหลังได้.** ทุก ADVERTISE/CLAIM/LEASE/RELEASE เป็น signed envelope; provider log การให้ + การ reclaim. ใครยืมอะไร เมื่อไหร่ ภายใต้ลายเซ็นของใคร สร้างใหม่ได้

## Philosophy grounding

| การตัดสินใจ | หลักธรรม |
|---|---|
| sharing gate refuse-by-default, cap ของ operator | **Sīla** (gate) + **Fleet Governance §6** (*cetiya*, เคารพทรัพยากรส่วนรวม) |
| lease มีอายุจำกัด, release ชัดเจน, auto-expire | **Aniccatā** (อนิจจัง) + **Anattā** (ไม่ยึดการถือครอง) |
| broker โง่ ไม่ถูกไว้ใจ; ความไว้ใจอยู่ที่ลายเซ็น | **Yoniso Manasikāra** (ไว้ใจสิ่งที่ verify ไม่ใช่สิ่งที่อ้าง) |
| deny มี type พร้อมเหตุผล ไม่ drop เงียบ | **Dhammānupassanā** (รายงาน state จริง) |
| หนึ่ง protocol, cap ตามฮาร์ดแวร์จริง | **Mattaññutā** (พอเหมาะ — ไม่ lease สิ่งที่ไม่ว่าง) |

## Slices

- **A (revision นี้, framework):** spec นี้ + `bwoc resource snapshot` (ตรวจ GPU/CPU/RAM) + `bwoc resource gate-check` (dry-run sharing gate) + types ที่ ship แล้ว (`ResourceSnapshot`, `Gpu`, `SharingConfig`/`Caps`, `ClaimSpec`, `DenyReason`, `ResourceKind`) + parse config `[resource]` sharing-gate + `evaluate_gate`. ทั้งหมด local + unit-test; ยังไม่มีอะไรคุยกับ broker. struct `Lease` และชนิดข้อความ `RES.*` ระบุไว้ด้านบนแต่จะมาพร้อม transport ใน slice B/C
- **B (bwoc-gateway, ship แล้ว):** ครึ่ง discovery ของ broker — `POST /v1/resource/advertise` + `POST /v1/resource/discover` บน offer registry ใน memory ที่ evict ตาม TTL (หนึ่ง offer live ต่อ provider, last-writer-wins). claim/lease จงใจวิ่งผ่าน signed-envelope relay เดิม ไม่ใช่ route ใหม่ของ broker
- **C (framework, ship แล้ว):** client ของ broker — `bwoc resource advertise` (publish offer แบบ one-shot; รันบน timer เป็น heartbeat, gate ด้วย `[resource] share = true`) และ `bwoc resource discover` (query ตาม kind + min spec). ทั้งคู่คุย gateway ผ่าน HTTP(S) ด้วย `curl` (CLI ไม่มี HTTP client). loop discovery ทำงาน end-to-end ทั้งฟลีตแล้ว
- **D (framework, ถัดไป):** การยืมจริง — `claim` (ส่ง envelope `RES.CLAIM` ไป provider ผ่าน relay; provider ประเมิน `evaluate_gate` แล้ว mint `RES.LEASE` ที่เซ็นชื่อ), `release`, การรัน offload `compute` (รันงานบน provider ผ่าน A2A → คืนผล), แล้ว `kv` และ `knowledge`

## Cross-references

- [Fleet Governance](FLEET-GOVERNANCE.th.md) — §6 *เคารพทรัพยากรส่วนรวม* คือ charter ของ protocol นี้
- [Signing](SIGNING.th.md) — สัญญา signed-envelope ed25519 ที่ข้อความ resource reuse
- A2A (`bwoc a2a`) — transport รันงานที่ lease `compute` รันงานผ่าน
- [gateway](https://github.com/bemindlabs/bwoc-gateway) — relay ที่กลายเป็น broker
- [PLUGINS](PLUGINS.th.md) §Write verbs — sharing gate เป็น refuse-by-default รูปแบบเดียวกับ financial-write / IAM gate
