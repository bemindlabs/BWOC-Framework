//! `bwoc handbook [section]` — a bundled, offline quick guide.
//!
//! Content ships **inside the binary** (no network, no files to find) as a
//! static section table. `bwoc handbook` prints the index; `bwoc handbook
//! <section>` prints one section. Bilingual: the resolved language (`--lang` /
//! `BWOC_LANG` / `LANG`) picks the Thai body, falling back to English.
//!
//! Kept deliberately terminal-sized and task-oriented (Mattaññutā) — the full
//! reference lives in `docs/`; this is the get-moving guide.

/// One handbook section: a stable `name` (the `bwoc handbook <name>` key), a
/// `title`, and English + Thai bodies.
struct Section {
    name: &'static str,
    title: &'static str,
    en: &'static str,
    th: &'static str,
}

/// Ordered for a natural read-through: install → incarnate → run → collaborate
/// → the runtime under it → ship.
const SECTIONS: &[Section] = &[
    Section {
        name: "start",
        title: "Getting started",
        en: "\
Install the toolkit, then create a workspace:

  cargo install --path crates/bwoc-cli      # or grab a release binary
  bwoc init                                  # scaffold .bwoc/ in the cwd
  bwoc info                                  # version, workspace, update status
  bwoc list                                  # registered agents (empty at first)

A *workspace* is any directory with a .bwoc/workspace.toml; bwoc resolves it
from --workspace, then BWOC_WORKSPACE, then by walking up from the cwd.

Next: `bwoc handbook agents` to incarnate your first agent.",
        th: "\
ติดตั้งเครื่องมือ แล้วสร้าง workspace:

  cargo install --path crates/bwoc-cli      # หรือใช้ไบนารีจาก release
  bwoc init                                  # สร้าง .bwoc/ ใน cwd
  bwoc info                                  # เวอร์ชัน, workspace, สถานะอัปเดต
  bwoc list                                  # agent ที่ลงทะเบียน (ตอนแรกว่าง)

*workspace* คือไดเรกทอรีที่มี .bwoc/workspace.toml; bwoc หาจาก --workspace
ก่อน แล้ว BWOC_WORKSPACE แล้วไล่ขึ้นจาก cwd

ถัดไป: `bwoc handbook agents` เพื่อสร้าง agent ตัวแรก",
    },
    Section {
        name: "agents",
        title: "Incarnate agents",
        en: "\
Agents are created by the CLI, never by hand-editing .bwoc/agents.toml:

  bwoc new <name> --template <path> --target agents/agent-<name>
  bwoc check <agent-path>          # backend-neutrality audit (run after edits)
  bwoc status <agent-path>         # health + identity
  bwoc retire <name>               # remove from registry + files

Each agent is agents/agent-<name>/ with slot dirs (persona/ mindsets/ skills/
interconnect/ memories/) plus AGENTS.md — the single backend-neutral source of
truth, symlinked to CLAUDE.md / CODEX.md / KIMI.md / AGY.md / OLLAMA.md.

Keep AGENTS.md neutral: no YAML frontmatter, no model IDs/vendor names (use
{{camelCase}} placeholders). `bwoc check` enforces this.",
        th: "\
agent ถูกสร้างผ่าน CLI เท่านั้น ห้ามแก้ .bwoc/agents.toml ด้วยมือ:

  bwoc new <name> --template <path> --target agents/agent-<name>
  bwoc check <agent-path>          # ตรวจ backend-neutrality (รันหลังแก้)
  bwoc status <agent-path>         # สุขภาพ + identity
  bwoc retire <name>               # ลบออกจาก registry + ไฟล์

แต่ละ agent คือ agents/agent-<name>/ มี slot dirs (persona/ mindsets/ skills/
interconnect/ memories/) และ AGENTS.md — แหล่งความจริงเดียวที่ backend-neutral
symlink ไปยัง CLAUDE.md / CODEX.md / KIMI.md / AGY.md / OLLAMA.md

ให้ AGENTS.md เป็นกลาง: ไม่มี YAML frontmatter, ไม่ฮาร์ดโค้ด model ID/ชื่อ vendor
(ใช้ placeholder {{camelCase}}) — `bwoc check` บังคับกฎนี้",
    },
    Section {
        name: "spawn",
        title: "Spawn & chat",
        en: "\
Run an agent interactively against its declared backend:

  bwoc spawn <agent>               # exec the backend CLI (claude/agy/codex/kimi)
  bwoc chat <agent>                # shortcut: spawn with the resolved backend
  bwoc chat <agent> --tui          # full-screen client (harness backends only)
  bwoc chat <agent> --tui --team <id>   # join the team's shared chat channel

Ollama / OpenAI-compatible backends have no vendor CLI — bwoc supplies the
agentic loop itself via bwoc-harness (see `bwoc handbook harness`). Vendor
backends (claude/agy/codex/kimi) speak their own interactive protocol.",
        th: "\
รัน agent แบบโต้ตอบกับ backend ที่ประกาศไว้:

  bwoc spawn <agent>               # exec backend CLI (claude/agy/codex/kimi)
  bwoc chat <agent>                # ทางลัด: spawn ด้วย backend ที่ resolve ได้
  bwoc chat <agent> --tui          # client เต็มจอ (เฉพาะ backend แบบ harness)
  bwoc chat <agent> --tui --team <id>   # เข้าร่วม chat channel ของทีม

Ollama / OpenAI-compatible ไม่มี vendor CLI — bwoc จัดหา agentic loop เองผ่าน
bwoc-harness (ดู `bwoc handbook harness`) ส่วน vendor (claude/agy/codex/kimi)
ใช้โปรโตคอลโต้ตอบของตัวเอง",
    },
    Section {
        name: "teams",
        title: "Saṅgha teams",
        en: "\
A team groups agents under a shared task list (the Saṅgha):

  bwoc team create <id> --members agent-a,agent-b
  bwoc team list
  bwoc task add <id> \"<title>\"     # put work on the shared list
  bwoc task list <id>

Agents self-claim pending tasks. The harness lead drains the list, spawning a
worker per task in its own git worktree (parallel up to --concurrency). Workers
leave a structured result envelope; the lead can route a diff to a designated
reviewer (the team's `reviewer`) before completing — APPROVE completes, REJECT
re-queues. Teammates can also share a live chat channel (`--team`).",
        th: "\
team จัดกลุ่ม agent ภายใต้ task list ร่วมกัน (สังฆะ):

  bwoc team create <id> --members agent-a,agent-b
  bwoc team list
  bwoc task add <id> \"<title>\"     # วางงานลง list ร่วม
  bwoc task list <id>

agent self-claim งานที่ค้าง lead ของ harness ดึงงานจาก list แล้ว spawn worker
ต่อหนึ่งงานใน git worktree ของตัวเอง (ขนานได้ถึง --concurrency) worker ทิ้ง
result envelope แบบมีโครงสร้างไว้ lead ส่ง diff ให้ reviewer ที่กำหนด (`reviewer`
ของทีม) ตรวจก่อน complete ได้ — APPROVE = complete, REJECT = คืนเข้า queue
เพื่อนร่วมทีมแชร์ chat channel สดได้ด้วย (`--team`)",
    },
    Section {
        name: "harness",
        title: "Harness runtime",
        en: "\
bwoc-harness is the self-hosted agentic loop for backends without a vendor CLI:

  bwoc run --task \"<task>\"          # run one task to completion, autonomously
  bwoc-harness --lead --tasks <path> [--reviewer <agent>]   # Saṅgha lead

Every tool call passes guardrails → permission → sandbox before it runs;
denials are fed back to the model, not fatal. It is multi-LLM (OpenAI-compatible
+ native Anthropic, with model fallback) and remembers across sessions via Tier
2 deep memory (wake-up / search / mine). `ask`-mode fails safe to deny in
non-TTY, so autonomous runs use an allow-listed .bwoc/harness-policy.toml.",
        th: "\
bwoc-harness คือ agentic loop แบบ self-host สำหรับ backend ที่ไม่มี vendor CLI:

  bwoc run --task \"<task>\"          # รันหนึ่งงานจนจบแบบอัตโนมัติ
  bwoc-harness --lead --tasks <path> [--reviewer <agent>]   # Saṅgha lead

ทุก tool call ผ่าน guardrails → permission → sandbox ก่อนรัน; การปฏิเสธถูก
ป้อนกลับให้ model ไม่ทำให้ล้ม รองรับหลาย LLM (OpenAI-compatible + Anthropic
native พร้อม model fallback) และจำข้าม session ผ่าน Tier 2 deep memory
(wake-up / search / mine) โหมด `ask` fail-safe เป็น deny ใน non-TTY งานอัตโนมัติ
จึงใช้ .bwoc/harness-policy.toml แบบ allow-list",
    },
    Section {
        name: "release",
        title: "Releasing & updates",
        en: "\
bwoc checks for a newer release in the background (≤ once/24h) and prints a
one-line notice when you've drifted. To act:

  bwoc update --check              # compare your build to the latest release
  bwoc update                      # upgrade via your install method (brew/cargo)
  bwoc info                        # version + release identity + update status

Cutting a release (maintainers): finalize CHANGELOG, bump the version, then push
a CalVer tag `vYYYY.M.D-<patch>` — the release pipeline builds the cross-platform
binaries with SHA-256 checksums. Full recipe: docs/en/RELEASING.en.md.",
        th: "\
bwoc ตรวจ release ใหม่ใน background (≤ วันละครั้ง) และแจ้งบรรทัดเดียวเมื่อคุณ
ตามหลัง วิธีจัดการ:

  bwoc update --check              # เทียบ build ของคุณกับ release ล่าสุด
  bwoc update                      # อัปเกรดตามวิธีติดตั้ง (brew/cargo)
  bwoc info                        # เวอร์ชัน + release identity + สถานะอัปเดต

การ cut release (maintainer): จบ CHANGELOG, bump เวอร์ชัน, แล้ว push tag CalVer
`vYYYY.M.D-<patch>` — pipeline จะ build ไบนารีข้ามแพลตฟอร์มพร้อม SHA-256
สูตรเต็ม: docs/en/RELEASING.en.md",
    },
];

/// Args for `bwoc handbook [section]`.
pub struct HandbookArgs {
    pub section: Option<String>,
    pub lang: String,
}

/// Pick the body for the resolved language (Thai if `lang` starts with `th`,
/// else English).
fn body<'a>(section: &'a Section, lang: &str) -> &'a str {
    if lang.starts_with("th") {
        section.th
    } else {
        section.en
    }
}

pub fn run(args: HandbookArgs) -> i32 {
    match args.section.as_deref() {
        None => {
            print_index(&args.lang);
            0
        }
        Some(name) => match SECTIONS.iter().find(|s| s.name == name) {
            Some(s) => {
                println!("## {}\n\n{}", s.title, body(s, &args.lang));
                0
            }
            None => {
                eprintln!("bwoc handbook: unknown section '{name}'. Available sections:");
                for s in SECTIONS {
                    eprintln!("  {:<10} {}", s.name, s.title);
                }
                2
            }
        },
    }
}

fn print_index(lang: &str) {
    let header = if lang.starts_with("th") {
        "BWOC Handbook — คู่มือใช้งานแบบสั้น"
    } else {
        "BWOC Handbook"
    };
    println!("{header}\n");
    for (i, s) in SECTIONS.iter().enumerate() {
        println!("  {}. {:<20} bwoc handbook {}", i + 1, s.title, s.name);
    }
    let hint = if lang.starts_with("th") {
        "\nดูหนึ่งหัวข้อ: bwoc handbook <ชื่อ>   ·   bwoc info สำหรับสถานะระบบ"
    } else {
        "\nRead a section: bwoc handbook <name>   ·   bwoc info for system status"
    };
    println!("{hint}");
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_section_has_both_languages_and_unique_names() {
        let mut seen = std::collections::HashSet::new();
        for s in SECTIONS {
            assert!(!s.en.trim().is_empty(), "{} missing EN", s.name);
            assert!(!s.th.trim().is_empty(), "{} missing TH", s.name);
            assert!(!s.title.trim().is_empty(), "{} missing title", s.name);
            assert!(seen.insert(s.name), "duplicate section name {}", s.name);
        }
    }

    #[test]
    fn body_picks_thai_for_th_lang_else_english() {
        let s = &SECTIONS[0];
        assert_eq!(body(s, "th"), s.th);
        assert_eq!(body(s, "th-TH"), s.th);
        assert_eq!(body(s, "en"), s.en);
        assert_eq!(body(s, "fr"), s.en); // unknown → English
    }

    #[test]
    fn unknown_section_is_error_known_is_ok() {
        assert_eq!(
            run(HandbookArgs {
                section: Some("nope".into()),
                lang: "en".into()
            }),
            2
        );
        assert_eq!(
            run(HandbookArgs {
                section: Some("teams".into()),
                lang: "en".into()
            }),
            0
        );
        assert_eq!(
            run(HandbookArgs {
                section: None,
                lang: "en".into()
            }),
            0
        );
    }
}
