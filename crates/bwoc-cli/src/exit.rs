//! The single, canonical exit-code contract for the `bwoc` CLI (3.0).
//!
//! Before 3.0 each command family invented its own exit vocabulary: `bwoc check`
//! returned `1` for "violations found" while `bwoc workspace validate` returned
//! `2` for the *same* semantic — and `2` elsewhere means "usage / not found", so
//! a script could not tell "the workspace is missing" from "the workspace has
//! violations". The plugin fronts each re-declared the same `0/1/2/4/255` block.
//!
//! 3.0 unifies this into one documented table. Exit codes are the framework's
//! primary **machine** contract (every CI gate branches on `$?`), so the table
//! is a hard part of the [compatibility contract](../../../docs/en/COMPATIBILITY.en.md)
//! and changing a code is a breaking change.
//!
//! | Code | Name | Meaning |
//! |------|------|---------|
//! | `0`   | [`OK`]             | Success. |
//! | `1`   | [`ERROR`]          | The command itself errored (I/O, parse, network, internal-but-recoverable). "The tool broke." |
//! | `2`   | [`USAGE`]          | Bad invocation, or a required workspace / agent / target was **not found**. |
//! | `3`   | [`FINDINGS`]       | The command ran fine but the **answer is negative**: `check`/`validate` violations, `council` not-resolved, an `audit` with failures. "The tool worked; the result is 'no'." |
//! | `4`   | [`NO_PLUGIN`]      | A required plugin is not installed / not discoverable. |
//! | `254` | [`FAIL_COUNT_MAX`] | Ceiling for `audit run`, which additionally encodes its fail **count** (`1..=254`) in the exit code — the one deliberate exception to the table (see `audit.rs`). |
//! | `255` | [`INTERNAL`]       | Framework/internal error the caller cannot act on (broker failure, invariant break). |
//!
//! The load-bearing distinction is **`1` vs `3`**: `1` means the command could
//! not do its job; `3` means it did its job and the finding is negative. CI that
//! wants "fail the build on violations but alert differently on a crash" can now
//! tell them apart — it could not before.

/// Success.
pub const OK: i32 = 0;

/// The command itself errored (I/O, parse, network, recoverable-internal).
pub const ERROR: i32 = 1;

/// Bad invocation, or a required workspace / agent / target was not found.
pub const USAGE: i32 = 2;

/// The command ran but the answer is negative: `check`/`validate` violations,
/// `council` not-resolved, an `audit` with failures. Distinct from [`ERROR`]
/// (the command worked; the result is "no").
pub const FINDINGS: i32 = 3;

/// A required plugin is not installed / not discoverable.
pub const NO_PLUGIN: i32 = 4;

/// Ceiling for `audit run`, which encodes its fail count (`1..=254`) in the exit
/// code — the one deliberate exception to the table.
pub const FAIL_COUNT_MAX: i32 = 254;

/// Framework/internal error the caller cannot act on (broker failure, invariant
/// break).
pub const INTERNAL: i32 = 255;
