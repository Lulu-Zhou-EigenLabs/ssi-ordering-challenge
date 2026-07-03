//! Parser for `src/ordering/deps.toml` — the controlled, minimal file by which
//! a submission declares third-party crates. It is NOT a Cargo manifest: only
//! `name = "x.y.z"` string entries under a single `[dependencies]` table are
//! accepted. Inline tables, git/path/registry/features keys, and any other
//! section are hard errors, so the escapes those would enable are syntactically
//! impossible. This runs in BOTH the local harness and the grader (Invariant 2).

use crate::GateError;

/// One declared dependency: an exact crate name and a plain version string.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DeclaredDep {
    pub name: String,
    pub version: String,
}

/// Parse the restricted `deps.toml` subset. Returns the declared deps in file
/// order, or a `GateError` naming the first offending line.
pub fn parse_deps_toml(src: &str) -> Result<Vec<DeclaredDep>, GateError> {
    let mut deps = Vec::new();
    let mut in_deps = false;
    for (lineno, raw) in src.lines().enumerate() {
        let line = strip_comment(raw).trim();
        if line.is_empty() {
            continue;
        }
        if line.starts_with('[') {
            if line == "[dependencies]" {
                in_deps = true;
                continue;
            }
            return Err(GateError(format!(
                "deps.toml:{}: only a [dependencies] table is allowed, found `{line}`",
                lineno + 1
            )));
        }
        if !in_deps {
            return Err(GateError(format!(
                "deps.toml:{}: entry `{line}` appears before [dependencies]",
                lineno + 1
            )));
        }
        let (name, rest) = line.split_once('=').ok_or_else(|| {
            GateError(format!("deps.toml:{}: expected `name = \"version\"`", lineno + 1))
        })?;
        let name = name.trim();
        let value = rest.trim();
        if value.starts_with('{') || value.starts_with('[') {
            return Err(GateError(format!(
                "deps.toml:{}: `{name}` uses a table/array form; only `name = \"version\"` is allowed \
                 (no git/path/features escapes)",
                lineno + 1
            )));
        }
        let version = value
            .strip_prefix('"')
            .and_then(|v| v.strip_suffix('"'))
            .ok_or_else(|| {
                GateError(format!("deps.toml:{}: version for `{name}` must be a quoted string", lineno + 1))
            })?;
        if !is_plain_version(version) {
            return Err(GateError(format!(
                "deps.toml:{}: version `{version}` for `{name}` must be a plain semver like \"1.2.3\" \
                 (no ranges, `*`, or git refs)",
                lineno + 1
            )));
        }
        if name.is_empty() {
            return Err(GateError(format!("deps.toml:{}: empty crate name", lineno + 1)));
        }
        deps.push(DeclaredDep { name: name.to_string(), version: version.to_string() });
    }
    Ok(deps)
}

/// A plain version is one or more dot-separated numeric components, optionally
/// with a pre-release/build suffix of `[A-Za-z0-9.-+]`. Rejects `*`, `^`, `~`,
/// `>=`, whitespace ranges, and empty strings.
fn is_plain_version(v: &str) -> bool {
    if v.is_empty() {
        return false;
    }
    for c in v.chars() {
        match c {
            '0'..='9' | '.' | '-' | '+' | 'A'..='Z' | 'a'..='z' => {}
            _ => return false, // '*', '^', '~', '>', '<', ' ', ',' etc.
        }
    }
    v.chars().next().map(|c| c.is_ascii_digit()).unwrap_or(false)
}

fn strip_comment(line: &str) -> &str {
    match line.find('#') {
        Some(i) => &line[..i],
        None => line,
    }
}
