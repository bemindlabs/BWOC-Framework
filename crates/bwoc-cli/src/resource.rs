//! `bwoc resource` — fleet compute & memory sharing (the Resource Protocol).
//!
//! See `docs/en/RESOURCE-PROTOCOL.en.md` for the full design. This is **slice A**:
//! the two local, no-network verbs plus the shared types the broker + offload
//! slices (B/C) build on.
//!
//! - `snapshot` — print this host's `ResourceSnapshot` (GPU/CPU/RAM). READ.
//! - `gate-check` — dry-run the provider sharing gate: given this host's
//!   `[resource]` config + a fresh snapshot, would a hypothetical claim (kind +
//!   consumer + spec) be allowed? An operator runs it to validate their caps
//!   before turning sharing on. READ.
//!
//! `advertise` / `discover` / `claim` / `release` / `kv` need the broker (the
//! `bwoc-gateway` resource registry, slice B) and are documented in the spec —
//! not wired here.
//!
//! Exit codes: `0` ok · `2` usage/config error · `1` local error.

use clap::{Args, Subcommand};
use serde::Serialize;
use std::path::{Path, PathBuf};

const EXIT_OK: i32 = 0;
const EXIT_LOCAL_ERROR: i32 = 1;
const EXIT_USAGE: i32 = 2;

/// Resource kinds — one lease lifecycle, three typed resources.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum ResourceKind {
    Compute,
    Kv,
    Knowledge,
}

impl ResourceKind {
    fn parse(s: &str) -> Option<Self> {
        match s {
            "compute" => Some(Self::Compute),
            "kv" => Some(Self::Kv),
            "knowledge" => Some(Self::Knowledge),
            _ => None,
        }
    }
    fn as_str(self) -> &'static str {
        match self {
            Self::Compute => "compute",
            Self::Kv => "kv",
            Self::Knowledge => "knowledge",
        }
    }
}

// ---------------------------------------------------------------------------
// Snapshot types (what a provider advertises).
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Gpu {
    pub index: u32,
    pub model: String,
    pub vram_total_mb: u64,
    pub vram_free_mb: u64,
    pub util_pct: u32,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ResourceSnapshot {
    pub host: String,
    pub gpus: Vec<Gpu>,
    pub cpu_cores: u32,
    pub cpu_load1: f64,
    pub ram_total_mb: u64,
    pub ram_free_mb: u64,
    pub sampled_at: String,
}

// ---------------------------------------------------------------------------
// Sharing-gate config (`.bwoc/workspace.toml [resource]`).
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Default)]
pub struct SharingConfig {
    pub share: bool,
    /// Broker to advertise to. Parsed now so a config author can set it, but
    /// only consumed by the `advertise` heartbeat (slice B).
    #[allow(dead_code)]
    pub gateway: Option<String>,
    pub caps: Caps,
}

#[derive(Debug, Clone, Default)]
pub struct Caps {
    pub max_vram_mb: Option<u64>,
    pub max_ram_mb: Option<u64>,
    pub max_cpu_cores: Option<u32>,
    /// Concurrent-lease ceiling. Broker-side state (slice B) evaluates it; the
    /// single-snapshot gate here cannot, so it is parsed but not yet read.
    #[allow(dead_code)]
    pub max_leases: Option<u32>,
    pub allow: Vec<String>,
    pub kinds: Vec<String>,
}

/// A hypothetical claim's resource footprint (subset used by the gate).
#[derive(Debug, Clone, Default)]
pub struct ClaimSpec {
    pub gpu_vram_mb: Option<u64>,
    pub ram_mb: Option<u64>,
    pub cpu_cores: Option<u32>,
}

/// Typed denial reasons — a gate miss reports *why* (Dhammānupassanā).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum DenyReason {
    NotSharing,
    KindNotOffered,
    NotAllowed,
    OverCap,
    InsufficientFree,
}

impl DenyReason {
    fn as_str(self) -> &'static str {
        match self {
            Self::NotSharing => "not_sharing",
            Self::KindNotOffered => "kind_not_offered",
            Self::NotAllowed => "not_allowed",
            Self::OverCap => "over_cap",
            Self::InsufficientFree => "insufficient_free",
        }
    }
}

// ---------------------------------------------------------------------------
// CLI surface.
// ---------------------------------------------------------------------------

#[derive(Subcommand, Debug)]
pub enum ResourceCommand {
    /// Print this host's ResourceSnapshot (GPU / CPU / RAM). READ; local.
    Snapshot(SnapshotArgs),
    /// Dry-run the sharing gate: would a hypothetical claim be allowed by this
    /// host's `[resource]` caps against a fresh snapshot? READ; local.
    GateCheck(GateCheckArgs),
}

#[derive(Args, Debug)]
pub struct SnapshotArgs {
    #[arg(long)]
    pub workspace: Option<PathBuf>,
    #[arg(long)]
    pub json: bool,
}

#[derive(Args, Debug)]
pub struct GateCheckArgs {
    /// Resource kind: `compute` | `kv` | `knowledge`.
    #[arg(long)]
    pub kind: String,
    /// The hypothetical consumer's agent id (matched against `caps.allow`).
    #[arg(long)]
    pub from: String,
    /// Claim footprint: free VRAM the claim needs (MB).
    #[arg(long)]
    pub gpu_vram: Option<u64>,
    /// Claim footprint: RAM the claim needs (MB).
    #[arg(long)]
    pub ram: Option<u64>,
    /// Claim footprint: CPU cores the claim needs.
    #[arg(long)]
    pub cores: Option<u32>,
    #[arg(long)]
    pub workspace: Option<PathBuf>,
    #[arg(long)]
    pub json: bool,
}

pub fn run(cmd: ResourceCommand) -> i32 {
    match cmd {
        ResourceCommand::Snapshot(a) => run_snapshot(a),
        ResourceCommand::GateCheck(a) => run_gate_check(a),
    }
}

// ---------------------------------------------------------------------------
// Workspace resolution (same shape as the other CLIs).
// ---------------------------------------------------------------------------

fn find_workspace_root(explicit: Option<PathBuf>) -> Option<PathBuf> {
    if let Some(p) = explicit {
        return Some(p);
    }
    if let Ok(env_path) = std::env::var("BWOC_WORKSPACE") {
        let p = PathBuf::from(env_path);
        if !p.as_os_str().is_empty() {
            return Some(p);
        }
    }
    let mut cur = std::env::current_dir().ok()?;
    loop {
        if cur.join(".bwoc/workspace.toml").is_file() {
            return Some(cur);
        }
        if !cur.pop() {
            return None;
        }
    }
}

// ---------------------------------------------------------------------------
// Detection — best-effort, platform-tolerant. Pure parsers are unit-tested;
// the wrappers shell out / read platform files.
// ---------------------------------------------------------------------------

/// Parse `nvidia-smi --query-gpu=index,name,memory.total,memory.free,utilization.gpu
/// --format=csv,noheader,nounits` output. One GPU per non-empty line. A malformed
/// line is skipped (best-effort — a partial GPU list beats a hard failure).
pub fn parse_nvidia_smi(csv: &str) -> Vec<Gpu> {
    let mut gpus = Vec::new();
    for line in csv.lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        let f: Vec<&str> = line.split(',').map(|s| s.trim()).collect();
        if f.len() < 5 {
            continue;
        }
        let (Ok(index), Ok(total), Ok(free), Ok(util)) = (
            f[0].parse::<u32>(),
            f[2].parse::<u64>(),
            f[3].parse::<u64>(),
            f[4].parse::<u32>(),
        ) else {
            continue;
        };
        gpus.push(Gpu {
            index,
            model: f[1].to_string(),
            vram_total_mb: total,
            vram_free_mb: free,
            util_pct: util,
        });
    }
    gpus
}

/// Parse `MemTotal` + `MemAvailable` (KB) from `/proc/meminfo` → (total_mb, free_mb).
/// Returns `None` if either line is absent.
pub fn parse_proc_meminfo(text: &str) -> Option<(u64, u64)> {
    let mut total_kb = None;
    let mut avail_kb = None;
    for line in text.lines() {
        if let Some(rest) = line.strip_prefix("MemTotal:") {
            total_kb = rest
                .split_whitespace()
                .next()
                .and_then(|v| v.parse::<u64>().ok());
        } else if let Some(rest) = line.strip_prefix("MemAvailable:") {
            avail_kb = rest
                .split_whitespace()
                .next()
                .and_then(|v| v.parse::<u64>().ok());
        }
    }
    Some((total_kb? / 1024, avail_kb? / 1024))
}

/// Parse the first field of `/proc/loadavg` (1-minute load average).
pub fn parse_loadavg(text: &str) -> Option<f64> {
    text.split_whitespace()
        .next()
        .and_then(|v| v.parse::<f64>().ok())
}

fn detect_gpus() -> Vec<Gpu> {
    let out = std::process::Command::new("nvidia-smi")
        .args([
            "--query-gpu=index,name,memory.total,memory.free,utilization.gpu",
            "--format=csv,noheader,nounits",
        ])
        .output();
    match out {
        Ok(o) if o.status.success() => parse_nvidia_smi(&String::from_utf8_lossy(&o.stdout)),
        _ => Vec::new(),
    }
}

fn detect_hostname() -> String {
    // Best-effort: $HOST/$HOSTNAME, then `hostname`, then "unknown".
    for var in ["HOSTNAME", "HOST"] {
        if let Ok(h) = std::env::var(var) {
            if !h.trim().is_empty() {
                return h.trim().to_string();
            }
        }
    }
    if let Ok(o) = std::process::Command::new("hostname").output() {
        if o.status.success() {
            let h = String::from_utf8_lossy(&o.stdout).trim().to_string();
            if !h.is_empty() {
                return h;
            }
        }
    }
    "unknown".to_string()
}

fn detect_cpu_cores() -> u32 {
    std::thread::available_parallelism()
        .map(|n| n.get() as u32)
        .unwrap_or(0)
}

/// (ram_total_mb, ram_free_mb, cpu_load1) — Linux via /proc; other platforms
/// return best-effort zeros (the snapshot is still valid, just GPU/CPU-only).
fn detect_mem_and_load() -> (u64, u64, f64) {
    let (total, free) = std::fs::read_to_string("/proc/meminfo")
        .ok()
        .and_then(|t| parse_proc_meminfo(&t))
        .unwrap_or((0, 0));
    let load1 = std::fs::read_to_string("/proc/loadavg")
        .ok()
        .and_then(|t| parse_loadavg(&t))
        .unwrap_or(0.0);
    (total, free, load1)
}

fn detect_snapshot() -> ResourceSnapshot {
    let (ram_total_mb, ram_free_mb, cpu_load1) = detect_mem_and_load();
    ResourceSnapshot {
        host: detect_hostname(),
        gpus: detect_gpus(),
        cpu_cores: detect_cpu_cores(),
        cpu_load1,
        ram_total_mb,
        ram_free_mb,
        sampled_at: bwoc_core::time::utc_now_iso8601(),
    }
}

// ---------------------------------------------------------------------------
// Sharing-gate config parse + evaluation.
// ---------------------------------------------------------------------------

/// Load `[resource]` from `.bwoc/workspace.toml`. Absent section ⇒ default
/// (share = false) — refuse-by-default. A malformed file is an error.
pub fn load_sharing_config(root: &Path) -> Result<SharingConfig, String> {
    let path = root.join(".bwoc/workspace.toml");
    let body =
        std::fs::read_to_string(&path).map_err(|e| format!("read {}: {e}", path.display()))?;
    let value: toml::Value =
        toml::from_str(&body).map_err(|e| format!("{}: parse: {e}", path.display()))?;
    let Some(res) = value.get("resource").and_then(|v| v.as_table()) else {
        return Ok(SharingConfig::default());
    };
    let share = res.get("share").and_then(|v| v.as_bool()).unwrap_or(false);
    let gateway = res
        .get("gateway")
        .and_then(|v| v.as_str())
        .map(str::to_string);
    let caps = res
        .get("caps")
        .and_then(|v| v.as_table())
        .map(|c| Caps {
            max_vram_mb: c
                .get("max_vram_mb")
                .and_then(toml::Value::as_integer)
                .map(|i| i as u64),
            max_ram_mb: c
                .get("max_ram_mb")
                .and_then(toml::Value::as_integer)
                .map(|i| i as u64),
            max_cpu_cores: c
                .get("max_cpu_cores")
                .and_then(toml::Value::as_integer)
                .map(|i| i as u32),
            max_leases: c
                .get("max_leases")
                .and_then(toml::Value::as_integer)
                .map(|i| i as u32),
            allow: string_array(c.get("allow")),
            kinds: string_array(c.get("kinds")),
        })
        .unwrap_or_default();
    Ok(SharingConfig {
        share,
        gateway,
        caps,
    })
}

fn string_array(v: Option<&toml::Value>) -> Vec<String> {
    v.and_then(toml::Value::as_array)
        .map(|a| {
            a.iter()
                .filter_map(|x| x.as_str().map(str::to_string))
                .collect()
        })
        .unwrap_or_default()
}

/// Evaluate the provider sharing gate for a hypothetical claim against a live
/// snapshot. `Ok(())` = would grant; `Err(reason)` = would deny (with why).
///
/// Mirrors the spec's five-step gate. The `max_leases` step is broker-side
/// state (slice B) and is not evaluable from a single snapshot, so it is out of
/// scope here — this checks the four snapshot-evaluable conditions.
pub fn evaluate_gate(
    cfg: &SharingConfig,
    kind: ResourceKind,
    consumer: &str,
    spec: &ClaimSpec,
    snapshot: &ResourceSnapshot,
) -> Result<(), DenyReason> {
    // 1) Master opt-in.
    if !cfg.share {
        return Err(DenyReason::NotSharing);
    }
    // 2) Kind offered.
    if !cfg.caps.kinds.iter().any(|k| k == kind.as_str()) {
        return Err(DenyReason::KindNotOffered);
    }
    // 3) Consumer allowed (empty allow ⇒ any enrolled peer).
    if !cfg.caps.allow.is_empty() && !cfg.caps.allow.iter().any(|a| a == consumer) {
        return Err(DenyReason::NotAllowed);
    }
    // 4) Spec fits caps AND live snapshot has it free.
    if let Some(need) = spec.gpu_vram_mb {
        if let Some(cap) = cfg.caps.max_vram_mb {
            if need > cap {
                return Err(DenyReason::OverCap);
            }
        }
        // Live free: the single best GPU must have enough headroom.
        let best_free = snapshot
            .gpus
            .iter()
            .map(|g| g.vram_free_mb)
            .max()
            .unwrap_or(0);
        if need > best_free {
            return Err(DenyReason::InsufficientFree);
        }
    }
    if let Some(need) = spec.ram_mb {
        if let Some(cap) = cfg.caps.max_ram_mb {
            if need > cap {
                return Err(DenyReason::OverCap);
            }
        }
        if need > snapshot.ram_free_mb {
            return Err(DenyReason::InsufficientFree);
        }
    }
    if let Some(need) = spec.cpu_cores {
        if let Some(cap) = cfg.caps.max_cpu_cores {
            if need > cap {
                return Err(DenyReason::OverCap);
            }
        }
        if need > snapshot.cpu_cores {
            return Err(DenyReason::InsufficientFree);
        }
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Verb: snapshot.
// ---------------------------------------------------------------------------

fn run_snapshot(args: SnapshotArgs) -> i32 {
    // Workspace is optional for snapshot (pure host probe) — resolve only to
    // stay consistent, but a missing workspace is not fatal here.
    let _ = find_workspace_root(args.workspace);
    let snap = detect_snapshot();
    if args.json {
        match serde_json::to_string_pretty(&snap) {
            Ok(s) => {
                println!("{s}");
                EXIT_OK
            }
            Err(e) => {
                eprintln!("bwoc resource snapshot: serialize: {e}");
                EXIT_LOCAL_ERROR
            }
        }
    } else {
        println!("host: {}", snap.host);
        if snap.gpus.is_empty() {
            println!("gpu:  (none — nvidia-smi absent or no GPU)");
        } else {
            for g in &snap.gpus {
                println!(
                    "gpu {}: {} — {} / {} MB free ({}% util)",
                    g.index, g.model, g.vram_free_mb, g.vram_total_mb, g.util_pct
                );
            }
        }
        println!(
            "cpu:  {} cores, load1 {:.2}",
            snap.cpu_cores, snap.cpu_load1
        );
        if snap.ram_total_mb == 0 {
            println!("ram:  (unavailable on this platform — Linux /proc only in slice A)");
        } else {
            println!("ram:  {} / {} MB free", snap.ram_free_mb, snap.ram_total_mb);
        }
        println!("at:   {}", snap.sampled_at);
        EXIT_OK
    }
}

// ---------------------------------------------------------------------------
// Verb: gate-check.
// ---------------------------------------------------------------------------

fn run_gate_check(args: GateCheckArgs) -> i32 {
    let Some(kind) = ResourceKind::parse(&args.kind) else {
        let msg = format!(
            "invalid --kind '{}' — expected compute | kv | knowledge",
            args.kind
        );
        if args.json {
            print_gate_json(false, Some("bad_kind"), &msg);
        } else {
            eprintln!("bwoc resource gate-check: {msg}");
        }
        return EXIT_USAGE;
    };
    let Some(root) = find_workspace_root(args.workspace) else {
        let msg = "no workspace found (no .bwoc/workspace.toml in cwd or ancestors)".to_string();
        if args.json {
            print_gate_json(false, Some("no_workspace"), &msg);
        } else {
            eprintln!("bwoc resource gate-check: {msg}");
        }
        return EXIT_USAGE;
    };
    let cfg = match load_sharing_config(&root) {
        Ok(c) => c,
        Err(e) => {
            if args.json {
                print_gate_json(false, Some("config_error"), &e);
            } else {
                eprintln!("bwoc resource gate-check: {e}");
            }
            return EXIT_USAGE;
        }
    };
    let snap = detect_snapshot();
    let spec = ClaimSpec {
        gpu_vram_mb: args.gpu_vram,
        ram_mb: args.ram,
        cpu_cores: args.cores,
    };
    match evaluate_gate(&cfg, kind, &args.from, &spec, &snap) {
        Ok(()) => {
            if args.json {
                print_gate_json(true, None, "would grant");
            } else {
                println!(
                    "ALLOW — {} claim from {} fits this host's caps + free resources",
                    kind.as_str(),
                    args.from
                );
            }
            EXIT_OK
        }
        Err(reason) => {
            let msg = format!("would deny: {}", reason.as_str());
            if args.json {
                print_gate_json(false, Some(reason.as_str()), &msg);
            } else {
                println!(
                    "DENY ({}) — {} claim from {}",
                    reason.as_str(),
                    kind.as_str(),
                    args.from
                );
            }
            // A deny is a valid answer, not an error — exit 0.
            EXIT_OK
        }
    }
}

fn print_gate_json(allow: bool, reason: Option<&str>, message: &str) {
    let value = serde_json::json!({
        "ok": true,
        "allow": allow,
        "reason": reason,
        "message": message,
    });
    if let Ok(s) = serde_json::to_string_pretty(&value) {
        println!("{s}");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn snap_with(gpu_free: u64, ram_free: u64, cores: u32) -> ResourceSnapshot {
        ResourceSnapshot {
            host: "test".into(),
            gpus: if gpu_free > 0 {
                vec![Gpu {
                    index: 0,
                    model: "Test GPU".into(),
                    vram_total_mb: gpu_free + 1000,
                    vram_free_mb: gpu_free,
                    util_pct: 5,
                }]
            } else {
                vec![]
            },
            cpu_cores: cores,
            cpu_load1: 1.0,
            ram_total_mb: ram_free + 1000,
            ram_free_mb: ram_free,
            sampled_at: "2026-07-25T00:00:00Z".into(),
        }
    }

    fn sharing(kinds: &[&str], allow: &[&str]) -> SharingConfig {
        SharingConfig {
            share: true,
            gateway: None,
            caps: Caps {
                max_vram_mb: Some(40000),
                max_ram_mb: Some(64000),
                max_cpu_cores: Some(96),
                max_leases: Some(4),
                allow: allow.iter().map(|s| s.to_string()).collect(),
                kinds: kinds.iter().map(|s| s.to_string()).collect(),
            },
        }
    }

    #[test]
    fn parse_nvidia_smi_reads_rows_skips_junk() {
        let csv = "0, NVIDIA RTX A6000, 49140, 40320, 12\nbroken line\n1, GPU B, 24576, 20000, 3\n";
        let gpus = parse_nvidia_smi(csv);
        assert_eq!(gpus.len(), 2);
        assert_eq!(gpus[0].model, "NVIDIA RTX A6000");
        assert_eq!(gpus[0].vram_free_mb, 40320);
        assert_eq!(gpus[1].index, 1);
    }

    #[test]
    fn parse_nvidia_smi_empty_is_empty() {
        assert!(parse_nvidia_smi("").is_empty());
        assert!(parse_nvidia_smi("\n\n").is_empty());
    }

    #[test]
    fn parse_meminfo_converts_kb_to_mb() {
        let text = "MemTotal:       131072000 kB\nMemFree: 1000 kB\nMemAvailable:   98304000 kB\n";
        let (total, free) = parse_proc_meminfo(text).unwrap();
        assert_eq!(total, 128000); // 131072000 / 1024
        assert_eq!(free, 96000); // 98304000 / 1024
    }

    #[test]
    fn parse_meminfo_missing_available_is_none() {
        assert!(parse_proc_meminfo("MemTotal: 100 kB\n").is_none());
    }

    #[test]
    fn parse_loadavg_first_field() {
        assert_eq!(parse_loadavg("8.42 6.13 5.00 2/1234 5678"), Some(8.42));
        assert_eq!(parse_loadavg(""), None);
    }

    #[test]
    fn gate_refuses_when_not_sharing() {
        let mut cfg = sharing(&["compute"], &[]);
        cfg.share = false;
        let r = evaluate_gate(
            &cfg,
            ResourceKind::Compute,
            "agent-anna",
            &ClaimSpec::default(),
            &snap_with(40000, 64000, 96),
        );
        assert_eq!(r, Err(DenyReason::NotSharing));
    }

    #[test]
    fn gate_refuses_kind_not_offered() {
        let cfg = sharing(&["compute"], &[]);
        let r = evaluate_gate(
            &cfg,
            ResourceKind::Kv,
            "agent-anna",
            &ClaimSpec::default(),
            &snap_with(0, 64000, 96),
        );
        assert_eq!(r, Err(DenyReason::KindNotOffered));
    }

    #[test]
    fn gate_refuses_consumer_not_allowed() {
        let cfg = sharing(&["compute"], &["agent-qianliyan"]);
        let r = evaluate_gate(
            &cfg,
            ResourceKind::Compute,
            "agent-anna",
            &ClaimSpec::default(),
            &snap_with(40000, 64000, 96),
        );
        assert_eq!(r, Err(DenyReason::NotAllowed));
    }

    #[test]
    fn gate_empty_allow_permits_any_peer() {
        let cfg = sharing(&["compute"], &[]);
        let spec = ClaimSpec {
            gpu_vram_mb: Some(24000),
            ..Default::default()
        };
        let r = evaluate_gate(
            &cfg,
            ResourceKind::Compute,
            "agent-anybody",
            &spec,
            &snap_with(40000, 64000, 96),
        );
        assert_eq!(r, Ok(()));
    }

    #[test]
    fn gate_over_cap_vs_insufficient_free_are_distinct() {
        let cfg = sharing(&["compute"], &[]);
        // needs 50G but cap is 40G → over_cap (even though live free is 45G).
        let over = ClaimSpec {
            gpu_vram_mb: Some(50000),
            ..Default::default()
        };
        assert_eq!(
            evaluate_gate(
                &cfg,
                ResourceKind::Compute,
                "x",
                &over,
                &snap_with(45000, 64000, 96)
            ),
            Err(DenyReason::OverCap)
        );
        // needs 30G (≤ cap) but live free is only 20G → insufficient_free.
        let tight = ClaimSpec {
            gpu_vram_mb: Some(30000),
            ..Default::default()
        };
        assert_eq!(
            evaluate_gate(
                &cfg,
                ResourceKind::Compute,
                "x",
                &tight,
                &snap_with(20000, 64000, 96)
            ),
            Err(DenyReason::InsufficientFree)
        );
    }

    #[test]
    fn gate_grants_when_everything_fits() {
        let cfg = sharing(&["compute", "knowledge"], &["agent-anna"]);
        let spec = ClaimSpec {
            gpu_vram_mb: Some(24000),
            ram_mb: Some(32000),
            cpu_cores: Some(16),
        };
        let r = evaluate_gate(
            &cfg,
            ResourceKind::Compute,
            "agent-anna",
            &spec,
            &snap_with(40000, 64000, 96),
        );
        assert_eq!(r, Ok(()));
    }

    #[test]
    fn load_config_absent_section_is_refuse_by_default() {
        let tmp = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(tmp.path().join(".bwoc")).unwrap();
        std::fs::write(
            tmp.path().join(".bwoc/workspace.toml"),
            "[workspace]\nname = \"x\"\nversion = \"0.1.0\"\n",
        )
        .unwrap();
        let cfg = load_sharing_config(tmp.path()).unwrap();
        assert!(!cfg.share);
        assert!(cfg.caps.kinds.is_empty());
    }

    #[test]
    fn load_config_parses_caps() {
        let tmp = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(tmp.path().join(".bwoc")).unwrap();
        std::fs::write(
            tmp.path().join(".bwoc/workspace.toml"),
            "[workspace]\nname=\"x\"\nversion=\"0.1.0\"\n\n\
             [resource]\nshare = true\ngateway = \"wss://gw\"\n\n\
             [resource.caps]\nmax_vram_mb = 40000\nallow = [\"agent-anna\"]\nkinds = [\"compute\"]\n",
        )
        .unwrap();
        let cfg = load_sharing_config(tmp.path()).unwrap();
        assert!(cfg.share);
        assert_eq!(cfg.gateway.as_deref(), Some("wss://gw"));
        assert_eq!(cfg.caps.max_vram_mb, Some(40000));
        assert_eq!(cfg.caps.allow, vec!["agent-anna"]);
        assert_eq!(cfg.caps.kinds, vec!["compute"]);
    }
}
