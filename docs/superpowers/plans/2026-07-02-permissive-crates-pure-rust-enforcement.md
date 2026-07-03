# Permissive-Crates + Pure-Rust Enforcement Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Replace the stdlib-only submission rule with "any *permissive, pure-Rust* crate is allowed," enforced by three defense layers — a build-time policy filter over the whole dependency tree, a no-C-compiler build environment, and a no-network runtime sandbox — so contestants gain crate access without opening a path for closed-source or foreign code to enter or be reached.

**Architecture:** A contestant declares crates in a controlled file `src/ordering/deps.toml` (a restricted TOML subset the grader parses itself — never a live `Cargo.toml`). A pre-build codegen step validates each declared crate and its full transitive tree against a policy filter (permissive license via `cargo-deny`, no `*-sys`/`links` native wrappers, no non-registry sources, FFI-token triage), then generates the trusted harness manifest with those deps. The build runs `--offline --locked` against a `cargo vendor` snapshot inside a minimal container that has **no C/C++ compiler** and uses `rust-lld` as the linker, so C source physically cannot compile even if the static scan misses a `build.rs`. The grading *run* is network-isolated so a submission cannot call a hosted closed-source model or exfiltrate the eval corpus. Every run goes through this one unified path (full switch — no separate zero-dep fast path).

**Tech Stack:** Rust (stdlib + the `ssi-purity` crate), `cargo-deny`, `cargo vendor`, `cargo-geiger` (triage), Docker/GHCR, GitHub Actions.

## Global Constraints

- **Amends frozen invariants — must be recorded, not silent (CLAUDE.md precedence rule).** This plan changes CLAUDE.md Invariant 1 (the *gate definition* changes) and Invariant 3 (submission is no longer stdlib-only). Task 0 records the amendment in the governing docs before any code changes. Do not skip it.
- **ONE SCORING / GATING CODE PATH (Invariant 2 — unchanged and load-bearing).** The local harness and the grader must run the *identical* validation, merge, and build logic. All new logic lives in `ssi-purity` (shared crate) or in scripts checked into the public repo that both sides invoke. Never fork the logic.
- **The score definition, `order()` signature, `score.json`/`results.tsv` formats stay frozen (Invariant 1, the *output/metric* half).** Only the purity/dependency gate definition changes.
- **The closed-form scorer tests always pass (Invariant 4).** This plan does not touch `ssi-scoring`'s scorer; its tests must stay green.
- **GREEN AND COMMITTED (Invariant 5).** `cargo test` passes at the end of every task; commit at every task boundary.
- **READ, DON'T GUESS (Invariant 6).** When a `cargo-deny`, `cargo vendor`, or `cargo-geiger` flag is unclear, read its `--help`/docs and record findings.
- **Permissive license allowlist (verbatim, from `deny.toml`):** `MIT`, `Apache-2.0`, `Apache-2.0 WITH LLVM-exception`, `BSD-2-Clause`, `BSD-3-Clause`, `Unlicense`, `Zlib`, `Unicode-3.0`.
- **Time cap unchanged:** 2 s/matrix, enforced by `src/watchdog.rs`.
- **Target triple for the grader build:** `x86_64-unknown-linux-gnu` (the `ubuntu-latest` container target). `rust-lld` is well-supported there.

---

## File map

**Created:**
- `docs/DECISION-crate-policy.md` — the amendment record (Task 0).
- `ssi-purity/src/deps.rs` — `deps.toml` parser + policy-filter types (Tasks 1–2, 5).
- `ssi-purity/tests/deps_toml.rs` — integration tests for the parser/filter (Tasks 1–2, 5).
- `scripts/prepare-build.sh` — validates `deps.toml`, generates `Cargo.toml` from the template, vendors (Tasks 3, 6).
- `Cargo.toml.in` — the trusted manifest template with a generated-deps marker (Task 3).
- `.cargo/config.base.toml` — non-generated cargo config (offline + linker) (Tasks 6, 7).
- `grader/Dockerfile` — minimal Rust image, no gcc/clang, `rust-lld` (Task 7).
- `src/ordering/deps.toml` — shipped **empty** starter (Task 3).
- `ssi-purity/src/bin/emit-deps.rs`, `ssi-purity/src/bin/scan-tree.rs` — CLI shims for the scripts (Tasks 5, 6).
- `.github/scripts/assert-no-network.sh` — post-run assertion the benchmark step had no egress (Task 9, optional belt-and-suspenders).

**Modified:**
- `ssi-purity/src/lib.rs` — replace name-allowlist `dependency_scan` with the policy-filter entry; add the tree scan; retire obsolete tests (Tasks 2, 5).
- `ssi-purity/Cargo.toml` — unchanged deps (stdlib only); bump description (Task 2).
- `deny.toml` — tighten `unknown-git = "deny"`; keep license allowlist (Task 4).
- `src/purity.rs` — switch harness mode `FallbackAllowed` → `RequireDeny` (Task 8).
- `.github/workflows/benchmark.yml` — run inside the no-cc container; add `prepare-build.sh`; network isolation on the Benchmark step (Tasks 7, 9).
- `CLAUDE.md` (public repo), `docs/END-TO-END.md`, `docs/HARNESS-DESIGN.md`, `README.md`, `src/ordering/mod.rs` doc header — reflect the new policy (Tasks 0, 10).

---

## Task 0: Record the invariant amendment (decision record)

**Files:**
- Create: `docs/DECISION-crate-policy.md`
- Modify: `CLAUDE.md` (public repo) — the "Constraints" block

**Interfaces:**
- Produces: the written policy that Tasks 1–10 implement. No code symbols.

This task carries no automated test — its deliverable is prose that unblocks the rest. Fold the doc edits into one commit.

- [ ] **Step 1: Write the decision record**

Create `docs/DECISION-crate-policy.md` with these sections (write full prose, not headers alone):
- **What changed:** submissions may now depend on third-party crates, subject to the policy filter. stdlib-only is no longer required.
- **Why:** stdlib-only slowed contestants/agents; the design doc (`COMPETITION-PROPOSAL.md` §2.4/§6) requires *pure Rust + permissive license + no foreign code*, not stdlib-only. The filter satisfies the design doc's actual requirement.
- **Invariants amended:** Invariant 1 (gate *definition* changes; score/signature/output formats unchanged), Invariant 3 (submission no longer stdlib-only). Invariant 2 (one code path) is *preserved and strengthened*.
- **Guarantee trade-off (state plainly):** stdlib-only was a *decidable* guarantee; the filter is a *heuristic* one for FFI (cfg-gated/macro-generated FFI can evade a static scan). The no-C-compiler build and no-network runtime are the backstops that buy back that risk.
- **Threat coverage table:** import closed crate → license/source filter; link prebuilt blob → FFI scan + `*-sys` ban + overlay; compile C → no-cc build; call hosted model at runtime / exfiltrate eval corpus → no-network sandbox.

- [ ] **Step 2: Amend the public-repo CLAUDE.md constraints block**

In `CLAUDE.md`, replace the line:
```
- A local purity & license gate runs before scoring: src/ordering/ must be
  stdlib-only — no build.rs, FFI/extern, #[no_mangle]/#[link], proc-macros,
  include! outside the dir, or added dependencies.
```
with:
```
- A local purity & license gate runs before scoring. src/ordering/ may depend
  on permissive, PURE-RUST crates declared in src/ordering/deps.toml. Forbidden
  in submission code AND anywhere in the dependency tree: FFI/extern,
  #[no_mangle]/#[link], build.rs that compiles C, *-sys / `links` native
  wrappers, proc-macro machinery in the submission dir, include! outside the
  dir, non-registry sources, and non-permissive licenses. See
  docs/DECISION-crate-policy.md.
```

- [ ] **Step 3: Commit**

```bash
git add docs/DECISION-crate-policy.md CLAUDE.md
git commit -m "docs: record crate-policy amendment (Invariants 1 & 3)"
```

---

## Task 1: `deps.toml` parser — accept the safe subset

**Files:**
- Create: `ssi-purity/src/deps.rs`
- Create: `ssi-purity/tests/deps_toml.rs`
- Modify: `ssi-purity/src/lib.rs` (add `mod deps;` and re-exports near the top, above `pub fn check`)

**Interfaces:**
- Produces:
  - `pub struct DeclaredDep { pub name: String, pub version: String }`
  - `pub fn parse_deps_toml(src: &str) -> Result<Vec<DeclaredDep>, GateError>`
  - Re-exported from `ssi-purity/src/lib.rs`: `pub use deps::{DeclaredDep, parse_deps_toml};`
- Consumes: `GateError` (already defined in `ssi-purity/src/lib.rs:26`).

**Format (the ONLY accepted form):** a single `[dependencies]` table of `name = "x.y.z"` string entries. Anything richer — inline tables (`{ ... }`), `git`/`path`/`registry`/`features`/`default-features` keys, other section headers, arrays — is a hard error. This is what makes the untrusted file safe to read: git/path/features escapes are syntactically impossible to express.

- [ ] **Step 1: Write the failing tests**

Create `ssi-purity/tests/deps_toml.rs`:
```rust
use ssi_purity::{parse_deps_toml, DeclaredDep};

#[test]
fn parses_simple_version_entries() {
    let src = "[dependencies]\nrand = \"0.8.5\"\npetgraph = \"0.6.4\"\n";
    let got = parse_deps_toml(src).expect("valid");
    assert_eq!(
        got,
        vec![
            DeclaredDep { name: "rand".into(), version: "0.8.5".into() },
            DeclaredDep { name: "petgraph".into(), version: "0.6.4".into() },
        ]
    );
}

#[test]
fn empty_file_is_ok_and_empty() {
    assert!(parse_deps_toml("").unwrap().is_empty());
    assert!(parse_deps_toml("[dependencies]\n").unwrap().is_empty());
}

#[test]
fn inline_table_is_rejected() {
    // The form that could carry git/path/features — must be impossible.
    let src = "[dependencies]\nevil = { git = \"https://x/y\" }\n";
    assert!(parse_deps_toml(src).is_err());
}

#[test]
fn unknown_section_is_rejected() {
    let src = "[build-dependencies]\ncc = \"1\"\n";
    assert!(parse_deps_toml(src).is_err());
}

#[test]
fn non_semverish_version_is_rejected() {
    // A version string must look like digits/dots (no "*", no ranges, no git refs).
    let src = "[dependencies]\nrand = \"*\"\n";
    assert!(parse_deps_toml(src).is_err());
}
```

Add to `ssi-purity/src/lib.rs`, above `pub fn check` (this crate currently declares no modules):
```rust
mod deps;
pub use deps::{parse_deps_toml, DeclaredDep};
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test -p ssi-purity --test deps_toml`
Expected: FAIL — `parse_deps_toml`/`DeclaredDep` not found.

- [ ] **Step 3: Implement the parser**

Create `ssi-purity/src/deps.rs`:
```rust
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
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test -p ssi-purity --test deps_toml`
Expected: PASS (5 tests).

- [ ] **Step 5: Commit**

```bash
git add ssi-purity/src/deps.rs ssi-purity/tests/deps_toml.rs ssi-purity/src/lib.rs
git commit -m "feat(purity): parse restricted src/ordering/deps.toml subset"
```

---

## Task 2: Submission-facing filter entry + retire the name-allowlist

**Files:**
- Modify: `ssi-purity/src/lib.rs` — remove `dependency_scan`/`dependency_names` (lines ~172–213); add `filter_declared_deps`; rewire `check()`
- Modify: `ssi-purity/tests/deps_toml.rs` — add filter tests
- Modify: `ssi-purity/Cargo.toml` — description text only

**Interfaces:**
- Consumes: `DeclaredDep`, `parse_deps_toml` (Task 1); `Mode`, `license_check`, `purity_scan`, `collect_rs`, `scan_source` (existing in `lib.rs`).
- Produces:
  - `pub fn filter_declared_deps(ordering_dir: &Path) -> Result<Vec<DeclaredDep>, GateError>` — reads `<ordering_dir>/deps.toml` (absent = empty), parses it, returns declared deps. Tree-level license/source/FFI enforcement is Tasks 4–5.
- Removed: `dependency_scan(cargo_toml)` name-allowlist. `check()` no longer asserts the manifest equals `{ssi-scoring, ssi-purity}` — the generated manifest is trusted output of `prepare-build.sh` (Task 3), and dependency policing moves to the vendored-tree scan (Task 5).

- [ ] **Step 1: Write the failing tests**

Add to `ssi-purity/tests/deps_toml.rs`:
```rust
use std::fs;
use ssi_purity::filter_declared_deps;

#[test]
fn missing_deps_toml_yields_empty() {
    let dir = std::env::temp_dir().join("ssi-purity-test-nodeps");
    let _ = fs::create_dir_all(&dir);
    let _ = fs::remove_file(dir.join("deps.toml"));
    assert!(filter_declared_deps(&dir).unwrap().is_empty());
}

#[test]
fn present_deps_toml_is_parsed() {
    let dir = std::env::temp_dir().join("ssi-purity-test-withdeps");
    let _ = fs::create_dir_all(&dir);
    fs::write(dir.join("deps.toml"), "[dependencies]\nrand = \"0.8.5\"\n").unwrap();
    let got = filter_declared_deps(&dir).unwrap();
    assert_eq!(got.len(), 1);
    assert_eq!(got[0].name, "rand");
}
```

- [ ] **Step 2: Run to verify it fails**

Run: `cargo test -p ssi-purity --test deps_toml`
Expected: FAIL — `filter_declared_deps` not found.

- [ ] **Step 3: Replace `dependency_scan` with `filter_declared_deps`**

In `ssi-purity/src/lib.rs`, delete `dependency_scan` and `dependency_names` (lines ~172–213) and the `dependency_scan(...)` call in `check()` (line 37). Add:
```rust
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
```
Rewrite `check()`:
```rust
pub fn check(repo_root: &Path, mode: Mode) -> Result<(), GateError> {
    let ordering_dir = repo_root.join("src/ordering");
    purity_scan(&ordering_dir)?;
    // Declared deps are validated for shape here; the resolved tree is scanned
    // for license/source/FFI by the grader after vendoring (scan_vendored_tree).
    filter_declared_deps(&ordering_dir)?;
    license_check(repo_root, mode)?;
    Ok(())
}
```

- [ ] **Step 4: Retire the obsolete unit tests**

In `lib.rs`'s `mod tests`, delete the three tests pinning the removed name-allowlist: `dependency_names_reads_dependencies_table_only`, `extra_dependency_is_rejected`, `ssi_purity_is_an_allowed_dependency`. Record "tests retired: name-allowlist" in `docs/DECISION-crate-policy.md`.

- [ ] **Step 5: Update `ssi-purity/Cargo.toml` description**

Change the `description` and the `[dependencies]` comment to say the submission may declare permissive pure-Rust crates in `deps.toml` (not "stdlib only"). Keep the `[dependencies]` table empty — `ssi-purity` itself stays stdlib-only.

- [ ] **Step 6: Run purity tests**

Run: `cargo test -p ssi-purity`
Expected: PASS.

- [ ] **Step 7: Commit**

```bash
git add ssi-purity/src/lib.rs ssi-purity/tests/deps_toml.rs ssi-purity/Cargo.toml
git commit -m "feat(purity): replace name-allowlist with deps.toml filter entry"
```

---

## Task 3: Manifest template + prepare-build codegen + empty starter deps.toml

**Files:**
- Create: `Cargo.toml.in`
- Create: `scripts/prepare-build.sh`
- Create: `src/ordering/deps.toml` (empty starter)
- Modify: `.gitignore`

**Interfaces:**
- Produces: `scripts/prepare-build.sh` — regenerates `Cargo.toml` from `Cargo.toml.in` + validated `src/ordering/deps.toml`. Idempotent. Exit non-zero on any validation failure.
- Consumes: `emit-deps` binary (created in Task 5). For THIS task, verify the awk generation logic with a stubbed value; wire the real binary call in Task 5/6.

**Design note:** the harness is one crate; for `src/ordering/` code to `use somecrate`, that crate must be in the harness `[dependencies]`. Keep the trusted manifest as `Cargo.toml.in` (with a `# === GENERATED DEPS BELOW ===` marker) and generate `Cargo.toml` from it plus validated `deps.toml`. Both local runs and CI call `prepare-build.sh` before `cargo`. This keeps the trusted manifest under our control and the untrusted input (`deps.toml`) parsed by our code.

- [ ] **Step 1: Create the manifest template**

Copy the current `Cargo.toml` to `Cargo.toml.in` and add a marker as the last line of the `[dependencies]` table:
```toml
[dependencies]
ssi-scoring = { path = "ssi-scoring" }
ssi-purity = { path = "ssi-purity" }
# === GENERATED DEPS BELOW (prepare-build.sh; do not edit by hand) ===
```
Leave `[workspace]`, `[dev-dependencies]`, `[profile.release]`, and the `[package]` header identical.

- [ ] **Step 2: Create the empty starter deps.toml**

Create `src/ordering/deps.toml`:
```toml
# Declare permissive, pure-Rust crates your ordering needs, one per line:
#   [dependencies]
#   rand = "0.8.5"
# Only `name = "x.y.z"` is accepted. No git/path/features. Every crate and its
# whole transitive tree must be permissively licensed and contain no C/FFI
# (enforced by the grader; see docs/DECISION-crate-policy.md).
[dependencies]
```

Decision to make and record in `docs/DECISION-crate-policy.md`: **Recommended — commit `Cargo.toml.in` as source of truth and gitignore the generated `Cargo.toml`.** (Alternative: keep `Cargo.toml` committed and let the script overwrite it; simpler for bare `cargo` users but shows a generated file in the tree.) Add the chosen ignored path(s) to `.gitignore`.

- [ ] **Step 3: Write prepare-build.sh**

Create `scripts/prepare-build.sh` and `chmod +x` it:
```bash
#!/usr/bin/env bash
# Regenerate Cargo.toml from Cargo.toml.in + validated src/ordering/deps.toml.
# Runs in BOTH the local harness and the grader (Invariant 2). Exit non-zero on
# any validation failure; the full transitive-tree license/FFI scan is added in
# Task 6 (after `cargo vendor`).
set -euo pipefail
ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT"

DEPS_TOML="src/ordering/deps.toml"
TEMPLATE="Cargo.toml.in"
OUT="Cargo.toml"

[ -f "$TEMPLATE" ] || { echo "prepare-build: missing $TEMPLATE" >&2; exit 2; }

# Shape-validate deps.toml via ssi-purity (the ONE parser). Emits one
# `name=version` line per validated dep to stdout, or exits non-zero.
GEN="$(cargo run --quiet -p ssi-purity --bin emit-deps -- "$DEPS_TOML")" || {
  echo "prepare-build: deps.toml rejected (see error above)" >&2
  exit 1
}

# Rebuild Cargo.toml: template up to and including the marker, then generated
# deps, then the remainder of the template after the marker.
awk '1; /=== GENERATED DEPS BELOW/ {exit}' "$TEMPLATE" > "$OUT"
if [ -n "$GEN" ]; then
  while IFS='=' read -r name version; do
    [ -z "$name" ] && continue
    printf '%s = "%s"\n' "$name" "$version" >> "$OUT"
  done <<< "$GEN"
fi
awk 'f; /=== GENERATED DEPS BELOW/ {f=1}' "$TEMPLATE" >> "$OUT"
echo "prepare-build: wrote $OUT"
```

- [ ] **Step 4: Verify the generation logic with a stub**

Run:
```bash
awk '1; /=== GENERATED DEPS BELOW/ {exit}' Cargo.toml.in > /tmp/out.toml
printf 'rand = "0.8.5"\n' >> /tmp/out.toml
awk 'f; /=== GENERATED DEPS BELOW/ {f=1}' Cargo.toml.in >> /tmp/out.toml
cat /tmp/out.toml
```
Expected: a valid `Cargo.toml` with `rand = "0.8.5"` inside `[dependencies]`, and `[workspace]`/`[dev-dependencies]`/`[profile.release]` intact below the marker.

- [ ] **Step 5: Commit**

```bash
git add Cargo.toml.in scripts/prepare-build.sh src/ordering/deps.toml .gitignore
git commit -m "feat(build): manifest template + prepare-build codegen + empty deps.toml"
```

---

## Task 4: Tighten deny.toml (block git/path sources)

**Files:**
- Modify: `deny.toml` — `[sources]` block

**Interfaces:** none (config only).

- [ ] **Step 1: Edit `deny.toml`**

Change the `[sources]` block from:
```toml
[sources]
unknown-registry = "deny"
unknown-git = "allow"
```
to:
```toml
[sources]
# Every crate must come from the frozen crates.io snapshot. Git sources are the
# escape hatch a policy filter cannot vet — deny them.
unknown-registry = "deny"
unknown-git = "deny"
allow-git = []
```

- [ ] **Step 2: Verify cargo-deny still parses and passes on the trusted tree**

Run (after `bash scripts/prepare-build.sh` generates `Cargo.toml`, or against the committed one):
```bash
cargo deny --manifest-path Cargo.toml check sources
```
Expected: PASS — path deps `ssi-scoring`/`ssi-purity` are workspace-local, not git. If `cargo-deny` is absent: `cargo install cargo-deny`, and record the version in `docs/DECISION-crate-policy.md`.

- [ ] **Step 3: Commit**

```bash
git add deny.toml
git commit -m "feat(deny): forbid git sources (frozen-registry only)"
```

---

## Task 5: `emit-deps` binary + transitive-tree FFI/native scan

**Files:**
- Create: `ssi-purity/src/bin/emit-deps.rs`
- Modify: `ssi-purity/src/lib.rs` — add `pub fn scan_vendored_tree`
- Modify: `ssi-purity/tests/deps_toml.rs` — add tree-scan tests over fixtures

**Interfaces:**
- Produces:
  - Binary `emit-deps`: `emit-deps <deps.toml path>` → prints `name=version` per validated dep to stdout, exit 0; on parse error prints the `GateError` to stderr, exit 1; a missing file is treated as empty (exit 0, no output).
  - `pub fn scan_vendored_tree(vendor_dir: &Path) -> Result<(), GateError>` — walks every crate dir under a `cargo vendor` output dir; rejects: dir whose crate name ends in `-sys`, any `Cargo.toml` with a `links =` key, and any `.rs` containing an FFI token (via the existing `scan_source`). These are HARD rejections — the no-cc build (Task 7) is the backstop for what the scan *misses*, never a license to allow what it *finds*.
- Consumes: `parse_deps_toml` (Task 1); `collect_rs`, `scan_source` (existing private fns — usable within the crate).

- [ ] **Step 1: Write failing tests for the tree scan**

Add to `ssi-purity/tests/deps_toml.rs`:
```rust
use ssi_purity::scan_vendored_tree;

fn vwrite(dir: &std::path::Path, rel: &str, body: &str) {
    let p = dir.join(rel);
    std::fs::create_dir_all(p.parent().unwrap()).unwrap();
    std::fs::write(p, body).unwrap();
}

#[test]
fn clean_vendor_tree_passes() {
    let root = std::env::temp_dir().join("ssi-vendor-clean");
    let _ = std::fs::remove_dir_all(&root);
    vwrite(&root, "rand-0.8.5/Cargo.toml", "[package]\nname=\"rand\"\n");
    vwrite(&root, "rand-0.8.5/src/lib.rs", "pub fn f() -> u32 { 1 }\n");
    assert!(scan_vendored_tree(&root).is_ok());
}

#[test]
fn links_key_is_rejected() {
    let root = std::env::temp_dir().join("ssi-vendor-links");
    let _ = std::fs::remove_dir_all(&root);
    vwrite(&root, "foo-1.0/Cargo.toml", "[package]\nname=\"foo\"\nlinks=\"foo\"\n");
    vwrite(&root, "foo-1.0/src/lib.rs", "pub fn f() {}\n");
    assert!(scan_vendored_tree(&root).is_err());
}

#[test]
fn extern_c_in_dep_is_rejected() {
    let root = std::env::temp_dir().join("ssi-vendor-extern");
    let _ = std::fs::remove_dir_all(&root);
    vwrite(&root, "bar-1.0/Cargo.toml", "[package]\nname=\"bar\"\n");
    vwrite(&root, "bar-1.0/src/lib.rs", "extern \"C\" { fn evil(); }\n");
    assert!(scan_vendored_tree(&root).is_err());
}

#[test]
fn sys_suffix_crate_is_rejected() {
    let root = std::env::temp_dir().join("ssi-vendor-sys");
    let _ = std::fs::remove_dir_all(&root);
    vwrite(&root, "openssl-sys-0.9/Cargo.toml", "[package]\nname=\"openssl-sys\"\n");
    vwrite(&root, "openssl-sys-0.9/src/lib.rs", "pub fn f() {}\n");
    assert!(scan_vendored_tree(&root).is_err());
}
```

- [ ] **Step 2: Run to verify failure**

Run: `cargo test -p ssi-purity --test deps_toml`
Expected: FAIL — `scan_vendored_tree` not found.

- [ ] **Step 3: Implement `scan_vendored_tree`**

In `ssi-purity/src/lib.rs` add (reusing `collect_rs` and `scan_source`):
```rust
/// Scan a `cargo vendor` output directory: reject native-wrapper crates
/// (`*-sys` name or a `links = ` manifest key) and FFI escapes in any `.rs`.
/// Hard rejections — the no-C-compiler build backstops what a static scan
/// misses; it must never be used to justify allowing what the scan finds.
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
        // A vendored dir is `<name>-<version>`; strip the trailing version.
        let name_no_ver = crate_name.rsplit_once('-').map(|(n, _)| n).unwrap_or(crate_name);
        if name_no_ver.ends_with("-sys") {
            return Err(GateError(format!(
                "dependency-scan: `{crate_name}` is a `*-sys` native-library wrapper; \
                 submissions must be pure Rust"
            )));
        }
        let manifest = crate_dir.join("Cargo.toml");
        if let Ok(toml) = std::fs::read_to_string(&manifest) {
            for line in toml.lines() {
                let t = line.trim();
                if t.starts_with("links") && t.contains('=') {
                    return Err(GateError(format!(
                        "dependency-scan: `{crate_name}` declares `links` (native library); \
                         submissions must be pure Rust"
                    )));
                }
            }
        }
        let mut files = Vec::new();
        collect_rs(&crate_dir, &mut files);
        for file in files {
            let src = std::fs::read_to_string(&file)
                .map_err(|e| GateError(format!("dependency-scan: cannot read {}: {e}", file.display())))?;
            scan_source(&file, &src)?; // reuses the FFI-token scan
        }
    }
    Ok(())
}
```

- [ ] **Step 4: Create the `emit-deps` binary**

Create `ssi-purity/src/bin/emit-deps.rs`:
```rust
//! `emit-deps <deps.toml>` — shape-validate the submission's deps.toml and
//! print `name=version` per validated dependency to stdout. Exit 1 on any
//! rejection (message to stderr). A missing file is empty (exit 0). Called by
//! scripts/prepare-build.sh.

use std::path::Path;
use std::process::ExitCode;

fn main() -> ExitCode {
    let Some(path) = std::env::args().nth(1) else {
        eprintln!("usage: emit-deps <deps.toml>");
        return ExitCode::from(2);
    };
    let src = match std::fs::read_to_string(Path::new(&path)) {
        Ok(s) => s,
        Err(ref e) if e.kind() == std::io::ErrorKind::NotFound => String::new(),
        Err(e) => {
            eprintln!("emit-deps: cannot read {path}: {e}");
            return ExitCode::from(1);
        }
    };
    match ssi_purity::parse_deps_toml(&src) {
        Ok(deps) => {
            for d in deps {
                println!("{}={}", d.name, d.version);
            }
            ExitCode::SUCCESS
        }
        Err(e) => {
            eprintln!("emit-deps: {e}");
            ExitCode::from(1)
        }
    }
}
```

- [ ] **Step 5: Run tests + the binary + full prepare-build**

Run: `cargo test -p ssi-purity --test deps_toml`
Expected: PASS (all tree-scan tests).
Run: `printf '[dependencies]\nrand = "0.8.5"\n' > /tmp/d.toml && cargo run -q -p ssi-purity --bin emit-deps -- /tmp/d.toml`
Expected: prints `rand=0.8.5`.
Run: `bash scripts/prepare-build.sh`
Expected: writes `Cargo.toml` (empty deps case) with no error.

- [ ] **Step 6: Commit**

```bash
git add ssi-purity/src/bin/emit-deps.rs ssi-purity/src/lib.rs ssi-purity/tests/deps_toml.rs
git commit -m "feat(purity): emit-deps binary + transitive vendored-tree FFI/native scan"
```

---

## Task 6: Wire vendoring + tree scan into prepare-build.sh

**Files:**
- Create: `ssi-purity/src/bin/scan-tree.rs`
- Create: `.cargo/config.base.toml`
- Modify: `scripts/prepare-build.sh` — add vendor + freeze + `scan-tree`
- Modify: `.gitignore` — ignore generated `.cargo/config.toml`, `.cargo/vendor-source.toml`, `vendor/`

**Interfaces:**
- Produces: after `prepare-build.sh` runs, `Cargo.lock` is frozen, `vendor/` holds the full tree, `.cargo/config.toml` redirects sources to `vendor/`, and the tree has passed `scan_vendored_tree`.
- Consumes: `scan_vendored_tree` (Task 5).

- [ ] **Step 1: Add the scan-tree binary**

Create `ssi-purity/src/bin/scan-tree.rs`:
```rust
//! `scan-tree <vendor_dir>` — run the transitive FFI/native scan over a
//! `cargo vendor` output dir. Exit 1 on the first rejection.
use std::path::Path;
use std::process::ExitCode;

fn main() -> ExitCode {
    let Some(dir) = std::env::args().nth(1) else {
        eprintln!("usage: scan-tree <vendor_dir>");
        return ExitCode::from(2);
    };
    match ssi_purity::scan_vendored_tree(Path::new(&dir)) {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("scan-tree: {e}");
            ExitCode::from(1)
        }
    }
}
```

- [ ] **Step 2: Create the base cargo config**

Create `.cargo/config.base.toml`:
```toml
# Base cargo config; prepare-build.sh appends the `cargo vendor` [source]
# replacement stanza to produce .cargo/config.toml. The rust-lld linker pin is
# added in Task 7. This file is the single source of non-generated cargo config.
[net]
offline = true
```

- [ ] **Step 3: Extend prepare-build.sh**

Append to `scripts/prepare-build.sh` after the manifest generation:
```bash
# Freeze exact dependency versions, then vendor the full transitive tree. This
# step needs network (to fetch declared crates); the later build is offline.
CARGO_NET_OFFLINE=false cargo generate-lockfile
mkdir -p .cargo
CARGO_NET_OFFLINE=false cargo vendor vendor > .cargo/vendor-source.toml
cat .cargo/config.base.toml .cargo/vendor-source.toml > .cargo/config.toml

# Scan the vendored tree for native/FFI escapes BEFORE any build.
cargo run --quiet -p ssi-purity --bin scan-tree -- vendor || {
  echo "prepare-build: vendored dependency tree failed the FFI/native scan" >&2
  exit 1
}
echo "prepare-build: vendored tree scanned clean"
```

- [ ] **Step 4: Update .gitignore**

Add:
```
/Cargo.toml            # generated from Cargo.toml.in (if you chose "gitignore generated")
/.cargo/config.toml
/.cargo/vendor-source.toml
/vendor/
```
(Omit the `/Cargo.toml` line if you chose to commit the generated manifest in Task 3.)

- [ ] **Step 5: Verify end-to-end on the empty deps case**

Run: `bash scripts/prepare-build.sh`
Expected: generates `Cargo.toml` (no extra deps), `generate-lockfile` succeeds, `cargo vendor` writes `vendor/`, `scan-tree` prints "scanned clean". Then:
Run: `cargo build --release --offline --locked`
Expected: builds (path deps resolve from vendor; no network).

- [ ] **Step 6: Verify a bad dep is rejected**

Run:
```bash
printf '[dependencies]\nopenssl-sys = "0.9"\n' > src/ordering/deps.toml
bash scripts/prepare-build.sh; echo "exit=$?"
git checkout src/ordering/deps.toml   # restore empty starter
```
Expected: non-zero exit — `scan-tree` rejects `openssl-sys` as `*-sys` (or `cargo vendor` pulls it and the scan then catches it). Record the observed failing layer in `docs/DECISION-crate-policy.md`.

- [ ] **Step 7: Commit**

```bash
git add scripts/prepare-build.sh ssi-purity/src/bin/scan-tree.rs .cargo/config.base.toml .gitignore
git commit -m "feat(build): vendor + freeze + transitive tree scan in prepare-build"
```

---

## Task 7: No-C-compiler grader container + rust-lld linker

**Files:**
- Create: `grader/Dockerfile`
- Modify: `.cargo/config.base.toml` — add the `rust-lld` linker pin
- Modify: `.github/workflows/benchmark.yml` — run the job inside the container; call `prepare-build.sh`

**Interfaces:** none (infra). Verification-based, not TDD.

- [ ] **Step 1: Add the linker pin to the base cargo config**

Append to `.cargo/config.base.toml`:
```toml
[target.x86_64-unknown-linux-gnu]
# Link with the Rust toolchain's own LLD — no system C compiler needed as the
# linker driver. Combined with a container that has NO gcc/clang, any crate that
# tries to COMPILE C (build.rs + cc crate, or a *-sys crate) fails hard.
linker = "rust-lld"
```

- [ ] **Step 2: Write the Dockerfile**

Create `grader/Dockerfile`:
```dockerfile
# Minimal grader build/run image: Rust toolchain, NO gcc/clang, rust-lld linker.
# Pin the base tag to match the repo's rust-toolchain (currently "stable").
FROM rust:1-slim-bookworm

# Deliberately DO NOT install build-essential / gcc / clang. Install only what a
# pure-Rust build + license scan needs.
RUN rustup component add llvm-tools \
 && cargo install cargo-deny --locked

# Fail image build if a C compiler is present.
RUN if command -v cc >/dev/null 2>&1 || command -v gcc >/dev/null 2>&1 \
      || command -v clang >/dev/null 2>&1; then \
      echo "FATAL: a C compiler is present in the grader image" >&2; exit 1; \
    fi

ENV CARGO_TARGET_X86_64_UNKNOWN_LINUX_GNU_LINKER=rust-lld
WORKDIR /work
```
Build & push to GHCR; record the pinned digest in `docs/DECISION-crate-policy.md`:
```bash
docker build -t ghcr.io/<owner>/ssi-grader-nocc:<tag> grader/
docker push ghcr.io/<owner>/ssi-grader-nocc:<tag>
```

- [ ] **Step 3: Verify pure-Rust builds and C compilation fails in the image**

Run:
```bash
docker run --rm -v "$PWD":/work ghcr.io/<owner>/ssi-grader-nocc:<tag> \
  bash -lc 'bash scripts/prepare-build.sh && cargo build --release --offline --locked'
```
Expected: PASS (pure-Rust harness links via rust-lld).
Negative check (a dep that compiles C must fail to build):
```bash
docker run --rm -v "$PWD":/work ghcr.io/<owner>/ssi-grader-nocc:<tag> \
  bash -lc 'printf "[dependencies]\nflate2 = \"1\"\n" > src/ordering/deps.toml;
            bash scripts/prepare-build.sh && cargo build --release --offline --locked;
            echo "exit=$?"; git checkout src/ordering/deps.toml'
```
Expected: non-zero — either the scan rejects a `*-sys`/`links` dep in the tree, or (for a pure-Rust crate whose `build.rs` compiles C) the build fails for lack of a compiler. Record which layer caught it.

- [ ] **Step 4: Point the workflow at the container + vendor-then-offline**

In `.github/workflows/benchmark.yml`, add to the `benchmark` job (after `runs-on:`):
```yaml
    container:
      image: ghcr.io/<owner>/ssi-grader-nocc:<pinned-digest>
```
Remove the `dtolnay/rust-toolchain@stable` step (the container pins the toolchain). Replace the `Setup` step with:
```yaml
      - name: Prepare build (validate deps, vendor, scan tree)
        run: bash scripts/prepare-build.sh   # network available here for `cargo vendor`
      - name: Setup (offline, locked)
        run: cargo build --release --offline --locked
```
Record in `docs/DECISION-crate-policy.md`: vendoring needs network, but the build (and the scored run, Task 9) are offline/locked/scanned.

- [ ] **Step 5: Commit**

```bash
git add grader/Dockerfile .cargo/config.base.toml .github/workflows/benchmark.yml
git commit -m "feat(grader): no-C-compiler container + rust-lld linker + vendored offline build"
```

---

## Task 8: Flip harness/grader to RequireDeny mode

**Files:**
- Modify: `src/purity.rs`
- Modify: `ssi-purity/src/lib.rs` — add one `mod tests` case

**Interfaces:**
- Consumes: `ssi_purity::Mode::RequireDeny` (already defined, `lib.rs:20`).

**Why:** with crates allowed, the license check is load-bearing (a dep can carry a non-permissive license). `FallbackAllowed` — which *skips* the check when `cargo-deny` is absent — is no longer sound. The grader container installs `cargo-deny` (Task 7), so `RequireDeny` always has it.

- [ ] **Step 1: Write the test**

Add to `ssi-purity`'s `mod tests` in `lib.rs`:
```rust
#[test]
fn require_deny_mode_errors_on_missing_deny_toml() {
    // A missing deny.toml is fatal; assert RequireDeny surfaces it as an error.
    let tmp = std::env::temp_dir().join("ssi-requiredeny");
    let _ = std::fs::create_dir_all(tmp.join("src/ordering"));
    let _ = std::fs::remove_file(tmp.join("deny.toml"));
    assert!(check(&tmp, Mode::RequireDeny).is_err());
}
```

- [ ] **Step 2: Run the test**

Run: `cargo test -p ssi-purity require_deny_mode_errors`
Expected: PASS (missing `deny.toml` already errors in `license_check`). If it fails, read `license_check` and fix the test's assumption — do not weaken the gate.

- [ ] **Step 3: Switch the harness mode**

In `src/purity.rs`, change:
```rust
    ssi_purity::check(repo_root, ssi_purity::Mode::FallbackAllowed)
```
to:
```rust
    // Crates are now allowed, so the license check is load-bearing: a dependency
    // may carry a non-permissive license. RequireDeny makes cargo-deny mandatory
    // (the grader container ships it; local runs need `cargo install cargo-deny`).
    ssi_purity::check(repo_root, ssi_purity::Mode::RequireDeny)
```
Update the file's module doc comment to describe RequireDeny as the active mode.

- [ ] **Step 4: Run the workspace tests**

Run: `cargo test`
Expected: PASS. (Contestants without `cargo-deny` now get a clear "install cargo-deny" error — documented in README, Task 10.)

- [ ] **Step 5: Commit**

```bash
git add src/purity.rs ssi-purity/src/lib.rs
git commit -m "feat(purity): require cargo-deny (RequireDeny) now that crates are allowed"
```

---

## Task 9: Runtime no-network sandbox on the grading run

**Files:**
- Modify: `.github/workflows/benchmark.yml` — isolate the `Benchmark` step's network
- Create: `.github/scripts/assert-no-network.sh` (optional belt-and-suspenders)

**Interfaces:** none (infra). This is the layer that stops a submission calling a hosted closed-source model or exfiltrating the eval corpus at runtime.

**Design note:** the eval corpus is fetched (needs network) *before* the scored run; the scored run itself must have no egress. Keep the network boundary between the "Fetch eval corpus" step (network ON) and the "Benchmark" step (network OFF). On GitHub-hosted runners, use an egress firewall step or run the benchmark in a network-namespaced inner container.

- [ ] **Step 1: Add egress blocking around the Benchmark step**

In `benchmark.yml`, before the `Benchmark` step:
```yaml
      - name: Block egress for the scored run
        run: |
          # Deny outbound except loopback for the scored run. Needs NET_ADMIN;
          # if unavailable on the hosted runner, run the scored step in an inner
          # `docker run --network=none` container instead (see docs note).
          sudo iptables -P OUTPUT DROP || true
          sudo iptables -A OUTPUT -o lo -j ACCEPT || true
      - name: Benchmark
        env:
          SSI_CORPUS_FILE: ${{ steps.corpus.outputs.corpus_file }}
        run: cargo run --release --offline --locked
```
The corpus is already on local disk by this point, so no network is needed to score.

**If hosted-runner permissions block iptables:** run the scored step via `docker run --network=none` using the grader image, mounting the workspace and the downloaded corpus. Record the chosen mechanism in `docs/DECISION-crate-policy.md`.

- [ ] **Step 2: Add a post-run no-egress assertion (optional)**

Create `.github/scripts/assert-no-network.sh` that attempts a connection to a known host and asserts it FAILS during the isolated window; wire it as a best-effort sanity step (does not gate scoring), purely to catch a misconfigured firewall in CI logs.

- [ ] **Step 3: Verify in a CI dry run**

Trigger the workflow (workflow_dispatch) on a branch. Confirm:
- "Fetch eval corpus" succeeds (network ON, or the dev-corpus no-op path),
- "Benchmark" scores with no network,
- a throwaway test submission that opens a socket in `order()` cannot connect (it fails or times out) rather than reaching the internet.
Expected: `score.json` produced; the socket-opening submission cannot exfiltrate.

- [ ] **Step 4: Commit**

```bash
git add .github/workflows/benchmark.yml .github/scripts/assert-no-network.sh
git commit -m "feat(grader): network-isolate the scored run (no runtime egress)"
```

---

## Task 10: Documentation + contestant-facing updates

**Files:**
- Modify: `README.md`, `docs/END-TO-END.md`, `docs/HARNESS-DESIGN.md`, `src/ordering/mod.rs` (doc header)

**Interfaces:** none (docs). One commit.

- [ ] **Step 1: Update the submission doc header**

In `src/ordering/mod.rs`, replace the "stdlib only — no added dependencies" sentence with instructions to declare permissive pure-Rust crates in `src/ordering/deps.toml` (link to `docs/DECISION-crate-policy.md`), and note the FFI / `*-sys` / non-permissive-license / no-C rules that apply tree-wide.

- [ ] **Step 2: Update END-TO-END.md and HARNESS-DESIGN.md**

- `END-TO-END.md` §4 Stage A row and §5: describe the deps.toml → validate → vendor → tree-scan → offline no-cc build → no-network run pipeline.
- `HARNESS-DESIGN.md` Stage A/B and the anti-cheat table row "escape to non-Rust code / deps": replace the stdlib-only description with the three-layer model; keep the "one code path" (Invariant 2) claim, now stronger.

- [ ] **Step 3: Update README.md**

Add a "Using crates" section: the `deps.toml` format, the permissive-license requirement, that `cargo-deny` must be installed locally (RequireDeny), and that `scripts/prepare-build.sh` runs before `cargo run`. Do not change reference score numbers (corpus/scorer unchanged here).

- [ ] **Step 4: Verify docs and tests**

Run: `cargo test`
Expected: PASS (incl. doctests); manually confirm internal doc links resolve.

- [ ] **Step 5: Commit**

```bash
git add README.md docs/END-TO-END.md docs/HARNESS-DESIGN.md src/ordering/mod.rs
git commit -m "docs: contestant + design docs for permissive-crate policy"
```

---

## Self-review notes (for the executor)

- **Local UX change:** contestants must now run `scripts/prepare-build.sh` before `cargo run`. To preserve the literal `cargo run --release -- --note "..."` one-liner, add a thin wrapper (`./x run`) or a cargo alias that runs prepare-build first, and update `CLAUDE.md`'s "The loop" step 3. Decide and record.
- **Vendoring needs network once; the build does not.** The grader fetches declared crates during `prepare-build.sh` (network ON), then builds and scores offline. The scored run (Task 9) is the only strictly no-network phase. Keep these boundaries distinct.
- **The FFI token scan over deps is a hard reject but still heuristic.** The no-C-compiler build (Task 7) is the real backstop for cfg-gated/macro-generated C. Do not present the scan as a proof.
- **Invariant 2 stays intact:** all validation logic lives in `ssi-purity` + `scripts/prepare-build.sh`, both checked into the public repo and run identically locally and on the grader. Never inline a second copy.
- **`rust-lld` linker:** verify a clean pure-Rust link in the container (Task 7 Step 3) before relying on it; it is the one environment-specific risk.
- **Spec coverage check:** build-time filter → Tasks 1–6; no-C build → Task 7; runtime sandbox → Task 9; deps declaration via `deps.toml` → Tasks 1,3; full switch (no fast path) → Task 2 `check()` always runs the filter path; invariant amendment recorded → Task 0.
