//! `bwoc set <name>` — update an incarnated agent's backend and/or model in
//! place. The missing CRUD "update" verb: `new` creates, `retire` removes,
//! `set` mutates.
//!
//! It touches only the two **structured** sources of truth — never the prose in
//! `AGENTS.md`:
//!   - `--backend` → the `backend` field of the `.bwoc/agents.toml` registry
//!     entry (which CLI `bwoc run` spawns). All backend symlinks already exist
//!     (`bwoc new` force-creates the full set), so a switch needs no file
//!     changes beyond the registry field.
//!   - `--primary-model` / `--fallback-model` → the agent's
//!     `config.manifest.json` (the runtime reads the model from here).
//!
//! At least one field must be given; unchanged values are reported as no-ops.

use std::path::PathBuf;

use bwoc_core::manifest::Manifest;
use bwoc_core::workspace::AgentsRegistry;

use crate::spawn::Backend;

pub struct SetArgs {
    pub name: String,
    pub backend: Option<Backend>,
    pub primary_model: Option<String>,
    pub fallback_model: Option<String>,
    pub workspace: Option<PathBuf>,
    pub json: bool,
}

/// Entry point — returns the process exit code.
pub fn run(args: SetArgs) -> i32 {
    if args.backend.is_none() && args.primary_model.is_none() && args.fallback_model.is_none() {
        eprintln!(
            "bwoc set: nothing to change — pass --backend and/or --primary-model / --fallback-model"
        );
        return 2;
    }
    let Some(root) = resolve_workspace(args.workspace) else {
        eprintln!(
            "bwoc set: no workspace (no .bwoc/workspace.toml in cwd or ancestors). \
             Pass --workspace or set BWOC_WORKSPACE."
        );
        return 2;
    };
    let mut registry = match AgentsRegistry::load(&root) {
        Ok(r) => r,
        Err(e) => {
            eprintln!("bwoc set: cannot read registry: {e}");
            return 2;
        }
    };

    // Match by id first ("agent-foo"), then by bare name ("foo" → agent-foo).
    let lookup_id = if args.name.starts_with("agent-") {
        args.name.clone()
    } else {
        format!("agent-{}", args.name)
    };
    let Some(idx) = registry.agents.iter().position(|a| a.id == lookup_id) else {
        eprintln!(
            "bwoc set: no agent named '{}' in workspace {}",
            args.name,
            root.display()
        );
        return 1;
    };
    let manifest_path = root
        .join(&registry.agents[idx].path)
        .join("config.manifest.json");

    // --- backend → registry entry ---
    let mut backend_change: Option<(String, String)> = None;
    if let Some(b) = args.backend {
        let new = b.display_name().to_string();
        let old = registry.agents[idx].backend.clone();
        if old != new {
            registry.agents[idx].backend = new.clone();
            backend_change = Some((old, new));
        }
    }

    // --- model(s) → manifest ---
    let mut primary_change: Option<(String, String)> = None;
    let mut fallback_change: Option<(Option<String>, Option<String>)> = None;
    if args.primary_model.is_some() || args.fallback_model.is_some() {
        let mut m = match Manifest::load_from_path(&manifest_path) {
            Ok(m) => m,
            Err(e) => {
                eprintln!(
                    "bwoc set: cannot read manifest {}: {e}",
                    manifest_path.display()
                );
                return 2;
            }
        };
        if let Some(pm) = &args.primary_model {
            if &m.primary_model != pm {
                primary_change = Some((m.primary_model.clone(), pm.clone()));
                m.primary_model = pm.clone();
            }
        }
        if let Some(fm) = &args.fallback_model {
            if m.fallback_model.as_deref() != Some(fm.as_str()) {
                fallback_change = Some((m.fallback_model.clone(), Some(fm.clone())));
                m.fallback_model = Some(fm.clone());
            }
        }
        if primary_change.is_some() || fallback_change.is_some() {
            if let Err(e) = m.save_to_path(&manifest_path) {
                eprintln!("bwoc set: cannot write manifest: {e}");
                return 2;
            }
        }
    }

    let registry_updated = backend_change.is_some();
    if registry_updated {
        if let Err(e) = registry.save(&root) {
            eprintln!("bwoc set: cannot write registry: {e}");
            return 2;
        }
    }
    let manifest_updated = primary_change.is_some() || fallback_change.is_some();

    if args.json {
        let mut out = serde_json::json!({
            "agent": lookup_id,
            "workspace": root.display().to_string(),
            "registry_updated": registry_updated,
            "manifest_updated": manifest_updated,
        });
        if let Some((o, n)) = &backend_change {
            out["backend"] = serde_json::json!({ "from": o, "to": n });
        }
        if let Some((o, n)) = &primary_change {
            out["primary_model"] = serde_json::json!({ "from": o, "to": n });
        }
        if let Some((o, n)) = &fallback_change {
            out["fallback_model"] = serde_json::json!({ "from": o, "to": n });
        }
        println!("{}", serde_json::to_string_pretty(&out).unwrap_or_default());
    } else if !registry_updated && !manifest_updated {
        println!("✓ {lookup_id}: no change — values already match");
    } else {
        println!("✓ updated {lookup_id}");
        if let Some((o, n)) = &backend_change {
            println!("  backend:        {o} → {n}");
        }
        if let Some((o, n)) = &primary_change {
            println!("  primaryModel:   {o} → {n}");
        }
        if let Some((o, n)) = &fallback_change {
            let show = |v: &Option<String>| v.clone().unwrap_or_else(|| "(none)".into());
            println!("  fallbackModel:  {} → {}", show(o), show(n));
        }
    }
    0
}

/// Standard workspace resolution: explicit flag > `BWOC_WORKSPACE` > ancestor
/// walk for `.bwoc/workspace.toml`.
fn resolve_workspace(explicit: Option<PathBuf>) -> Option<PathBuf> {
    if let Some(p) = explicit {
        return Some(p);
    }
    if let Ok(env_path) = std::env::var("BWOC_WORKSPACE") {
        if !env_path.is_empty() {
            return Some(PathBuf::from(env_path));
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
