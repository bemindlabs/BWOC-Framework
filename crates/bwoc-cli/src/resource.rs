//! `bwoc resource` — fleet compute & memory sharing (the Resource Protocol).
//!
//! See `docs/en/RESOURCE-PROTOCOL.en.md` for the full design. Slices A + C:
//!
//! - `snapshot` — print this host's `ResourceSnapshot` (GPU/CPU/RAM). READ; local.
//! - `gate-check` — dry-run the provider sharing gate: given this host's
//!   `[resource]` config + a fresh snapshot, would a hypothetical claim (kind +
//!   consumer + spec) be allowed? An operator runs it to validate their caps
//!   before turning sharing on. READ; local.
//! - `advertise` — publish this host's offer to the gateway broker (one shot;
//!   run on a timer for a heartbeat). Requires `[resource] share = true`.
//! - `discover` — query the broker for offers of a kind meeting a minimum spec.
//!
//! `advertise` / `discover` reach the gateway over HTTP(S) by shelling `curl`
//! (the framework CLI stays HTTP-client-free, matching the plugin path). The
//! `claim` / `release` / `kv` verbs — the lease round trip over the relay + the
//! offload execution — are documented in the spec and land in the next slice.
//!
//! Exit codes: `0` ok · `1` local error · `2` usage/config error · `255` broker
//! transport error (network / non-2xx / non-JSON).

use clap::{Args, Subcommand};
use serde::Serialize;
use std::path::{Path, PathBuf};

const EXIT_OK: i32 = 0;
const EXIT_LOCAL_ERROR: i32 = 1;
const EXIT_USAGE: i32 = 2;
/// Gateway/broker transport failure (network, non-2xx, non-JSON response).
const EXIT_BROKER_ERROR: i32 = 255;

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
    /// Broker to advertise to / discover through (`ws(s)://` or `http(s)://`).
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
    /// Publish this host's offer to the gateway broker (one shot — run on a
    /// timer for a heartbeat). Requires `[resource] share = true` + a gateway.
    Advertise(AdvertiseArgs),
    /// Query the gateway broker for offers of a kind meeting a minimum spec.
    Discover(DiscoverArgs),
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

#[derive(Args, Debug)]
pub struct AdvertiseArgs {
    /// This provider's agent id (the offer key + the `RES.CLAIM` recipient).
    #[arg(long)]
    pub provider: String,
    /// Offer lifetime in seconds; the broker evicts the offer after this unless
    /// re-advertised. Run on a timer at ~half this interval for a heartbeat.
    #[arg(long, default_value_t = 30)]
    pub ttl: u64,
    /// Gateway HTTP(S) base or `ws(s)://` URL (overrides `[resource] gateway`).
    #[arg(long)]
    pub gateway: Option<String>,
    #[arg(long)]
    pub workspace: Option<PathBuf>,
    #[arg(long)]
    pub json: bool,
}

#[derive(Args, Debug)]
pub struct DiscoverArgs {
    /// Resource kind to discover: `compute` | `kv` | `knowledge`.
    #[arg(long)]
    pub kind: String,
    /// Minimum free VRAM (MB) an offer's best GPU must have.
    #[arg(long)]
    pub gpu_vram: Option<u64>,
    /// Minimum free RAM (MB).
    #[arg(long)]
    pub ram: Option<u64>,
    /// Minimum CPU cores.
    #[arg(long)]
    pub cores: Option<u32>,
    /// Gateway HTTP(S) base or `ws(s)://` URL (overrides `[resource] gateway`).
    #[arg(long)]
    pub gateway: Option<String>,
    #[arg(long)]
    pub workspace: Option<PathBuf>,
    #[arg(long)]
    pub json: bool,
}

pub fn run(cmd: ResourceCommand) -> i32 {
    match cmd {
        ResourceCommand::Snapshot(a) => run_snapshot(a),
        ResourceCommand::GateCheck(a) => run_gate_check(a),
        ResourceCommand::Advertise(a) => run_advertise(a),
        ResourceCommand::Discover(a) => run_discover(a),
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
            // Checked conversions: a negative cap (`-1`) must become `None`, not
            // wrap to a huge value that silently allows everything.
            max_vram_mb: cap_u64(c.get("max_vram_mb")),
            max_ram_mb: cap_u64(c.get("max_ram_mb")),
            max_cpu_cores: cap_u32(c.get("max_cpu_cores")),
            max_leases: cap_u32(c.get("max_leases")),
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

/// A cap must be a non-negative integer. A negative (or non-integer) value is
/// treated as "unset" (`None`) rather than wrapping — a `-1` cap must never
/// become an unbounded allow.
fn cap_u64(v: Option<&toml::Value>) -> Option<u64> {
    v.and_then(toml::Value::as_integer)
        .and_then(|i| u64::try_from(i).ok())
}

fn cap_u32(v: Option<&toml::Value>) -> Option<u32> {
    v.and_then(toml::Value::as_integer)
        .and_then(|i| u32::try_from(i).ok())
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
            emit_gate_error_json("bad_kind", &msg);
        } else {
            eprintln!("bwoc resource gate-check: {msg}");
        }
        return EXIT_USAGE;
    };
    let Some(root) = find_workspace_root(args.workspace) else {
        let msg = "no workspace found (no .bwoc/workspace.toml in cwd or ancestors)".to_string();
        if args.json {
            emit_gate_error_json("no_workspace", &msg);
        } else {
            eprintln!("bwoc resource gate-check: {msg}");
        }
        return EXIT_USAGE;
    };
    let cfg = match load_sharing_config(&root) {
        Ok(c) => c,
        Err(e) => {
            if args.json {
                emit_gate_error_json("config_error", &e);
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

/// A valid gate answer (`ok: true`). `allow` is the verdict; `reason` names the
/// deny reason on a deny, `null` on a grant. A *deny is a valid answer*, so it is
/// still `ok: true` — distinct from an invalid invocation below.
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

/// An invalid invocation (`ok: false`) — bad args, no workspace, unreadable
/// config. Distinct shape so a script never mistakes a usage error for a deny.
fn emit_gate_error_json(error: &str, message: &str) {
    let value = serde_json::json!({
        "ok": false,
        "error": error,
        "message": message,
    });
    if let Ok(s) = serde_json::to_string_pretty(&value) {
        println!("{s}");
    }
}

// ---------------------------------------------------------------------------
// Broker transport — advertise / discover over HTTP(S) to the gateway.
//
// The framework CLI stays HTTP-client-free (like the plugin path, which shells
// `curl`); the gateway is reachable over HTTPS (tailscale serve), which rules
// out a hand-rolled TCP client. So these verbs shell `curl` too.
// ---------------------------------------------------------------------------

/// Normalise a configured gateway URL to an HTTP(S) base. `ws://`→`http://`,
/// `wss://`→`https://`; an `http(s)://` value passes through; a bare host is
/// assumed `https://`. Trailing slash trimmed.
fn gateway_http_base(url: &str) -> String {
    let u = url.trim().trim_end_matches('/');
    if let Some(rest) = u.strip_prefix("wss://") {
        format!("https://{rest}")
    } else if let Some(rest) = u.strip_prefix("ws://") {
        format!("http://{rest}")
    } else if u.starts_with("http://") || u.starts_with("https://") {
        u.to_string()
    } else {
        format!("https://{u}")
    }
}

/// Resolve the gateway base: `--gateway` flag first, then `[resource] gateway`.
fn resolve_gateway(flag: Option<&str>, cfg: &SharingConfig) -> Result<String, String> {
    let raw = flag.map(str::to_string).or_else(|| cfg.gateway.clone()).ok_or_else(|| {
        "no gateway configured — pass --gateway or set [resource] gateway in .bwoc/workspace.toml"
            .to_string()
    })?;
    Ok(gateway_http_base(&raw))
}

/// POST `body` as JSON to `url` via `curl`, returning the parsed JSON response.
///
/// Portable across curl versions — it does **not** use `--fail`/`--fail-with-body`
/// (the latter is missing on older system curls, e.g. some macOS builds). Instead
/// it appends the HTTP status via `-w` and splits it off in Rust, keeping the body
/// for diagnostics and treating any non-2xx as an error. A non-zero curl exit is a
/// transport failure (connection refused, timeout).
fn http_post_json(url: &str, body: &serde_json::Value) -> Result<serde_json::Value, String> {
    use std::io::Write;
    use std::process::{Command, Stdio};

    let payload = serde_json::to_string(body).map_err(|e| format!("serialize request: {e}"))?;
    // `-w "\n<status>"` appends a final line with the numeric HTTP code; we split
    // it off below. A sentinel newline separates it from the (possibly newline-free)
    // JSON body.
    let mut child = Command::new("curl")
        .args([
            "-sS", // silent but show transport errors on stderr
            "-X",
            "POST",
            "-H",
            "content-type: application/json",
            "--connect-timeout",
            "5",
            "--max-time",
            "20",
            "-w",
            "\n%{http_code}",
            "--data-binary",
            "@-",
            url,
        ])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| format!("spawn curl (is it installed?): {e}"))?;
    if let Some(stdin) = child.stdin.as_mut() {
        stdin
            .write_all(payload.as_bytes())
            .map_err(|e| format!("write request body: {e}"))?;
    }
    drop(child.stdin.take());
    let out = child
        .wait_with_output()
        .map_err(|e| format!("wait curl: {e}"))?;
    if !out.status.success() {
        // Transport failure (no HTTP response at all).
        let stderr = String::from_utf8_lossy(&out.stderr);
        return Err(format!(
            "gateway request failed ({}): {}",
            out.status.code().unwrap_or(-1),
            stderr.trim()
        ));
    }
    let raw = String::from_utf8_lossy(&out.stdout);
    // Split the trailing `\n<status>` the `-w` format appended.
    let (body_str, status) = raw.rsplit_once('\n').unwrap_or((raw.as_ref(), ""));
    let code: u16 = status.trim().parse().unwrap_or(0);
    if !(200..300).contains(&code) {
        return Err(format!(
            "gateway returned HTTP {code}{}",
            if body_str.trim().is_empty() {
                String::new()
            } else {
                format!(" — {}", body_str.trim())
            }
        ));
    }
    serde_json::from_str(body_str).map_err(|e| format!("gateway returned non-JSON: {e}"))
}

/// Build the `/v1/resource/advertise` request body (pure — unit-tested).
fn advertise_body(
    provider: &str,
    kinds: &[String],
    snapshot: &ResourceSnapshot,
    ttl: u64,
) -> serde_json::Value {
    serde_json::json!({
        "provider": provider,
        "kinds": kinds,
        "snapshot": snapshot,
        "ttl_secs": ttl,
    })
}

/// Build the `/v1/resource/discover` query body (pure — unit-tested).
fn discover_body(
    kind: &str,
    gpu_vram: Option<u64>,
    ram: Option<u64>,
    cores: Option<u32>,
) -> serde_json::Value {
    serde_json::json!({
        "kind": kind,
        "min_vram_mb": gpu_vram,
        "min_ram_mb": ram,
        "min_cpu_cores": cores,
    })
}

// ---------------------------------------------------------------------------
// Verb: advertise (provider → broker).
// ---------------------------------------------------------------------------

fn run_advertise(args: AdvertiseArgs) -> i32 {
    let Some(root) = find_workspace_root(args.workspace) else {
        eprintln!("bwoc resource advertise: no workspace found (no .bwoc/workspace.toml)");
        return EXIT_USAGE;
    };
    let cfg = match load_sharing_config(&root) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("bwoc resource advertise: {e}");
            return EXIT_USAGE;
        }
    };
    // Master opt-in — a host that isn't sharing must not advertise an offer.
    if !cfg.share {
        eprintln!(
            "bwoc resource advertise: sharing is off — set [resource] share = true in \
             .bwoc/workspace.toml before advertising"
        );
        return EXIT_USAGE;
    }
    if cfg.caps.kinds.is_empty() {
        eprintln!(
            "bwoc resource advertise: no kinds offered — set [resource.caps] kinds = \
             [\"compute\", …] in .bwoc/workspace.toml"
        );
        return EXIT_USAGE;
    }
    let base = match resolve_gateway(args.gateway.as_deref(), &cfg) {
        Ok(b) => b,
        Err(e) => {
            eprintln!("bwoc resource advertise: {e}");
            return EXIT_USAGE;
        }
    };
    let snap = detect_snapshot();
    let body = advertise_body(&args.provider, &cfg.caps.kinds, &snap, args.ttl);
    let url = format!("{base}/v1/resource/advertise");
    match http_post_json(&url, &body) {
        Ok(resp) => {
            if args.json {
                let _ = print_json_value(&resp);
            } else {
                let live = resp
                    .get("live_offers")
                    .and_then(|v| v.as_u64())
                    .unwrap_or(0);
                println!(
                    "advertised {} ({}) → {} · {} live offer(s) on the broker",
                    args.provider,
                    cfg.caps.kinds.join(","),
                    base,
                    live
                );
            }
            EXIT_OK
        }
        Err(e) => {
            eprintln!("bwoc resource advertise: {e}");
            EXIT_BROKER_ERROR
        }
    }
}

// ---------------------------------------------------------------------------
// Verb: discover (consumer → broker).
// ---------------------------------------------------------------------------

fn run_discover(args: DiscoverArgs) -> i32 {
    if ResourceKind::parse(&args.kind).is_none() {
        eprintln!(
            "bwoc resource discover: invalid --kind '{}' — expected compute | kv | knowledge",
            args.kind
        );
        return EXIT_USAGE;
    }
    // Workspace/config is optional for discover — `--gateway` can stand alone with
    // no `[resource]` block. But a *present but malformed* workspace file is a real
    // error we must not swallow (matching advertise/gate-check); we only skip config
    // when `--gateway` makes it unnecessary.
    let cfg = match find_workspace_root(args.workspace) {
        Some(root) => match load_sharing_config(&root) {
            Ok(c) => c,
            Err(e) => {
                // A bad config is fatal unless --gateway lets us bypass it entirely.
                if args.gateway.is_none() {
                    eprintln!("bwoc resource discover: {e}");
                    return EXIT_USAGE;
                }
                SharingConfig::default()
            }
        },
        None => SharingConfig::default(),
    };
    let base = match resolve_gateway(args.gateway.as_deref(), &cfg) {
        Ok(b) => b,
        Err(e) => {
            eprintln!("bwoc resource discover: {e}");
            return EXIT_USAGE;
        }
    };
    let body = discover_body(&args.kind, args.gpu_vram, args.ram, args.cores);
    let url = format!("{base}/v1/resource/discover");
    match http_post_json(&url, &body) {
        Ok(resp) => {
            if args.json {
                let _ = print_json_value(&resp);
                return EXIT_OK;
            }
            let offers = resp.get("offers").and_then(|v| v.as_array());
            match offers {
                Some(list) if !list.is_empty() => {
                    println!("{} offer(s) for '{}':", list.len(), args.kind);
                    for o in list {
                        let provider = o.get("provider").and_then(|v| v.as_str()).unwrap_or("?");
                        let snap = o.get("snapshot").cloned().unwrap_or(serde_json::json!({}));
                        let host = snap.get("host").and_then(|v| v.as_str()).unwrap_or("?");
                        let vram = snapshot_best_vram(&snap);
                        let ram = snap
                            .get("ram_free_mb")
                            .and_then(|v| v.as_u64())
                            .unwrap_or(0);
                        let cores = snap.get("cpu_cores").and_then(|v| v.as_u64()).unwrap_or(0);
                        println!(
                            "  {provider} (host {host}) — GPU {vram} MB free · {ram} MB RAM · {cores} cores"
                        );
                    }
                }
                _ => println!("no live offers for '{}'", args.kind),
            }
            EXIT_OK
        }
        Err(e) => {
            eprintln!("bwoc resource discover: {e}");
            EXIT_BROKER_ERROR
        }
    }
}

/// Best single-GPU free VRAM in an offer snapshot (mirror of the broker's).
fn snapshot_best_vram(snap: &serde_json::Value) -> u64 {
    snap.get("gpus")
        .and_then(|v| v.as_array())
        .map(|gpus| {
            gpus.iter()
                .filter_map(|g| g.get("vram_free_mb").and_then(serde_json::Value::as_u64))
                .max()
                .unwrap_or(0)
        })
        .unwrap_or(0)
}

fn print_json_value(value: &serde_json::Value) -> bool {
    match serde_json::to_string_pretty(value) {
        Ok(s) => {
            println!("{s}");
            true
        }
        Err(_) => false,
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
    fn gateway_base_normalises_schemes() {
        assert_eq!(
            gateway_http_base("wss://gw.bemind.tech"),
            "https://gw.bemind.tech"
        );
        assert_eq!(gateway_http_base("ws://gw:8787"), "http://gw:8787");
        assert_eq!(gateway_http_base("https://gw/"), "https://gw");
        assert_eq!(gateway_http_base("http://gw:8787"), "http://gw:8787");
        assert_eq!(
            gateway_http_base("gw.bemind.tech"),
            "https://gw.bemind.tech"
        );
    }

    #[test]
    fn resolve_gateway_prefers_flag_then_config() {
        let cfg = SharingConfig {
            gateway: Some("wss://from-config".into()),
            ..Default::default()
        };
        assert_eq!(
            resolve_gateway(Some("ws://from-flag"), &cfg).unwrap(),
            "http://from-flag"
        );
        assert_eq!(resolve_gateway(None, &cfg).unwrap(), "https://from-config");
        assert!(resolve_gateway(None, &SharingConfig::default()).is_err());
    }

    #[test]
    fn advertise_and_discover_bodies_shape() {
        let snap = snap_with(40000, 96000, 128);
        let kinds = vec!["compute".to_string(), "knowledge".to_string()];
        let a = advertise_body("bemind", &kinds, &snap, 30);
        assert_eq!(a["provider"], "bemind");
        assert_eq!(a["ttl_secs"], 30);
        assert_eq!(a["kinds"][0], "compute");
        assert_eq!(a["snapshot"]["host"], "test");

        let d = discover_body("compute", Some(24000), None, Some(16));
        assert_eq!(d["kind"], "compute");
        assert_eq!(d["min_vram_mb"], 24000);
        assert!(d["min_ram_mb"].is_null());
        assert_eq!(d["min_cpu_cores"], 16);
    }

    #[test]
    fn cap_conversion_rejects_negatives() {
        assert_eq!(cap_u64(Some(&toml::Value::Integer(40000))), Some(40000));
        assert_eq!(cap_u64(Some(&toml::Value::Integer(-1))), None);
        assert_eq!(cap_u32(Some(&toml::Value::Integer(-5))), None);
        assert_eq!(cap_u64(Some(&toml::Value::String("x".into()))), None);
        assert_eq!(cap_u64(None), None);
    }

    #[test]
    fn negative_cap_does_not_wrap_to_allow() {
        let tmp = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(tmp.path().join(".bwoc")).unwrap();
        std::fs::write(
            tmp.path().join(".bwoc/workspace.toml"),
            "[workspace]\nname=\"x\"\nversion=\"0.1.0\"\n\n\
             [resource]\nshare = true\n\n\
             [resource.caps]\nmax_vram_mb = -1\nkinds = [\"compute\"]\n",
        )
        .unwrap();
        let cfg = load_sharing_config(tmp.path()).unwrap();
        // -1 must be None (unset), never a huge cap.
        assert_eq!(cfg.caps.max_vram_mb, None);
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
