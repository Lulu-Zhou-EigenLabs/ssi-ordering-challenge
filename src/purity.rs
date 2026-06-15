//! Local purity & license gate — a thin delegator to the shared `ssi-purity`
//! crate (Phase 4a). The scan logic lives in ONE place so the local harness and
//! the private grader cannot drift. The harness runs in `FallbackAllowed` mode
//! (a missing `cargo-deny` falls back to the dependency scan, since a
//! stdlib-only submission adds no crate); the grader runs in `RequireDeny` mode.
//!
//! HARNESS FILE — do not modify. The contract (order signature, score, gates,
//! output formats) is unchanged; only the implementation moved into ssi-purity.

use std::path::Path;

pub use ssi_purity::GateError;

/// Run the local Stage-A gate from the repo root (fallback-allowed mode).
pub fn check(repo_root: &Path) -> Result<(), GateError> {
    ssi_purity::check(repo_root, ssi_purity::Mode::FallbackAllowed)
}
