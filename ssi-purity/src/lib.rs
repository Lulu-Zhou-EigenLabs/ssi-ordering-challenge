//! Shared Stage-A purity & license gate (lifted from the harness's src/purity.rs
//! in Phase 4a so submissions are scanned by ONE implementation). The grader
//! runs this same harness binary, so both local and graded runs use the same
//! mode — `RequireDeny` — and cannot drift. Two modes exist:
//! - `Mode::RequireDeny` — `cargo-deny` MUST be installed and pass; no fallback.
//!   This is what the harness (and thus the grader) uses today: with submissions
//!   able to declare third-party crates, a dependency can carry a non-permissive
//!   license, so the authoritative license check is load-bearing.
//! - `Mode::FallbackAllowed` — if `cargo-deny` is absent, skip the license check
//!   with a printed note. Retired from active use: it was sound only under the
//!   old stdlib-only policy, where a submission added no crate and thus no new
//!   license to vet. Kept for tests / that historical contract.

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

/// Scan a `cargo vendor` output directory for native-code signals, applied
/// UNIFORMLY to every vendored crate (no trusted allowlist — the dependency
/// tree is not assumed safe, though feral is vetted).
///
/// ## What this scans, and what it deliberately does NOT
///
/// The code whose *source purity* we protect is the submission itself
/// (`src/ordering/`, scanned by `purity_scan`): that is the ordering applied to
/// solvers. For the DEPENDENCY tree, source-token purity is the wrong tool — a
/// crate may legitimately *export* a C ABI (`#[no_mangle] pub extern "C" fn`),
/// define a C-ABI fn, use a proc-macro, or set `links` purely as a
/// single-version guard (e.g. rayon-core links nothing). Rejecting those tokens
/// would bar a large, safe swath of the ecosystem while proving nothing: the
/// real guarantee that *no foreign code executes* is the no-C-compiler build
/// (Task 7) plus the no-network runtime (Task 9).
///
/// So this scan rejects only sound, near-false-positive-free signals that a
/// crate carries or compiles NON-Rust code:
///   - a `*-sys` crate name (native-library-wrapper convention — cheap early
///     signal),
///   - a prebuilt native artifact committed in the crate (`.a/.so/.dylib/.dll/
///     .lib/.o/.obj`) — a linkable blob bypasses "build from source",
///   - a C/C++-toolchain build-dependency (`cc`/`cmake`/`bindgen`/`nasm`/`gcc`/
///     `clang`) — the crate compiles native code from its build script.
/// These are hard rejections; the no-C-compiler build backstops anything a
/// static scan cannot decide, and must never be used to justify allowing what
/// the scan finds.
pub fn scan_vendored_tree(vendor_dir: &Path) -> Result<(), GateError> {
    let Ok(entries) = std::fs::read_dir(vendor_dir) else {
        return Ok(()); // no vendor dir = no third-party deps to scan
    };
    for entry in entries.flatten() {
        let crate_dir = entry.path();
        if !crate_dir.is_dir() {
            continue;
        }
        let crate_name = crate_dir.file_name().and_then(|s| s.to_str()).unwrap_or("");
        // A vendored dir is `<name>-<version>`. The version always starts with a
        // digit and a crate name never has a component starting with a digit, so
        // the split point is the LAST `-` immediately followed by an ASCII digit.
        // (A plain `rsplit_once('-')` would mis-split a hyphenated pre-release
        // version like `foo-sys-1.0.0-alpha.1`, leaving `foo-sys-1.0.0` and
        // letting a `*-sys` crate evade the check.)
        let name_no_ver = crate_version_split(crate_name);
        if name_no_ver.ends_with("-sys") {
            return Err(GateError(format!(
                "dependency-scan: `{crate_name}` is a `*-sys` native-library wrapper; \
                 dependencies must be pure Rust (no native library to link)"
            )));
        }
        // A C/C++-toolchain build-dependency compiles native code from build.rs.
        let manifest = crate_dir.join("Cargo.toml");
        if let Ok(toml) = std::fs::read_to_string(&manifest) {
            if let Some(tool) = c_build_dependency(&toml) {
                return Err(GateError(format!(
                    "dependency-scan: `{crate_name}` has a `{tool}` build-dependency; \
                     it would compile native (C/C++) code — dependencies must be pure Rust"
                )));
            }
        }
        // A prebuilt native artifact committed in the crate is a linkable blob
        // that bypasses building from source.
        if let Some(blob) = find_prebuilt_artifact(&crate_dir) {
            return Err(GateError(format!(
                "dependency-scan: `{crate_name}` ships a prebuilt native artifact ({}); \
                 dependencies must be pure Rust source",
                blob.display()
            )));
        }
    }
    Ok(())
}

/// If a Cargo manifest declares a C/C++-toolchain crate as a build-dependency,
/// return its name. Handles both the inline table form under
/// `[build-dependencies]` and the `[build-dependencies.<name>]` table form.
fn c_build_dependency(manifest: &str) -> Option<String> {
    const TOOLS: &[&str] = &["cc", "gcc", "clang", "cmake", "bindgen", "nasm"];
    let mut in_build_deps = false;
    for line in manifest.lines() {
        let t = line.trim();
        // A `[build-dependencies.<name>]` table header, including the
        // target-conditional form `[target.'cfg(...)'.build-dependencies.<name>]`.
        if let Some(inner) = t.strip_prefix('[').and_then(|s| s.strip_suffix(']')) {
            let build_dep_name = inner
                .strip_prefix("build-dependencies.")
                .or_else(|| {
                    inner
                        .rsplit_once(".build-dependencies.")
                        .map(|(_, name)| name)
                });
            if let Some(name) = build_dep_name {
                let name = name.trim();
                if TOOLS.contains(&name) {
                    return Some(name.to_string());
                }
                in_build_deps = false;
                continue;
            }
        }
        if t.starts_with('[') {
            // A `[build-dependencies]` table or its target-conditional form
            // `[target.'cfg(...)'.build-dependencies]`. Anything else closes the
            // current build-deps table.
            in_build_deps = t == "[build-dependencies]"
                || (t.starts_with("[target.") && t.ends_with(".build-dependencies]"));
            continue;
        }
        if in_build_deps && !t.is_empty() && !t.starts_with('#') {
            // The dep name is the key before `=`/whitespace, with any dotted
            // suffix stripped so `cc.workspace = true` / `cc.version = "1"` are
            // recognized as `cc`.
            let name = t
                .split(|c| c == '=' || c == ' ' || c == '\t')
                .next()
                .unwrap_or("")
                .trim()
                .split('.')
                .next()
                .unwrap_or("");
            if TOOLS.contains(&name) {
                return Some(name.to_string());
            }
        }
    }
    None
}

/// Walk a crate directory for a committed prebuilt native artifact (a linkable
/// object/library). Returns the first such path found, or `None`.
///
/// Matches by final extension. This catches the standard linkable forms; a
/// versioned shared object (`libfoo.so.1.2`) whose final component is not one of
/// these is not detected by extension alone, but such binaries are not expected
/// in a source-distributed vendored crate, and the no-C-compiler build (Task 7)
/// backstops any attempt to actually link native code at build time.
fn find_prebuilt_artifact(dir: &Path) -> Option<PathBuf> {
    const EXTS: &[&str] = &["a", "so", "dylib", "dll", "lib", "o", "obj"];
    let mut stack = vec![dir.to_path_buf()];
    while let Some(d) = stack.pop() {
        let Ok(entries) = std::fs::read_dir(&d) else {
            continue;
        };
        for e in entries.flatten() {
            let p = e.path();
            if p.is_dir() {
                stack.push(p);
            } else if let Some(ext) = p.extension().and_then(|s| s.to_str()) {
                if EXTS.contains(&ext) {
                    return Some(p);
                }
            }
        }
    }
    None
}

/// Strip the trailing `-<version>` from a `cargo vendor` directory name, yielding
/// the crate name. The version always begins with an ASCII digit and crate-name
/// components never do, so the boundary is the LAST `-` followed by a digit. This
/// handles hyphenated pre-release versions (`foo-1.0.0-alpha.1` → `foo`) that a
/// naive last-hyphen split would get wrong. Returns the whole string if no such
/// boundary exists (e.g. a name with no version suffix).
fn crate_version_split(dir_name: &str) -> &str {
    let bytes = dir_name.as_bytes();
    for i in (0..bytes.len().saturating_sub(1)).rev() {
        if bytes[i] == b'-' && bytes[i + 1].is_ascii_digit() {
            return &dir_name[..i];
        }
    }
    dir_name
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

    #[test]
    fn require_deny_mode_errors_on_missing_deny_toml() {
        // With crates allowed, the license check is load-bearing. RequireDeny
        // must surface a missing deny.toml as an error (not silently skip it).
        let tmp = std::env::temp_dir().join("ssi-requiredeny");
        let _ = std::fs::create_dir_all(tmp.join("src/ordering"));
        let _ = std::fs::remove_file(tmp.join("deny.toml"));
        assert!(check(&tmp, Mode::RequireDeny).is_err());
    }
}
