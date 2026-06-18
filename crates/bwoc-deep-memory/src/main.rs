//! `bwoc-deep-memory` — Tier 2 deep-memory reference binary.
//!
//! Speaks the `bwoc-core::deep_memory` contract (`wake-up` | `search` | `mine`)
//! over a local SQLite store with embedding-based recall. Thin shell: parse
//! args, resolve config, build the store + embedder, call the library verb,
//! print. All real logic lives in the library so it stays unit-tested offline.

use std::path::PathBuf;
use std::process::ExitCode;

use bwoc_deep_memory::embed::HttpEmbedder;
use bwoc_deep_memory::store::Store;
use bwoc_deep_memory::{mine, prune, render, search, wake_up};
use clap::{Parser, Subcommand};

/// Default embedding endpoint (local Ollama / OpenAI-compatible).
const DEFAULT_EMBED_URL: &str = "http://localhost:11434";
/// Default embedding model.
const DEFAULT_EMBED_MODEL: &str = "nomic-embed-text";
/// Default store location, relative to the working directory.
const DEFAULT_DB: &str = ".bwoc/deep-memory.db";

#[derive(Parser)]
#[command(
    name = "bwoc-deep-memory",
    about = "BWOC Tier 2 deep-memory reference: wake-up | search | mine over a local SQLite + embedding store",
    version
)]
struct Cli {
    /// SQLite store path. Env: BWOC_DEEP_MEMORY_DB. Default: .bwoc/deep-memory.db
    #[arg(long, global = true)]
    db: Option<PathBuf>,

    /// Embedding endpoint root (OpenAI-compatible). Env: BWOC_EMBED_URL.
    #[arg(long, global = true)]
    embed_url: Option<String>,

    /// Embedding model id. Env: BWOC_EMBED_MODEL.
    #[arg(long, global = true)]
    embed_model: Option<String>,

    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Emit recent context at session start.
    WakeUp {
        /// Max memories to emit.
        #[arg(long, default_value_t = 10)]
        limit: usize,
    },
    /// Find relevant past memories for a query.
    Search {
        /// The query text.
        query: String,
        /// Max results.
        #[arg(long, default_value_t = 5)]
        limit: usize,
    },
    /// Persist session learnings under a path into the store.
    Mine {
        /// File or directory of session artifacts to ingest.
        path: PathBuf,
        /// Tool-defined mode tag stored with each memory.
        #[arg(long, default_value = "convos")]
        mode: String,
    },
    /// Apply a retention policy: drop old or excess memories.
    ///
    /// Specify at least one of `--older-than-days` / `--keep` (they combine as
    /// a union). Operator/cron command — not part of the recall contract.
    Prune {
        /// Delete memories older than N days.
        #[arg(long)]
        older_than_days: Option<u32>,
        /// Keep only the newest N memories; delete the rest.
        #[arg(long)]
        keep: Option<usize>,
        /// Report what would be deleted without deleting.
        #[arg(long)]
        dry_run: bool,
    },
}

fn main() -> ExitCode {
    let cli = Cli::parse();

    let db = cli
        .db
        .or_else(|| std::env::var_os("BWOC_DEEP_MEMORY_DB").map(PathBuf::from))
        .unwrap_or_else(|| PathBuf::from(DEFAULT_DB));

    let store = match Store::open(&db) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("bwoc-deep-memory: cannot open store {}: {e}", db.display());
            return ExitCode::FAILURE;
        }
    };

    // The embedder is only built for verbs that need it (`mine`, `search`).
    let make_embedder = || {
        let url = cli
            .embed_url
            .clone()
            .or_else(|| std::env::var("BWOC_EMBED_URL").ok())
            .unwrap_or_else(|| DEFAULT_EMBED_URL.to_string());
        let model = cli
            .embed_model
            .clone()
            .or_else(|| std::env::var("BWOC_EMBED_MODEL").ok())
            .unwrap_or_else(|| DEFAULT_EMBED_MODEL.to_string());
        let api_key = std::env::var("BWOC_EMBED_API_KEY")
            .ok()
            .filter(|k| !k.is_empty());
        HttpEmbedder::new(url, model, api_key)
    };

    match cli.command {
        Command::WakeUp { limit } => match wake_up(&store, limit) {
            Ok(mems) => {
                print!("{}", render(&mems, false));
                ExitCode::SUCCESS
            }
            Err(e) => fail("wake-up", e),
        },
        Command::Search { query, limit } => {
            let emb = make_embedder();
            match search(&store, &emb, &query, limit) {
                Ok(mems) => {
                    print!("{}", render(&mems, true));
                    ExitCode::SUCCESS
                }
                Err(e) => fail("search", e),
            }
        }
        Command::Mine { path, mode } => {
            let emb = make_embedder();
            match mine(&store, &emb, &path, &mode, now_unix()) {
                Ok(report) => {
                    println!(
                        "mined {} chunk(s) from {} file(s) → {}",
                        report.stored,
                        report.files,
                        db.display()
                    );
                    if report.redacted > 0 {
                        println!("redacted {} secret(s) before storing", report.redacted);
                    }
                    ExitCode::SUCCESS
                }
                Err(e) => fail("mine", e),
            }
        }
        Command::Prune {
            older_than_days,
            keep,
            dry_run,
        } => {
            if older_than_days.is_none() && keep.is_none() {
                eprintln!(
                    "bwoc-deep-memory prune: specify at least one of \
                     --older-than-days / --keep"
                );
                return ExitCode::FAILURE;
            }
            let older_than = older_than_days.map(|d| now_unix() - i64::from(d) * 86_400);
            match prune(&store, older_than, keep, dry_run) {
                Ok(n) => {
                    let verb = if dry_run { "would prune" } else { "pruned" };
                    println!("{verb} {n} memory(ies) from {}", db.display());
                    ExitCode::SUCCESS
                }
                Err(e) => fail("prune", e),
            }
        }
    }
}

fn fail(verb: &str, e: impl std::fmt::Display) -> ExitCode {
    eprintln!("bwoc-deep-memory {verb}: {e}");
    ExitCode::FAILURE
}

/// Current Unix time in seconds (0 if the clock predates the epoch).
fn now_unix() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}
