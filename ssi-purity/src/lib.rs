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
    let ordering_dir = repo_root.join("src/ordering");
    purity_scan(&ordering_dir)?;
    // Declared deps are validated for shape here; the resolved tree is scanned
    // for license/source/FFI by the grader after vendoring (scan_vendored_tree).
    filter_declared_deps(&ordering_dir)?;
    license_check(repo_root, mode)?;
    Ok(())
}

/// Read and parse `<ordering_dir>/deps.toml` (absent file = no declared deps).
/// This is the submission-facing half of the dependency policy; license/source
/// and FFI enforcement over the RESOLVED transitive tree run in the grader's
/// tree scan (see `scan_vendored_tree`) after `cargo vendor`.
pub fn filter_declared_deps(ordering_dir: &Path) -> Result<Vec<DeclaredDep>, GateError> {
    let deps_toml = ordering_dir.join("deps.toml");
    let src = match std::fs::read_to_string(&deps_toml) {
        Ok(s) => s,
        Err(ref e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(e) => return Err(GateError(format!("cannot read {}: {e}", deps_toml.display()))),
    };
    parse_deps_toml(&src)
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

/// LICENSE: run `cargo-deny check licenses` against the shipped `deny.toml`. In
/// `FallbackAllowed` mode, a missing `cargo-deny` is tolerated with a printed
/// note (the submission's declared deps have already been validated for shape by
/// `filter_declared_deps`; tree-level license enforcement happens in the grader's
/// vendored-tree scan). In `RequireDeny` mode, a missing `cargo-deny` is fatal:
/// the authoritative check must run.
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
                    "license-check: cargo-deny not installed; skipping license check \
                     (declared deps were validated for shape; tree-level license enforcement \
                     happens in the grader's vendored-tree scan). \
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
}
