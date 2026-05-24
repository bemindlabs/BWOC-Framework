//! `bwoc trust <agent>` — read an agent's Kalyāṇamitta-7 trust profile.
//! `bwoc trust keygen [--agent <path>]` — generate an ed25519 keypair (Trust v2 / HV2-4).
//! See `modules/agent-template/interconnect/trust.md`.

use std::path::PathBuf;

use bwoc_core::manifest::{Manifest, TrustBlock, TrustDeclared};
use bwoc_core::signing;
use bwoc_core::workspace::AgentsRegistry;

pub struct TrustArgs {
    pub agent: String,
    pub workspace: Option<PathBuf>,
    pub json: bool,
    /// When `Some`, run `keygen` for the named agent instead of showing the profile.
    pub keygen: Option<KeygenArgs>,
}

pub struct KeygenArgs {
    /// Overwrite an existing key without prompting.
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
    if let Some(kg) = args.keygen {
        return run_keygen(&args.agent, args.workspace, kg);
    }

    let Some(workspace) = resolve_workspace(args.workspace) else {
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
    let lookup_id = if args.agent.starts_with("agent-") {
        args.agent.clone()
    } else {
        format!("agent-{}", args.agent)
    };
    let Some(entry) = registry.agents.iter().find(|a| a.id == lookup_id) else {
        eprintln!(
            "bwoc trust: no agent named '{}' in workspace {}.",
            args.agent,
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

/// `bwoc trust keygen [--agent <name>] [--force]`
///
/// Generates an ed25519 keypair for the named agent:
/// - Private key → `<agent>/.bwoc/agent.key` (hex, mode 0600).
/// - Public key → `trust.publicKey` in `<agent>/config.manifest.json`.
///
/// Idempotent: refuses to overwrite an existing key unless `--force`.
fn run_keygen(agent_name: &str, workspace: Option<PathBuf>, kg: KeygenArgs) -> i32 {
    let Some(ws) = resolve_workspace(workspace) else {
        eprintln!(
            "bwoc trust keygen: no workspace found. Pass --workspace, set BWOC_WORKSPACE, \
             or run `bwoc init`."
        );
        return 2;
    };
    let registry = match AgentsRegistry::load(&ws) {
        Ok(r) => r,
        Err(e) => {
            eprintln!("bwoc trust keygen: failed to read agents.toml: {e}");
            return 1;
        }
    };
    let lookup_id = if agent_name.starts_with("agent-") {
        agent_name.to_string()
    } else {
        format!("agent-{agent_name}")
    };
    let Some(entry) = registry.agents.iter().find(|a| a.id == lookup_id) else {
        eprintln!(
            "bwoc trust keygen: no agent named '{agent_name}' in workspace {}.",
            ws.display()
        );
        return 2;
    };

    let agent_dir = ws.join(&entry.path);
    let bwoc_dir = agent_dir.join(".bwoc");
    let manifest_path = agent_dir.join("config.manifest.json");

    // Generate keypair (writes private key to .bwoc/agent.key).
    let pubkey_hex = match signing::generate_keypair(&bwoc_dir, kg.force) {
        Ok(pk) => pk,
        Err(signing::SigningError::KeyExists(p)) => {
            eprintln!(
                "bwoc trust keygen: key already exists at {}. \
                 Pass --force to overwrite.",
                p.display()
            );
            return 2;
        }
        Err(e) => {
            eprintln!("bwoc trust keygen: failed to generate keypair: {e}");
            return 1;
        }
    };

    // Patch trust.publicKey in config.manifest.json.
    let mut manifest = match Manifest::load_from_path(&manifest_path) {
        Ok(m) => m,
        Err(e) => {
            eprintln!(
                "bwoc trust keygen: failed to read {}: {e}",
                manifest_path.display()
            );
            return 1;
        }
    };
    let trust = manifest.trust.get_or_insert_with(|| TrustBlock {
        schema_version: 1,
        ..TrustBlock::default()
    });
    trust.public_key = Some(pubkey_hex.clone());

    if let Err(e) = manifest.save_to_path(&manifest_path) {
        eprintln!(
            "bwoc trust keygen: failed to write {}: {e}",
            manifest_path.display()
        );
        return 1;
    }

    println!();
    println!("Keypair generated for {}", entry.id);
    println!(
        "  Private key: {}/.bwoc/agent.key (mode 0600)",
        agent_dir.display()
    );
    println!("  Public key:  {pubkey_hex}");
    println!(
        "  Manifest:    {} (trust.publicKey updated)",
        manifest_path.display()
    );
    println!();
    println!("The private key is gitignored by convention (.bwoc/agent.key).");
    println!("Add .bwoc/agent.key to your .gitignore if not already present.");
    println!();
    0
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
            println!();
            match &t.public_key {
                None => println!(
                    "publicKey: (none — no keypair generated; run `bwoc trust keygen {agent_id}`)"
                ),
                Some(pk) => println!("publicKey: {pk}"),
            }
            println!("requireSignature: {}", t.require_signature);
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
                "publicKey": t.public_key,
                "requireSignature": t.require_signature,
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
