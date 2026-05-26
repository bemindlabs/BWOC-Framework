//! `bwoc trust <agent>` — read an agent's Kalyāṇamitta-7 trust profile.
//! See `modules/agent-template/interconnect/trust.md`. Read-only.

use std::path::{Path, PathBuf};

use bwoc_core::manifest::{Manifest, TrustDeclared};
use bwoc_core::workspace::AgentsRegistry;

pub struct TrustArgs {
    /// Optional — required for the read path; with `--keygen --all` it may be absent.
    pub agent: Option<String>,
    pub workspace: Option<PathBuf>,
    pub json: bool,
    /// Generate an ed25519 signing keypair (HV2-4) instead of reading the profile.
    pub keygen: bool,
    /// With `--keygen`: every registered agent (backfill).
    pub all: bool,
    /// With `--keygen`: overwrite an existing key (rotates identity).
    pub force: bool,
}

/// The 7 Kalyāṇamitta qualities in canonical (manifest-key) order.
const QUALITIES: &[(&str, &str)] = &[
    ("piyo", "Pleasant to delegate to"),
    ("garu", "Respectable in capability"),
    ("bhavaniyo", "Helps us improve"),
    ("vatta", "Speaks beneficial truth"),
    ("vacanakkhamo", "Can take feedback"),
    ("gambhira", "Can explain depth"),
    ("noCatthana", "Does not lead astray"),
];

pub fn run(args: TrustArgs) -> i32 {
    let Some(workspace) = resolve_workspace(args.workspace.clone()) else {
        eprintln!(
            "bwoc trust: no workspace found. Pass --workspace, set BWOC_WORKSPACE, \
             or run `bwoc init`."
        );
        return 2;
    };
    let registry = match AgentsRegistry::load(&workspace) {
        Ok(r) => r,
        Err(e) => {
            eprintln!("bwoc trust: failed to read agents.toml: {e}");
            return 1;
        }
    };

    // ── keygen path (HV2-4) ───────────────────────────────────────────────
    if args.keygen {
        return keygen(
            &workspace,
            &registry,
            args.agent.as_deref(),
            args.all,
            args.force,
        );
    }

    // ── read path (Kalyāṇamitta-7 profile) ────────────────────────────────
    let Some(agent) = args.agent.as_deref() else {
        eprintln!("bwoc trust: an agent name is required (or use `--keygen --all`).");
        return 2;
    };
    let lookup_id = canonical_id(agent);
    let Some(entry) = registry.agents.iter().find(|a| a.id == lookup_id) else {
        eprintln!(
            "bwoc trust: no agent named '{}' in workspace {}.",
            agent,
            workspace.display()
        );
        return 2;
    };
    let manifest_path = workspace.join(&entry.path).join("config.manifest.json");
    let manifest = match Manifest::load_from_path(&manifest_path) {
        Ok(m) => m,
        Err(e) => {
            eprintln!(
                "bwoc trust: failed to read {}: {e}",
                manifest_path.display()
            );
            return 1;
        }
    };

    if args.json {
        print_json(&entry.id, &manifest);
    } else {
        print_human(&entry.id, &manifest);
    }
    0
}

/// `bwoc trust --keygen [agent | --all]` — generate ed25519 signing keypair(s)
/// (HV2-4): private key → `<agent>/.bwoc/agent.key` (0600), public key →
/// manifest `trust.signingPublicKey`. Backfills existing agents so enforce-mode
/// messaging works.
fn keygen(
    workspace: &Path,
    registry: &AgentsRegistry,
    agent: Option<&str>,
    all: bool,
    force: bool,
) -> i32 {
    let targets: Vec<_> = if all {
        registry.agents.iter().collect()
    } else {
        let Some(a) = agent else {
            eprintln!("bwoc trust --keygen: pass an agent name or --all.");
            return 2;
        };
        let id = canonical_id(a);
        match registry.agents.iter().find(|e| e.id == id) {
            Some(e) => vec![e],
            None => {
                eprintln!("bwoc trust: no agent named '{a}' in workspace.");
                return 2;
            }
        }
    };

    let (mut generated, mut skipped, mut failed) = (0u32, 0u32, 0u32);
    for entry in targets {
        let bwoc_dir = workspace.join(&entry.path).join(".bwoc");
        match bwoc_signing::generate_keypair(&bwoc_dir, force) {
            Ok(pubkey) => {
                let manifest_path = workspace.join(&entry.path).join("config.manifest.json");
                match publish_pubkey(&manifest_path, &pubkey) {
                    Ok(()) => {
                        println!("✓ {} — keypair generated, public key published", entry.id);
                        generated += 1;
                    }
                    Err(e) => {
                        eprintln!(
                            "✗ {} — key written but manifest update failed: {e}",
                            entry.id
                        );
                        failed += 1;
                    }
                }
            }
            Err(bwoc_signing::SigningError::KeyExists(_)) => {
                println!(
                    "· {} — key already exists (use --force to rotate)",
                    entry.id
                );
                skipped += 1;
            }
            Err(e) => {
                eprintln!("✗ {} — keygen failed: {e}", entry.id);
                failed += 1;
            }
        }
    }
    println!("\nkeygen: {generated} generated, {skipped} skipped, {failed} failed");
    i32::from(failed > 0)
}

/// Load the manifest, set `trust.signingPublicKey`, and save it back.
fn publish_pubkey(manifest_path: &Path, pubkey_hex: &str) -> Result<(), String> {
    let mut m = Manifest::load_from_path(manifest_path).map_err(|e| e.to_string())?;
    let mut block = m.trust.take().unwrap_or_default();
    block.signing_public_key = Some(pubkey_hex.to_string());
    m.trust = Some(block);
    m.save_to_path(manifest_path).map_err(|e| e.to_string())
}

/// Resolve a bare name or full id to the canonical `agent-<name>` id.
fn canonical_id(name: &str) -> String {
    if name.starts_with("agent-") {
        name.to_string()
    } else {
        format!("agent-{name}")
    }
}

fn print_human(agent_id: &str, m: &Manifest) {
    println!();
    println!("Trust profile: {agent_id}");
    println!("================");
    println!();
    match m.trust.as_ref() {
        None => {
            println!("(no trust block declared — recipient ships permissive)");
            println!();
            println!("Add a `trust` block to config.manifest.json to opt in.");
            println!("Spec: modules/agent-template/interconnect/trust.md");
        }
        Some(t) => {
            println!("schemaVersion: {}", t.schema_version);
            println!();
            println!("Declared (Kalyāṇamitta 7):");
            for (key, gloss) in QUALITIES {
                let v = bool_field(&t.declared, key);
                let mark = if v { "✓" } else { "·" };
                println!("  {mark} {:<14} {gloss}", key);
            }
            println!();
            if t.required_trust.is_empty() {
                println!("requiredTrust: (empty — no gating; recipient accepts all)");
            } else {
                println!("requiredTrust:");
                for q in &t.required_trust {
                    let known = QUALITIES.iter().any(|(k, _)| *k == q);
                    let mark = if known { " " } else { "?" };
                    println!("  {mark} {q}");
                }
            }
        }
    }
    println!();
}

fn print_json(agent_id: &str, m: &Manifest) {
    let value = match m.trust.as_ref() {
        None => serde_json::json!({ "agent": agent_id, "trust": null }),
        Some(t) => serde_json::json!({
            "agent": agent_id,
            "trust": {
                "schemaVersion": t.schema_version,
                "declared": {
                    "piyo": t.declared.piyo,
                    "garu": t.declared.garu,
                    "bhavaniyo": t.declared.bhavaniyo,
                    "vatta": t.declared.vatta,
                    "vacanakkhamo": t.declared.vacanakkhamo,
                    "gambhira": t.declared.gambhira,
                    "noCatthana": t.declared.no_catthana,
                },
                "requiredTrust": t.required_trust,
            },
        }),
    };
    println!(
        "{}",
        serde_json::to_string(&value).unwrap_or_else(|_| "{}".into())
    );
}

fn bool_field(d: &TrustDeclared, key: &str) -> bool {
    match key {
        "piyo" => d.piyo,
        "garu" => d.garu,
        "bhavaniyo" => d.bhavaniyo,
        "vatta" => d.vatta,
        "vacanakkhamo" => d.vacanakkhamo,
        "gambhira" => d.gambhira,
        "noCatthana" => d.no_catthana,
        _ => false,
    }
}

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
