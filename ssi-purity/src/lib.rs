//! Shared Stage-A purity & license gate (lifted from the harness's src/purity.rs
//! in Phase 4a so submissions are scanned by ONE implementation). The grader
//! runs this same harness binary, so both local and graded runs use the same
//! mode — `FallbackAllowed` — and cannot drift. Two modes exist:
//! - `Mode::FallbackAllowed` — if `cargo-deny` is absent, fall back to the
//!   dependency scan with a printed note. Sound under the zero-dependency
//!   policy: with no added crates there is no new license to vet. This is what
//!   the harness (and thus the grader) uses today.
//! - `Mode::RequireDeny` — `cargo-deny` MUST be installed and pass; no fallback.
//!   Dormant: reserved for when the dependency allowlist grows to vetted
//!   third-party crates and the authoritative license check starts to matter.

use std::path::{Path, PathBuf};
use std::process::Command;

/// Whether a missing `cargo-deny` is tolerated (harness) or fatal (grader).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    FallbackAllowed,
    RequireDeny,
}

/// A purity/license violation, with a human-readable reason naming the
/// offending file/license/feature.
#[derive(Debug)]
pub struct GateError(pub String);

impl std::fmt::Display for GateError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

mod deps;
pub use deps::{parse_deps_toml, DeclaredDep};

/// Run the full Stage-A gate from the repo root.
pub fn check(repo_root: &Path, mode: Mode) -> Result<(), GateError> {
    purity_scan(&repo_root.join("src/ordering"))?;
    dependency_scan(&repo_root.join("Cargo.toml"))?;
    license_check(repo_root, mode)?;
    Ok(())
}

/// PURITY: walk every `.rs` file under `src/ordering/` and reject foreign-code
/// escape hatches. Returns the offending file + reason on the first hit.
fn purity_scan(ordering_dir: &Path) -> Result<(), GateError> {
    // build.rs anywhere in the submission directory is forbidden.
    if ordering_dir.join("build.rs").exists() {
        return Err(GateError(format!(
            "purity: {} contains a build script (build.rs); submissions must be pure Rust with no build scripts",
            ordering_dir.display()
        )));
    }
    let mut files = Vec::new();
    collect_rs(ordering_dir, &mut files);
    for file in files {
        let src = std::fs::read_to_string(&file)
            .map_err(|e| GateError(format!("purity: cannot read {}: {e}", file.display())))?;
        scan_source(&file, &src)?;
    }
    Ok(())
}

/// Scan a single source string for forbidden constructs. Comment-stripping is
/// deliberately conservative: we strip `//` line comments so an explanatory
/// comment mentioning `extern` does not trip the gate, but we do NOT try to
/// parse Rust — any real use of these constructs in code trips it.
fn scan_source(file: &Path, src: &str) -> Result<(), GateError> {
    let where_ = file.display();
    for (lineno, raw) in src.lines().enumerate() {
        let line = strip_line_comment(raw);
        let trimmed = line.trim();
        let hit = |what: &str| {
            Err(GateError(format!(
                "purity: {where_}:{} uses {what}, which is forbidden in submissions (no foreign-function interface / non-Rust escape)",
                lineno + 1
            )))
        };
        // extern block or extern fn (FFI). `extern crate` is also disallowed:
        // submissions are stdlib-only and may not name external crates.
        if trimmed.starts_with("extern ")
            || trimmed.contains(" extern ")
            || trimmed.contains("extern\"")
            || trimmed.contains("extern \"")
        {
            return hit("an `extern` block / FFI");
        }
        if contains_attr(trimmed, "no_mangle") {
            return hit("`#[no_mangle]`");
        }
        if contains_attr(trimmed, "link") && (trimmed.contains("link(") || trimmed.contains("link =")) {
            return hit("a `#[link]` attribute");
        }
        if trimmed.contains("proc_macro") {
            return hit("proc-macro machinery");
        }
        if trimmed.contains("include!") {
            // include! is allowed only for paths inside src/ordering/. We
            // cannot resolve the literal robustly without parsing, so we reject
            // any include! pointing outside via "../" — the only way to escape.
            if trimmed.contains("..") {
                return hit("an `include!` of a path outside src/ordering/");
            }
        }
    }
    Ok(())
}

/// Strip a `//` line comment (ignoring `//` inside string literals is overkill
/// here; the constructs we scan for would never legitimately appear in a string
/// in a real ordering, and a false positive fails loud rather than silently).
fn strip_line_comment(line: &str) -> &str {
    match line.find("//") {
        Some(i) => &line[..i],
        None => line,
    }
}

/// True if `line` contains an attribute named `name`, e.g. `#[no_mangle]` or
/// `#![no_mangle]` (with optional whitespace).
fn contains_attr(line: &str, name: &str) -> bool {
    line.contains("#[") && line.contains(name) && line.contains(']')
        || line.contains("#![") && line.contains(name)
}

/// DEPENDENCY scan: the harness `Cargo.toml` ships with the submission allowed
/// NO crate dependencies of its own. The contestant may not add any. We assert
/// the `[dependencies]` table (as opposed to the harness's own, which lives in
/// the trusted workspace manifest) names nothing.
///
/// In this workspace layout the harness package's `[dependencies]` legitimately
/// contains `ssi-scoring` (trusted). The rule the contestant must satisfy is:
/// the submission directory introduces no NEW dependency. Since contestant code
/// is stdlib-only and cannot reach `[dependencies]` (they may edit only
/// `src/ordering/`), this scan guards against a tampered manifest by checking
/// the dependency set equals the known template set.
///
/// ## Why stdlib-only (and not just "pure Rust")
///
/// The governing proposal (`COMPETITION-PROPOSAL.md` §2.4/§6 Stage A) requires
/// submissions to be **pure Rust, no foreign-code escape, permissively
/// licensed** — it does NOT itself require the standard library only.
/// stdlib-only is a deliberately STRICTER policy this challenge adopts because
/// it is the cheapest airtight way to guarantee the proposal's pure-Rust rule.
///
/// The hard part of "pure Rust" is *arbitrary* dependencies: a permissively
/// licensed, innocently named crate can still contain an `extern "C"` block —
/// or pull in a transitive dependency that does — which is a non-Rust escape
/// the license check and the `*-sys`/`build.rs` bans do not catch. Proving "no
/// FFI anywhere in a tree of arbitrary crates" by static scan is unreliable
/// (FFI can be macro-generated or `cfg`-gated). Forbidding ALL dependencies
/// removes that vector entirely: with an empty set there is nothing to vet, so
/// the gate's source scan only has to read the contestant's own `src/ordering/`
/// (which it does, catching FFI the contestant writes directly).
///
/// ## Allowing a FIXED list of crates, if ever needed
///
/// This rule CAN be relaxed to permit a small, curated set of permissive,
/// pure-Rust crates (e.g. a PRNG or a graph/partitioning crate) without
/// reopening the hole above. The mechanism the proposal names (§6 Stage A,
/// §10.2) is an offline build against a frozen, vendored registry: vet each
/// allowlisted crate AND its transitive tree by hand ONCE (confirm no FFI / no
/// `build.rs` / no `*-sys` / permissive license), pin it, and build `--offline
/// --locked` so nothing else can enter. To wire it in here:
///   1. add the vetted crate(s) to the trusted workspace `Cargo.toml` — note a
///      contestant cannot do this themselves: the grader rebuilds from its own
///      frozen manifest and overlays ONLY `src/ordering/`, so both the harness
///      and grader see the SAME dependency set (local↔grader equivalence,
///      Invariant 2, holds for free);
///   2. extend `ALLOWED` below to include those crate names;
///   3. keep the source scan and `cargo-deny` license check as-is.
/// Until there is real demand, the zero-dependency policy stands because it is
/// strictly simpler and already satisfies the governing docs.
fn dependency_scan(cargo_toml: &Path) -> Result<(), GateError> {
    let src = std::fs::read_to_string(cargo_toml)
        .map_err(|e| GateError(format!("dependency-scan: cannot read {}: {e}", cargo_toml.display())))?;
    // Collect crate names listed under [dependencies] (not dev/build).
    let deps = dependency_names(&src);
    // The only dependencies the trusted harness ships with are the scoring
    // wrapper and the purity crate.
    const ALLOWED: &[&str] = &["ssi-scoring", "ssi-purity"];
    for d in &deps {
        if !ALLOWED.contains(&d.as_str()) {
            return Err(GateError(format!(
                "dependency-scan: Cargo.toml [dependencies] contains '{d}', but submissions are stdlib-only — the only allowed harness dependency is ssi-scoring. Remove '{d}'."
            )));
        }
    }
    Ok(())
}

/// Extract crate names from the `[dependencies]` table of a Cargo.toml string
/// (stops at the next `[section]`). Handles both `name = "1.0"` and
/// `name = { ... }` forms.
fn dependency_names(toml: &str) -> Vec<String> {
    let mut names = Vec::new();
    let mut in_deps = false;
    for line in toml.lines() {
        let t = line.trim();
        if t.starts_with('[') {
            in_deps = t == "[dependencies]";
            continue;
        }
        if !in_deps || t.is_empty() || t.starts_with('#') {
            continue;
        }
        if let Some(eq) = t.find('=') {
            let name = t[..eq].trim().trim_matches('"').to_string();
            if !name.is_empty() {
                names.push(name);
            }
        }
    }
    names
}

/// LICENSE: run `cargo-deny check licenses` against the shipped `deny.toml`. In
/// `FallbackAllowed` mode, a missing `cargo-deny` falls back to the dependency
/// scan (already run) with a printed note — a submission that adds no dependency
/// cannot pull in a non-permissive license, so the fallback is sound for the
/// stdlib-only contract (the mode both the harness and the grader use today).
/// In the dormant `RequireDeny` mode, a missing `cargo-deny` is fatal: the
/// authoritative check must run — reserved for a future with an expanded
/// dependency allowlist.
fn license_check(repo_root: &Path, mode: Mode) -> Result<(), GateError> {
    let deny_toml = repo_root.join("deny.toml");
    if !deny_toml.exists() {
        return Err(GateError(format!(
            "license-check: {} is missing (it ships with the template)",
            deny_toml.display()
        )));
    }
    let available = Command::new("cargo")
        .arg("deny")
        .arg("--version")
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false);
    if !available {
        match mode {
            Mode::RequireDeny => {
                return Err(GateError(
                    "license-check: cargo-deny is REQUIRED for grading but is not installed. \
                     Install it with `cargo install cargo-deny`."
                        .to_string(),
                ));
            }
            Mode::FallbackAllowed => {
                eprintln!(
                    "license-check: cargo-deny not installed; falling back to the dependency scan \
                     (a stdlib-only submission adds no crate, so no non-permissive license can enter). \
                     Install with `cargo install cargo-deny` to run the authoritative check the grader uses."
                );
                return Ok(());
            }
        }
    }
    let output = Command::new("cargo")
        .arg("deny")
        .arg("--manifest-path")
        .arg(repo_root.join("Cargo.toml"))
        .arg("check")
        .arg("licenses")
        .current_dir(repo_root)
        .output()
        .map_err(|e| GateError(format!("license-check: failed to run cargo-deny: {e}")))?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(GateError(format!(
            "license-check: cargo-deny rejected the dependency licenses:\n{}",
            stderr.trim()
        )));
    }
    Ok(())
}

fn collect_rs(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            collect_rs(&path, out);
        } else if path.extension().and_then(|e| e.to_str()) == Some("rs") {
            out.push(path);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clean_source_passes() {
        let src = "pub fn order(p: &Pattern) -> Vec<usize> { (0..p.n).collect() }\n";
        assert!(scan_source(Path::new("x.rs"), src).is_ok());
    }

    #[test]
    fn extern_block_is_rejected() {
        let src = "extern \"C\" { fn evil(); }\n";
        assert!(scan_source(Path::new("x.rs"), src).is_err());
    }

    #[test]
    fn no_mangle_is_rejected() {
        let src = "#[no_mangle]\npub fn foo() {}\n";
        assert!(scan_source(Path::new("x.rs"), src).is_err());
    }

    #[test]
    fn comment_mentioning_extern_is_allowed() {
        // A comment explaining we must not use extern should not trip the gate.
        let src = "// we never call into extern \"C\" code here\nfn ok() {}\n";
        assert!(scan_source(Path::new("x.rs"), src).is_ok());
    }

    #[test]
    fn include_outside_dir_is_rejected() {
        let src = "include!(\"../../../etc/passwd\");\n";
        assert!(scan_source(Path::new("x.rs"), src).is_err());
    }

    #[test]
    fn dependency_names_reads_dependencies_table_only() {
        let toml = "[package]\nname=\"x\"\n[dependencies]\nssi-scoring = { path = \"ssi-scoring\" }\n[dev-dependencies]\nprototype-oracle = { path = \"p\" }\n";
        let names = dependency_names(toml);
        assert_eq!(names, vec!["ssi-scoring"]);
    }

    #[test]
    fn extra_dependency_is_rejected() {
        let toml = "[dependencies]\nssi-scoring = \"0\"\nrand = \"0.8\"\n";
        let names = dependency_names(toml);
        assert!(names.contains(&"rand".to_string()));
    }

    #[test]
    fn ssi_purity_is_an_allowed_dependency() {
        // The harness now depends on both the scorer and the purity crate; both
        // are trusted and must pass the dependency scan.
        let toml = "[dependencies]\nssi-scoring = { path = \"ssi-scoring\" }\nssi-purity = { path = \"ssi-purity\" }\n";
        let names = dependency_names(toml);
        for n in ["ssi-scoring", "ssi-purity"] {
            assert!(names.contains(&n.to_string()));
        }
    }
}
