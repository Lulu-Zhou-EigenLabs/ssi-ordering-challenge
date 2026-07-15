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

/// C/C++-toolchain crates whose presence as a build-dependency means the crate
/// compiles native code from its build script.
const C_BUILD_TOOLS: &[&str] = &["cc", "gcc", "clang", "cmake", "bindgen", "nasm"];

/// If a Cargo manifest declares a C/C++-toolchain crate as a build-dependency,
/// return its name. Handles the inline form under `[build-dependencies]`, the
/// `[build-dependencies.<name>]` table form (incl. target-conditional
/// `[target.'cfg(...)'.build-dependencies...]`), dotted keys
/// (`cc.workspace = true`), AND the renamed form
/// (`mycc = { package = "cc" }` / a `package = "cc"` line under a table header),
/// which would otherwise smuggle a C-toolchain crate under a benign key.
fn c_build_dependency(manifest: &str) -> Option<String> {
    const TOOLS: &[&str] = C_BUILD_TOOLS;
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
                // Keep scanning: a `package = "cc"` line under this header would
                // rename a C-toolchain crate to a benign key.
                in_build_deps = true;
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
            // A `package = "<tool>"` rename, in an inline table
            // (`mycc = { package = "cc" }`) or as a line under a
            // `[build-dependencies.<name>]` header (`package = "cc"`).
            if let Some(pkg) = package_rename_to_tool(t, TOOLS) {
                return Some(pkg);
            }
        }
    }
    None
}

/// If a build-dependency line renames a C-toolchain crate via `package = "cc"`
/// (etc.) — the crates.io rename mechanism — return that tool name. Matches a
/// `package = "<tool>"` fragment anywhere in the line (covers both an inline
/// table `mycc = { package = "cc", ... }` and a bare `package = "cc"` under a
/// `[build-dependencies.<name>]` header).
fn package_rename_to_tool(line: &str, tools: &[&str]) -> Option<String> {
    let idx = line.find("package")?;
    let after = line[idx + "package".len()..].trim_start();
    let after = after.strip_prefix('=')?.trim_start();
    // Value is a quoted string: "cc" or 'cc'.
    let quote = after.chars().next()?;
    if quote != '"' && quote != '\'' {
        return None;
    }
    let rest = &after[1..];
    let end = rest.find(quote)?;
    let pkg = &rest[..end];
    if tools.contains(&pkg) {
        Some(pkg.to_string())
    } else {
        None
    }
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

/// Scan a single source string for forbidden constructs. We strip BOTH `//` line
/// comments and `/* */` block/doc comments first, so prose that merely mentions
/// `extern`/`proc_macro` (a very common case at public scale) does not trip the
/// gate. String literals are tracked while stripping so a comment marker inside a
/// string does not start a "comment" — that avoids a bypass where forbidden code
/// hides after a `/*` embedded in a string. We do NOT try to fully parse Rust:
/// any real use of these constructs in code still trips the gate.
fn scan_source(file: &Path, src: &str) -> Result<(), GateError> {
    let where_ = file.display();
    // Two views of the source with comments removed: `kw` also blanks string
    // interiors (so a keyword inside a string is not matched); `paths` keeps
    // string contents (so `include!("../x")` still shows its `..` escape).
    let kw = strip_comments(src, true);
    let paths = strip_comments(src, false);
    let hit = |lineno: usize, what: &str| {
        Err(GateError(format!(
            "purity: {where_}:{} uses {what}, which is forbidden in submissions (no foreign-function interface / non-Rust escape)",
            lineno + 1
        )))
    };
    for (lineno, line) in kw.lines().enumerate() {
        let trimmed = line.trim();
        // `extern`, `no_mangle`, and `proc_macro` are single contiguous tokens
        // (whitespace can never split a keyword/identifier), so a whole-word match
        // per line catches them however the SURROUNDING tokens are spread across
        // lines — closing the block-comment / line-split evasions where the ABI
        // string or the `#[ ]` brackets sat on another line. `extern` is a
        // reserved keyword (never a valid identifier), so word-matching it cannot
        // false-positive on a benign name; `no_mangle`/`proc_macro` are matched on
        // identifier boundaries so `no_mangle_helper` / `count_proc_macro` pass.
        if contains_word(trimmed, "extern") {
            return hit(lineno, "an `extern` block / FFI");
        }
        if contains_word(trimmed, "no_mangle") {
            return hit(lineno, "`#[no_mangle]`");
        }
        if contains_word(trimmed, "proc_macro") {
            return hit(lineno, "proc-macro machinery");
        }
        // `#[link(...)]` / `#[link_name = ...]`. Unlike the keywords above, `link`
        // is a legal identifier (`let link = ...`, `x.link()`), so it is matched
        // only in attribute form — `#[` and the `link(`/`link =`/`link_name`
        // fragment on the same line. A `#[link]` split across physical lines is
        // NOT caught here; that is acceptable because `#[link]` is inert without an
        // `extern` block (now caught robustly above) and the no-native-link build
        // (Task 7) backstops any actual native linking.
        if contains_attr(trimmed, "link")
            && (trimmed.contains("link(")
                || trimmed.contains("link =")
                || trimmed.contains("link_name"))
        {
            return hit(lineno, "a `#[link]` attribute");
        }
    }
    // `include!` is allowed only for paths inside src/ordering/. We cannot resolve
    // the literal robustly without parsing, so we reject any `include!` whose line
    // also contains `..` — the only way to escape the directory. This runs on the
    // string-preserving view so the `..` inside the path literal is visible.
    for (lineno, line) in paths.lines().enumerate() {
        let trimmed = line.trim();
        if trimmed.contains("include!") && trimmed.contains("..") {
            return hit(lineno, "an `include!` of a path outside src/ordering/");
        }
    }
    Ok(())
}

/// Strip `//` line comments and `/* */` block comments (both nest, per Rust) from
/// a whole source string, preserving newlines so line numbers reported for real
/// hits stay accurate. Comment markers INSIDE string literals are not treated as
/// comment starts, and string-delimiter characters inside a comment are ignored —
/// otherwise a `/*` in a string (or a `"` in a comment) could hide forbidden code
/// from the scanner. Normal (`"..."`), byte (`b"..."`), and raw (`r#"..."#`)
/// strings are handled, as are char literals (`'x'`, `'"'`, `b'x'`): a char
/// literal is consumed whole so a `"` inside it does not open a phantom string,
/// while a bare `'` that is NOT a complete char literal (a lifetime like `&'a T`)
/// is left as an ordinary byte.
///
/// String INTERIORS are additionally blanked (each interior byte becomes a space,
/// newlines preserved), so a forbidden token appearing only inside a string
/// literal — e.g. `return Err("extern calls are not allowed")` — is not matched.
/// This is a false-positive fix, not a bypass: string contents are data, never
/// code, so blanking them can only remove non-code tokens. Char-literal interiors
/// are left as-is (at most one char/escape — they cannot spell a forbidden word).
///
/// `blank_strings` controls that interior blanking: `true` for the keyword scan
/// (so a token inside a string is not matched), `false` for the `include!` scan,
/// which must read the string PATH to see a `..` escape — blanking it would hide
/// the escape and open a bypass.
fn strip_comments(src: &str, blank_strings: bool) -> String {
    let bytes = src.as_bytes();
    let mut out = String::with_capacity(src.len());
    let mut i = 0;
    // Nesting depth of `/* */` block comments (0 = not in a block comment).
    let mut block_depth = 0usize;
    // Inside a `"..."`/`b"..."` string literal (not raw).
    let mut in_string = false;
    // Inside a raw string `r#"..."#`; `raw_hashes` is the number of `#` that must
    // precede the closing `"` to end it.
    let mut in_raw = false;
    let mut raw_hashes = 0usize;
    while i < bytes.len() {
        let b = bytes[i];
        if block_depth > 0 {
            // Only comment markers matter inside a block comment.
            if b == b'/' && i + 1 < bytes.len() && bytes[i + 1] == b'*' {
                block_depth += 1;
                i += 2;
            } else if b == b'*' && i + 1 < bytes.len() && bytes[i + 1] == b'/' {
                block_depth -= 1;
                i += 2;
            } else {
                if b == b'\n' {
                    out.push('\n'); // keep line numbering
                }
                i += 1;
            }
            continue;
        }
        if in_raw {
            if b == b'"' {
                // A raw string ends at `"` followed by exactly `raw_hashes` `#`.
                let mut j = i + 1;
                let mut seen = 0;
                while j < bytes.len() && bytes[j] == b'#' && seen < raw_hashes {
                    seen += 1;
                    j += 1;
                }
                if seen == raw_hashes {
                    out.push('"'); // keep closing delimiter
                    for _ in 0..raw_hashes {
                        out.push('#');
                    }
                    in_raw = false;
                    i = j;
                    continue;
                }
            }
            // Interior byte: blank it (data, not code) when requested, always
            // preserving newlines.
            out.push(blank_or_keep(b, blank_strings));
            i += 1;
            continue;
        }
        if in_string {
            if b == b'\\' && i + 1 < bytes.len() {
                // Escaped pair (e.g. `\"`): keep it whole so it can't close the
                // string; blank both bytes when blanking so content isn't matched.
                out.push(blank_or_keep(b'\\', blank_strings));
                out.push(blank_or_keep(bytes[i + 1], blank_strings));
                i += 2;
                continue;
            }
            if b == b'"' {
                out.push('"'); // keep closing delimiter
                in_string = false;
                i += 1;
                continue;
            }
            // Interior byte: blank it when requested, preserving newlines.
            out.push(blank_or_keep(b, blank_strings));
            i += 1;
            continue;
        }
        // Not in a comment or string: look for the start of one.
        if b == b'/' && i + 1 < bytes.len() && bytes[i + 1] == b'/' {
            // Line comment: skip to end of line, keeping the newline.
            while i < bytes.len() && bytes[i] != b'\n' {
                i += 1;
            }
            continue;
        }
        if b == b'/' && i + 1 < bytes.len() && bytes[i + 1] == b'*' {
            block_depth = 1;
            i += 2;
            continue;
        }
        // Raw string start: `r"`, `r#"`, `br"`, `br#"`, etc.
        if let Some((hashes, len)) = raw_string_start(&bytes[i..]) {
            in_raw = true;
            raw_hashes = hashes;
            for k in 0..len {
                out.push(bytes[i + k] as char);
            }
            i += len;
            continue;
        }
        // Char literal `'x'` / `'\n'` / `'\''`, optionally a byte-char `b'x'`.
        // Consumed whole so a `"` inside it (`'"'`) does not open a string, and
        // its closing `'` is not mistaken for a lifetime tick. A bare `'` that is
        // NOT a complete char literal (a lifetime like `'a`) falls through and is
        // copied as an ordinary byte.
        {
            let quote_at = if b == b'b' && bytes.get(i + 1) == Some(&b'\'') {
                Some(i + 1)
            } else if b == b'\'' {
                Some(i)
            } else {
                None
            };
            if let Some(q) = quote_at {
                if let Some(clen) = char_literal_len(&bytes[q..]) {
                    let total = (q - i) + clen;
                    for k in 0..total {
                        out.push(bytes[i + k] as char);
                    }
                    i += total;
                    continue;
                }
            }
        }
        if b == b'"' {
            in_string = true;
            out.push('"');
            i += 1;
            continue;
        }
        out.push(b as char);
        i += 1;
    }
    out
}

/// A string-interior byte becomes a space when `blank` is set (newlines always
/// kept so line numbering survives); otherwise it is copied verbatim.
fn blank_or_keep(b: u8, blank: bool) -> char {
    if b == b'\n' {
        '\n'
    } else if blank {
        ' '
    } else {
        b as char
    }
}

/// If `bytes` starts with a raw-string opener (`r"`, `r#"`, `r##"`, …, optionally
/// prefixed by `b` for a raw byte string), return `(number_of_hashes, prefix_len)`
/// where `prefix_len` covers everything through the opening `"`. Otherwise `None`.
fn raw_string_start(bytes: &[u8]) -> Option<(usize, usize)> {
    let mut i = 0;
    if i < bytes.len() && bytes[i] == b'b' {
        i += 1;
    }
    if i >= bytes.len() || bytes[i] != b'r' {
        return None;
    }
    i += 1;
    let mut hashes = 0;
    while i < bytes.len() && bytes[i] == b'#' {
        hashes += 1;
        i += 1;
    }
    if i < bytes.len() && bytes[i] == b'"' {
        Some((hashes, i + 1))
    } else {
        None
    }
}

/// If `bytes` starts with a char literal (`'x'`, `'\n'`, `'\''`, `'\u{1f}'`),
/// return its total byte length including both quotes. Returns `None` when the
/// leading `'` opens a lifetime/label (`'a`, `'static`) rather than a char
/// literal — the presence of a closing `'` after exactly one char (or one escape)
/// is what distinguishes the two, exactly as Rust's own grammar does. The caller
/// handles a `b'...'` byte-char prefix by pointing us at the `'`.
fn char_literal_len(bytes: &[u8]) -> Option<usize> {
    if bytes.first() != Some(&b'\'') {
        return None;
    }
    let mut i = 1;
    if bytes.get(i) == Some(&b'\\') {
        // Escaped: `\n`, `\'`, `\\`, `\xHH`, `\u{...}`, etc.
        i += 1;
        match bytes.get(i) {
            Some(b'u') => {
                i += 1;
                if bytes.get(i) != Some(&b'{') {
                    return None;
                }
                i += 1;
                while i < bytes.len() && bytes[i] != b'}' {
                    i += 1;
                }
                if bytes.get(i) != Some(&b'}') {
                    return None;
                }
                i += 1;
            }
            Some(b'x') => {
                i += 1;
                let mut k = 0;
                while k < 2 && bytes.get(i).is_some_and(|b| b.is_ascii_hexdigit()) {
                    i += 1;
                    k += 1;
                }
            }
            Some(_) => i += 1, // single-char escape
            None => return None,
        }
    } else {
        // A single (possibly multi-byte UTF-8) char, then the closing `'` must
        // follow immediately — otherwise this is a lifetime, not a char literal.
        match bytes.get(i) {
            Some(&lead) => i += utf8_len(lead),
            None => return None,
        }
    }
    if bytes.get(i) == Some(&b'\'') {
        Some(i + 1)
    } else {
        None
    }
}

/// Byte length of the UTF-8 code point whose leading byte is `lead`.
fn utf8_len(lead: u8) -> usize {
    if lead < 0x80 {
        1
    } else if lead >> 5 == 0b110 {
        2
    } else if lead >> 4 == 0b1110 {
        3
    } else if lead >> 3 == 0b11110 {
        4
    } else {
        1 // continuation/invalid byte: treat as 1 so we never over-consume
    }
}

/// True if `hay` contains `word` as a whole identifier — i.e. not immediately
/// preceded or followed by an identifier character (`[A-Za-z0-9_]`). Used so a
/// forbidden token matches only as its own identifier, never as a substring of a
/// larger benign name.
fn contains_word(hay: &str, word: &str) -> bool {
    let hb = hay.as_bytes();
    let wb = word.as_bytes();
    if wb.is_empty() {
        return false;
    }
    let mut start = 0;
    while let Some(rel) = hay[start..].find(word) {
        let idx = start + rel;
        let before_ok = idx == 0 || !is_ident_byte(hb[idx - 1]);
        let after = idx + wb.len();
        let after_ok = after >= hb.len() || !is_ident_byte(hb[after]);
        if before_ok && after_ok {
            return true;
        }
        start = idx + 1;
    }
    false
}

fn is_ident_byte(b: u8) -> bool {
    b == b'_' || b.is_ascii_alphanumeric()
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
    // Check BOTH licenses and sources: `sources` enforces the crates.io-only
    // policy (deny.toml `unknown-git = "deny"`, empty `allow-git`) so a tampered
    // manifest cannot reintroduce a git/path source. `licenses` enforces the
    // permissive allow-list. Both run against the frozen lockfile — no network.
    let output = Command::new("cargo")
        .arg("deny")
        .arg("--manifest-path")
        .arg(repo_root.join("Cargo.toml"))
        .arg("check")
        .arg("licenses")
        .arg("sources")
        .current_dir(repo_root)
        .output()
        .map_err(|e| GateError(format!("license-check: failed to run cargo-deny: {e}")))?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(GateError(format!(
            "license-check: cargo-deny rejected the dependency licenses or sources:\n{}",
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
    fn extern_split_from_abi_by_block_comment_is_rejected() {
        // Review finding #1 (regression): a block comment between `extern` and its
        // ABI string must NOT hide the extern. Compiles as real FFI.
        let src = "extern /* c\nc */ \"C\" { fn getenv(n: *const u8) -> *const u8; }\n";
        assert!(scan_source(Path::new("x.rs"), src).is_err());
    }

    #[test]
    fn extern_split_across_lines_is_rejected() {
        // Review finding #2a: `extern` and `"C"` on separate physical lines.
        let src = "extern\n\"C\" { fn evil(); }\n";
        assert!(scan_source(Path::new("x.rs"), src).is_err());
    }

    #[test]
    fn no_mangle_split_across_lines_is_rejected() {
        // Review finding #2b: `#[ no_mangle ]` broken across physical lines.
        let src = "#[\nno_mangle\n]\npub fn exported() {}\n";
        assert!(scan_source(Path::new("x.rs"), src).is_err());
    }

    #[test]
    fn forbidden_token_inside_string_literal_is_allowed() {
        // Review finding #3: a forbidden word appearing only inside a string is
        // data, not code, and must not trip the gate.
        let src = "pub fn f() -> &'static str { \"extern proc_macro no_mangle\" }\n";
        assert!(scan_source(Path::new("x.rs"), src).is_ok());
    }

    #[test]
    fn no_mangle_as_identifier_substring_is_allowed() {
        // The word-boundary match must still let benign identifiers pass.
        let src = "fn no_mangle_helper() {}\nlet link_count = 1;\n";
        assert!(scan_source(Path::new("x.rs"), src).is_ok());
    }

    #[test]
    fn include_escape_hidden_in_string_is_still_rejected() {
        // The `include!` check reads the string-preserving view, so a `..` escape
        // inside the path literal is still seen even though the keyword scan blanks
        // string interiors.
        let src = "include!(\"../../../etc/passwd\");\n";
        assert!(scan_source(Path::new("x.rs"), src).is_err());
    }

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
    fn block_comment_mentioning_extern_is_allowed() {
        // Issue #16 repro: a block comment naming `extern` must not trip the gate.
        let src = "/* NOTE: we deliberately avoid extern \"C\" in this module */\npub fn unused_demo() {}\n";
        assert!(scan_source(Path::new("x.rs"), src).is_ok());
    }

    #[test]
    fn multiline_block_comment_with_extern_is_allowed() {
        // A block comment spanning lines and mentioning forbidden tokens is fine;
        // line numbers past it must still be reported correctly for real hits.
        let src = "/*\n extern \"C\"\n proc_macro\n*/\nfn ok() {}\n";
        assert!(scan_source(Path::new("x.rs"), src).is_ok());
    }

    #[test]
    fn doc_comment_mentioning_proc_macro_is_allowed() {
        // Outer/inner doc comments (`///`, `//!`) are line comments and stripped.
        let src = "/// Uses no proc_macro machinery whatsoever.\npub fn f() {}\n";
        assert!(scan_source(Path::new("x.rs"), src).is_ok());
    }

    #[test]
    fn identifier_containing_proc_macro_is_allowed() {
        // Issue #16 repro: an identifier containing `proc_macro` is benign.
        let src = "pub fn count_proc_macro_mentions() -> usize { 0 }\n";
        assert!(scan_source(Path::new("x.rs"), src).is_ok());
    }

    #[test]
    fn identifier_containing_extern_is_allowed() {
        // `extern` as a substring of a larger identifier is not FFI.
        let src = "fn my_extern_helper() {}\nlet external = 1;\n";
        assert!(scan_source(Path::new("x.rs"), src).is_ok());
    }

    #[test]
    fn real_proc_macro_use_is_rejected() {
        // A genuine `proc_macro` token (not a substring) must still be caught.
        let src = "use proc_macro;\n";
        assert!(scan_source(Path::new("x.rs"), src).is_err());
    }

    #[test]
    fn extern_without_space_is_rejected() {
        // `extern"C"` (no space before the ABI string) is still FFI.
        let src = "extern\"C\" { fn evil(); }\n";
        assert!(scan_source(Path::new("x.rs"), src).is_err());
    }

    #[test]
    fn code_after_inline_block_comment_is_still_scanned() {
        // Stripping a block comment must not blind the scanner to real code that
        // follows it on the same line.
        let src = "/* ok */ extern \"C\" { fn evil(); }\n";
        assert!(scan_source(Path::new("x.rs"), src).is_err());
    }

    #[test]
    fn block_comment_marker_inside_string_does_not_hide_code() {
        // Untrusted-code bypass guard: a `/*` inside a string literal must NOT be
        // treated as a comment start, or forbidden code after it would be hidden.
        let src = "let s = \"/*\";\nextern \"C\" { fn evil(); }\nlet e = \"*/\";\n";
        assert!(scan_source(Path::new("x.rs"), src).is_err());
    }

    #[test]
    fn comment_marker_inside_raw_string_does_not_hide_code() {
        // Same guard for raw strings, where `\"` does not escape the delimiter.
        let src = "let s = r#\"/*\"#;\nuse proc_macro;\n";
        assert!(scan_source(Path::new("x.rs"), src).is_err());
    }

    #[test]
    fn string_containing_comment_open_is_not_treated_as_comment() {
        // A benign string that merely contains `/*` must not swallow later lines
        // (which would be a false NEGATIVE hiding nothing here, but proves the
        // stripper resumes scanning normal code after the string closes).
        let src = "let s = \"/* not a comment */\";\nfn ok() {}\n";
        assert!(scan_source(Path::new("x.rs"), src).is_ok());
    }

    #[test]
    fn double_quote_char_literal_does_not_desync_string_state() {
        // A char literal holding `"` (valid Rust) must NOT flip the scanner into
        // string mode, or it would swallow the following block comment and then
        // falsely flag the `extern` prose inside it.
        let src = "let q = '\"';\n/* we avoid extern \"C\" here */\nfn ok() {}\n";
        assert!(scan_source(Path::new("x.rs"), src).is_ok());
    }

    #[test]
    fn byte_char_literal_with_quote_does_not_desync() {
        // Same for a byte-char literal `b'"'`.
        let src = "let q = b'\"';\n/* mentions proc_macro in prose */\nfn ok() {}\n";
        assert!(scan_source(Path::new("x.rs"), src).is_ok());
    }

    #[test]
    fn lifetime_tick_is_not_treated_as_char_literal() {
        // A lifetime `'a` is a bare `'` NOT opening a char literal; code after it
        // (here a real extern block) must still be scanned.
        let src = "struct S<'a>(&'a u8);\nextern \"C\" { fn evil(); }\n";
        assert!(scan_source(Path::new("x.rs"), src).is_err());
    }

    #[test]
    fn escaped_quote_char_literal_does_not_desync() {
        // The char literal `'\''` contains an escaped single quote; it must be
        // consumed whole so the following comment is still stripped.
        let src = "let q = '\\'';\n/* extern \"C\" in prose */\nfn ok() {}\n";
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
