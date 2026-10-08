//! `bwoc fleet term` — open a terminal for every agent in the fleet and arrange
//! the panes with a chosen layout.
//!
//! Portable across **macOS + Linux**: tmux is the one terminal multiplexer both
//! ship (or install identically), and its `select-layout` gives the layout
//! arrangements for free — equal grid, columns, rows, or a main + stack. Each
//! agent gets its own pane running `bwoc spawn` in the agent's directory, with
//! the pane border titled by the agent id so the grid stays legible.
//!
//! The multiplexer calls live behind [`crate::pane_backend::PaneBackend`] —
//! tmux by default, herdr with `--backend herdr` or `[fleet] pane_backend =
//! "herdr"`; this module owns workspace/registry resolution, backend choice,
//! and the open-or-attach policy.
//!
//! OS-native window tiling (separate Ghostty / Terminal.app windows positioned
//! on the desktop) is a mac-only follow-up — deliberately not this command.

use std::io::IsTerminal;
use std::path::PathBuf;

use bwoc_core::workspace::AgentsRegistry;

use crate::chat::resolve_workspace;
use crate::pane_backend::{FleetAgent, PaneBackend, PaneBackendKind};
use crate::spawn;

/// The pane arrangement, backend-neutral. Each maps to a built-in tmux layout
/// (one `select-layout`) and to a herdr split tree
/// (`crate::herdr_backend::layout_tree`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, clap::ValueEnum)]
pub enum PaneLayout {
    /// Equal tiles in a grid — tmux `tiled`. The default; scales best past ~4 agents.
    Grid,
    /// One column per agent, side by side — tmux `even-horizontal`.
    Columns,
    /// One row per agent, stacked top to bottom — tmux `even-vertical`.
    Rows,
    /// A large main pane on the left, the rest stacked on the right — tmux `main-vertical`.
    MainVertical,
    /// A large main pane on top, the rest in a row below — tmux `main-horizontal`.
    MainHorizontal,
}

impl PaneLayout {
    pub(crate) fn tmux_name(self) -> &'static str {
        match self {
            PaneLayout::Grid => "tiled",
            PaneLayout::Columns => "even-horizontal",
            PaneLayout::Rows => "even-vertical",
            PaneLayout::MainVertical => "main-vertical",
            PaneLayout::MainHorizontal => "main-horizontal",
        }
    }
}

pub struct FleetTermArgs {
    pub workspace: Option<PathBuf>,
    pub layout: PaneLayout,
    /// `--backend`; `None` → the workspace's `[fleet] pane_backend`, else tmux.
    pub backend: Option<PaneBackendKind>,
    /// tmux session name. `None` → a per-workspace default
    /// (`default_session_name`) so concurrent fleets don't collide.
    pub session: Option<String>,
    /// Build the session but do not attach — just print the attach command.
    /// Implied when stdout is not a TTY (e.g. a script / the control center).
    pub print: bool,
}

/// Deterministic, tmux-safe default session name for a fleet, unique per
/// workspace so two fleets opened at the same time don't collide on a shared
/// `bwoc-fleet`. Shape: `bwoc-fleet-<slug>-<hash>` where `slug` is the
/// workspace directory basename and `hash` is an FNV-1a digest of the canonical
/// path (disambiguates same-named dirs). Deterministic, so re-running in the
/// same workspace targets the *same* session (attach), not a new one.
pub(crate) fn default_session_name(workspace: &std::path::Path) -> String {
    // Canonicalize so the name is stable regardless of the cwd the command ran
    // from (relative vs absolute path resolving to the same workspace root).
    let canon = std::fs::canonicalize(workspace).unwrap_or_else(|_| workspace.to_path_buf());
    let slug = canon
        .file_name()
        .and_then(|s| s.to_str())
        .map(sanitize_segment)
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "ws".to_string());
    let hash = fnv1a_24(canon.to_string_lossy().as_bytes());
    format!("bwoc-fleet-{slug}-{hash}")
}

/// Lower-case, keep `[a-z0-9-_]`, map the rest to `-`, trim stray `-`, cap at 24.
/// tmux session names must not contain `.` or `:`; this keeps them safe + legible.
fn sanitize_segment(s: &str) -> String {
    let mut out: String = s
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '-' || c == '_' {
                c.to_ascii_lowercase()
            } else {
                '-'
            }
        })
        .collect();
    out.truncate(24);
    out.trim_matches('-').to_string()
}

/// FNV-1a over `bytes`, folded to the low 24 bits → 6 hex chars. Stable across
/// runs (fixed constants), so the derived session name is reproducible.
fn fnv1a_24(bytes: &[u8]) -> String {
    let mut h: u32 = 0x811c_9dc5;
    for &b in bytes {
        h ^= u32::from(b);
        h = h.wrapping_mul(0x0100_0193);
    }
    format!("{:06x}", h & 0x00ff_ffff)
}

/// Attach to `session` (or, with `print` / no TTY, just print the attach line).
fn attach_session(backend: &dyn PaneBackend, session: &str, print: bool) -> i32 {
    if print || !std::io::stdout().is_terminal() {
        println!("Attach with:  {}", backend.attach_hint(session));
        return 0;
    }
    match backend.attach_fleet(session) {
        Ok(()) => 0,
        Err(e) => {
            eprintln!(
                "bwoc fleet term: attach failed: {e} — run `{}` manually.",
                backend.attach_hint(session)
            );
            1
        }
    }
}

pub fn run(args: FleetTermArgs) -> i32 {
    let Some(workspace) = resolve_workspace(args.workspace) else {
        eprintln!(
            "bwoc fleet term: no workspace found (no .bwoc/workspace.toml in cwd or ancestors). \
             Pass --workspace, set BWOC_WORKSPACE, or run `bwoc init` first."
        );
        return 2;
    };
    // No explicit --session → a per-workspace default so two fleets running at
    // once (different workspaces) never collide on a shared `bwoc-fleet` name.
    let session = args
        .session
        .clone()
        .unwrap_or_else(|| default_session_name(&workspace));
    let registry = match AgentsRegistry::load(&workspace) {
        Ok(r) => r,
        Err(e) => {
            eprintln!("bwoc fleet term: failed to read agents registry: {e}");
            return 1;
        }
    };
    if registry.agents.is_empty() {
        eprintln!(
            "bwoc fleet term: no agents in {} — incarnate one with `bwoc new`.",
            workspace.display()
        );
        return 2;
    }

    let agents: Vec<FleetAgent> = registry
        .agents
        .iter()
        .map(|a| FleetAgent {
            id: a.id.clone(),
            path: workspace.join(&a.path).to_string_lossy().into_owned(),
            backend: a.backend.clone(),
        })
        .collect();

    let kind = resolve_kind(args.backend, &workspace);
    let backend =
        crate::pane_backend::backend_for(kind, &workspace, crate::herdr_backend::FLEET_BUDGET);
    open_with(
        backend.as_ref(),
        &session,
        args.session.is_some(),
        &agents,
        args.layout,
        &spawn::bwoc_exe(),
        args.print,
    )
}

/// The `--backend` flag when given, else the workspace's `[fleet] pane_backend`
/// (tmux when absent or unrecognised).
fn resolve_kind(flag: Option<PaneBackendKind>, workspace: &std::path::Path) -> PaneBackendKind {
    flag.unwrap_or_else(|| crate::pane_backend::configured_kind(workspace, "fleet term"))
}

/// Open (or re-attach) the fleet through `backend`. Split from [`run`] so the
/// call sequence is testable against a recording backend.
fn open_with(
    backend: &dyn PaneBackend,
    session: &str,
    explicit_session: bool,
    agents: &[FleetAgent],
    layout: PaneLayout,
    bwoc_exe: &str,
    print: bool,
) -> i32 {
    if let Err(hint) = backend.check_available() {
        eprintln!("bwoc fleet term: {hint}");
        return 2;
    }
    if backend.fleet_exists(session) {
        // An explicit --session that's taken is a user/input error (they named
        // it). But the auto-derived per-workspace default already running means
        // "this fleet is already open" — attach it (idempotent re-run) rather
        // than refuse.
        if explicit_session {
            eprintln!(
                "bwoc fleet term: {} session '{session}' already exists — attach with `{}`, \
                 kill it with `{}`, or pass a different --session.",
                backend.name(),
                backend.attach_hint(session),
                backend.kill_hint(session)
            );
            return 2;
        }
        println!("Fleet already open for this workspace (session '{session}') — attaching.");
        return attach_session(backend, session, print);
    }

    match backend.open_fleet(session, agents, layout, bwoc_exe) {
        Ok(confirmation) => println!("{confirmation}"),
        Err(detail) => {
            eprintln!("bwoc fleet term: {detail}");
            return 1;
        }
    }

    // Attach unless asked not to (or no TTY to attach to).
    attach_session(backend, session, print)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn layout_names_map_to_tmux() {
        assert_eq!(PaneLayout::Grid.tmux_name(), "tiled");
        assert_eq!(PaneLayout::Rows.tmux_name(), "even-vertical");
        assert_eq!(PaneLayout::MainVertical.tmux_name(), "main-vertical");
        assert_eq!(PaneLayout::MainHorizontal.tmux_name(), "main-horizontal");
    }

    #[test]
    fn sanitize_segment_is_tmux_safe() {
        // Dots/colons/spaces (tmux-special or ugly) become '-', trimmed + capped.
        assert_eq!(sanitize_segment("My Fleet.v2"), "my-fleet-v2");
        assert_eq!(sanitize_segment("bwoc"), "bwoc");
        assert_eq!(sanitize_segment("--weird--"), "weird");
        assert_eq!(sanitize_segment(""), "");
        let long = sanitize_segment(&"a".repeat(50));
        assert!(long.len() <= 24, "capped: {}", long.len());
    }

    #[test]
    fn fnv1a_24_is_deterministic_and_six_hex() {
        let a = fnv1a_24(b"/Users/x/ws-one");
        assert_eq!(a, fnv1a_24(b"/Users/x/ws-one"), "stable across calls");
        assert_ne!(a, fnv1a_24(b"/Users/x/ws-two"), "differs by path");
        assert_eq!(a.len(), 6);
        assert!(a.chars().all(|c| c.is_ascii_hexdigit()));
    }

    #[test]
    fn default_session_name_is_unique_per_workspace() {
        let base = std::env::temp_dir().join(format!("bwoc-ftname-{}", std::process::id()));
        let a = base.join("fleet-a");
        let b = base.join("fleet-b");
        std::fs::create_dir_all(&a).unwrap();
        std::fs::create_dir_all(&b).unwrap();

        let na = default_session_name(&a);
        let nb = default_session_name(&b);
        // Shape + tmux-safety.
        assert!(na.starts_with("bwoc-fleet-"), "{na}");
        assert!(!na.contains('.') && !na.contains(':'));
        // Two different fleets → different sessions (no collision when concurrent).
        assert_ne!(na, nb);
        // Deterministic: same workspace re-run → same session (attach, not new).
        assert_eq!(na, default_session_name(&a));
        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn backend_flag_overrides_the_workspace_key() {
        let ws = std::env::temp_dir().join(format!("bwoc-ftkind-{}", std::process::id()));
        std::fs::create_dir_all(ws.join(".bwoc")).unwrap();
        let toml = |extra: &str| {
            std::fs::write(
                ws.join(".bwoc/workspace.toml"),
                format!("[workspace]\nname = 'd'\nversion = '0'\ncreated = 'x'\n{extra}"),
            )
            .unwrap();
        };
        toml("");
        assert_eq!(resolve_kind(None, &ws), PaneBackendKind::Tmux, "default");
        toml("[fleet]\npane_backend = 'herdr'\n");
        assert_eq!(
            resolve_kind(None, &ws),
            PaneBackendKind::Herdr,
            "from config"
        );
        assert_eq!(
            resolve_kind(Some(PaneBackendKind::Tmux), &ws),
            PaneBackendKind::Tmux,
            "flag beats config"
        );
        toml("[fleet]\npane_backend = 'screen'\n");
        assert_eq!(
            resolve_kind(None, &ws),
            PaneBackendKind::Tmux,
            "unknown → tmux"
        );
        assert_eq!(
            resolve_kind(Some(PaneBackendKind::Herdr), &ws),
            PaneBackendKind::Herdr
        );
        let _ = std::fs::remove_dir_all(&ws);
    }

    // --- call sequence through the PaneBackend trait (recording fake) ---

    use crate::pane_backend::fake::FakeBackend;

    fn fleet() -> Vec<FleetAgent> {
        vec![
            FleetAgent {
                id: "agent-pi".into(),
                path: "/ws/agents/agent-pi".into(),
                backend: "claude".into(),
            },
            FleetAgent {
                id: "agent-ji".into(),
                path: "/ws/agents/agent-ji".into(),
                backend: "ollama".into(),
            },
        ]
    }

    #[test]
    fn fresh_fleet_checks_opens_then_prints_attach() {
        let b = FakeBackend::default();
        let rc = open_with(
            &b,
            "s1",
            false,
            &fleet(),
            PaneLayout::Rows,
            "/bin/bwoc",
            true,
        );
        assert_eq!(rc, 0);
        assert_eq!(
            b.calls(),
            vec![
                "check_available",
                "fleet_exists s1",
                "open_fleet s1 [agent-pi@/ws/agents/agent-pi:claude,\
                 agent-ji@/ws/agents/agent-ji:ollama] Rows /bin/bwoc",
                "attach_hint s1",
            ]
        );
    }

    #[test]
    fn unavailable_backend_refuses_before_touching_anything() {
        let b = FakeBackend {
            unavailable: true,
            ..Default::default()
        };
        assert_eq!(
            open_with(&b, "s1", false, &fleet(), PaneLayout::Grid, "bwoc", true),
            2
        );
        assert_eq!(b.calls(), vec!["check_available"]);
    }

    #[test]
    fn explicit_session_taken_is_refused_without_opening() {
        let b = FakeBackend {
            exists: true,
            ..Default::default()
        };
        assert_eq!(
            open_with(&b, "mine", true, &fleet(), PaneLayout::Grid, "bwoc", true),
            2
        );
        let calls = b.calls();
        assert!(
            !calls.iter().any(|c| c.starts_with("open_fleet")),
            "{calls:?}"
        );
        assert!(calls.contains(&"kill_hint mine".to_string()), "{calls:?}");
    }

    #[test]
    fn default_session_already_open_reattaches_without_opening() {
        let b = FakeBackend {
            exists: true,
            ..Default::default()
        };
        assert_eq!(
            open_with(&b, "auto", false, &fleet(), PaneLayout::Grid, "bwoc", true),
            0
        );
        assert_eq!(
            b.calls(),
            vec!["check_available", "fleet_exists auto", "attach_hint auto"]
        );
    }

    #[test]
    fn open_failure_exits_1_and_skips_attach() {
        let b = FakeBackend {
            open_fails: true,
            ..Default::default()
        };
        assert_eq!(
            open_with(&b, "s1", false, &fleet(), PaneLayout::Grid, "bwoc", true),
            1
        );
        let calls = b.calls();
        assert!(calls.last().unwrap().starts_with("open_fleet"), "{calls:?}");
    }
}
