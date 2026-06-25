//! Local purity & license gate — a thin delegator to the shared `ssi-purity`
//! crate (Phase 4a). The scan logic lives in ONE place, and the grader runs
//! THIS SAME harness binary (Yukon dispatches `cargo run --release`), so the
//! gate cannot drift between local and graded runs — they are byte-identical.
//!
//! Mode is `FallbackAllowed`: if `cargo-deny` is absent the license check falls
//! back to the dependency scan. That is sound under the current zero-dependency
//! policy — the dependency scan already guarantees a submission adds no crate,
//! so there is no new license for `cargo-deny` to vet. The stricter
//! `RequireDeny` mode (mandatory `cargo-deny`) exists in `ssi-purity` for the
//! future where the dependency allowlist grows to vetted third-party crates;
//! until then it is dormant and the dependency scan is the load-bearing gate.
//!
//! HARNESS FILE — do not modify. The contract (order signature, score, gates,
//! output formats) is unchanged; only the implementation moved into ssi-purity.

use std::path::Path;

pub use ssi_purity::GateError;

/// Run the local Stage-A gate from the repo root (fallback-allowed mode).
pub fn check(repo_root: &Path) -> Result<(), GateError> {
    ssi_purity::check(repo_root, ssi_purity::Mode::FallbackAllowed)
}
