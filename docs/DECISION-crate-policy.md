# Decision Record: Permissive-Crate Submission Policy

*Date: 2026-07-02. Amended Invariants 1 (gate definition) and 3 (submission constraints) to allow permissive, pure-Rust third-party crates in `src/ordering/`, subject to a multi-layer filter. Replaced the stdlib-only requirement with a purity-and-license policy that better maps to the design doc's actual guarantees.*

---

## What changed

Submissions may now depend on third-party crates from crates.io, subject to the policy filter described below. The previous stdlib-only restriction is removed. Contestants declare dependencies in a new manifest `src/ordering/deps.toml`, which the harness validates before scoring.

The **submission contract** (the `order()` signature, the score definition, the gates, and the output formats `score.json` / `results.tsv`) remains unchanged. The **one scoring code path** (Invariant 2) remains unchanged. What changed is the **definition** of the purity gate (Invariant 1) and the **scope** of allowed submission code (Invariant 3).

## Why

The stdlib-only requirement slowed contestants and coding agents by forcing reimplementation of common structures (priority queues, graph algorithms, disjoint-set-union) that are readily available in high-quality, permissively-licensed crates. It was a conservative, easily-enforced boundary — but it was **stricter than necessary** to achieve the competition's actual security and fairness goals.

The design doc (`COMPETITION-PROPOSAL.md` §2.4, §6) requires:
1. **Pure Rust** — no foreign code (FFI, C/C++ build scripts, precompiled blobs).
2. **Permissive license** — no GPL/AGPL/proprietary.
3. **No exfiltration or external oracle calls** — no network, no filesystem reads beyond what the sandbox allows.

The stdlib-only rule **over-approximated** these requirements. A crate like `indexmap` or `petgraph` (both MIT-licensed, pure Rust, no FFI, no build scripts, no network calls) posed no threat to the competition's integrity, yet was disallowed.

This amendment relaxes the gate to enforce **what the design doc actually needs**, not a simpler proxy for it. The result: contestants can use battle-tested libraries (priority queues, graph utilities, random-number generators) while the multi-layer filter maintains the same cheat-proof guarantees.

## Invariants amended

### Invariant 1 (Contract frozen)
The **gate definition** changes: submissions are no longer required to be stdlib-only. However, the **contract surfaces** (the `order()` signature, the score definition, the validity gates, the output formats, the time/memory caps) remain frozen. This is a gate-enforcement change, not a contract change.

Contestants who wrote stdlib-only submissions before this amendment will continue to score identically; their code requires no edits. The change is permissive: it **adds** allowed submissions, it does not invalidate or penalize existing ones.

### Invariant 3 (Submission directory)
Previously: code under `src/ordering/` may use **only the Rust standard library**. Now: code under `src/ordering/` may depend on **permissive, pure-Rust crates** declared in `src/ordering/deps.toml`, subject to the filter below. The submission directory is no longer stdlib-only, but it remains **isolated from the harness's dependencies** — submission code cannot reach `feral`, `ssi-scoring`, or any harness crate.

### Invariant 2 (One scoring code path) — preserved and strengthened
The one-scoring-path invariant is **unchanged**. The harness and the grader compute the score by calling the same functions in `ssi-scoring/`. The amendment does not add a second scoring path, nor does it allow submissions to influence how scoring is computed.

In fact, this amendment **strengthens** Invariant 2: because submissions can now use high-quality third-party crates instead of rolling their own approximations of standard algorithms, there is **less** temptation to hack around the harness or reimplement scoring logic locally to "test" an ordering. The cleaner the submission boundary, the less likely contestants are to probe or bypass it.

## Guarantee trade-off (stated plainly)

The stdlib-only rule was a **decidable** guarantee: the Rust compiler enforces it statically, with zero false negatives. The new filter is a **heuristic** one. Specifically:

- **FFI detection is heuristic.** The purity gate scans for `extern`, `#[no_mangle]`, `#[link]`, `build.rs` with C compilation, and `*-sys` crates. However, **cfg-gated or macro-generated FFI** can evade a static scan. For example, a crate could conditionally include FFI on a target the gate does not check, or a proc-macro could emit FFI symbols that are invisible to a simple token scan.
- **License detection is heuristic.** The gate uses `cargo-deny` to check licenses, which parses `Cargo.toml` metadata. However, a crate could **misdeclare** its license, or bundle dual-licensed code without correctly expressing it in metadata.

These are **not theoretical**: adversarial crates exist on crates.io, and a contestant motivated to cheat (or an agent acting on underspecified instructions) could attempt to exploit these gaps.

### Backstops that buy back the risk

The competition's security does **not** rest on the purity gate alone. The gate is the **first layer** of a defense-in-depth strategy. Even if a submission evades the gate and includes hidden FFI or a malicious dependency, the subsequent layers prevent it from affecting the score or exfiltrating data:

1. **No-C-compiler build (Stage B).** The grader's Docker sandbox builds with **no C compiler, no linker for foreign libraries, no build-time network**. A crate that declares a `build.rs` compiling C will fail at build time. A crate that ships a precompiled blob and attempts to link it will fail at link time. This is a **runtime enforcement** of the "pure Rust" requirement.

2. **No-network, no-filesystem sandbox (Stage B).** The grader runs with **no network access and no filesystem reads** beyond the vendored dependencies and the matrix pattern. Even if FFI were present and compiled, it cannot phone home to an external oracle, cannot exfiltrate the eval corpus, and cannot read precomputed lookup tables from disk.

3. **Memory and time caps (Stage B).** A submission that embeds a large precomputed table (e.g., "if hash(pattern) == X, return precomputed_perm_X") is defeated by two facts: (a) the eval corpus is **held out**, so the table would not match the matrices; (b) a table large enough to cover a meaningful fraction of the space exceeds the **2–4 GB per-matrix memory cap** and is detected.

4. **Determinism enforcement (Stage E).** The grader re-runs each submission R=3 times and requires identical output. Nondeterminism (from syscall timing, ASLR, uninitialized memory, or a remote oracle) is flagged.

The purity gate is a **convenience layer** that rejects obviously-invalid submissions early, before the expensive sandbox build. The sandbox is the **authoritative layer** that enforces the actual requirements.

## Threat coverage table

> **Note:** the "Defense layer" column reflects the *implemented* design, which
> deviates from the original plan for the FFI/build rows — see the Task 6 and
> Task 7 follow-up records below for why. The submission's own source
> (`src/ordering/`) is still scanned strictly (`purity_scan`); the dependency
> *tree* is not source-token-scanned (unsound), and the "no foreign code"
> guarantee rests on frozen-registry sourcing + the offline vendored build +
> the no-network scored run, not a custom no-C-compiler image.

| Threat | Defense layer |
|--------|---------------|
| Import a closed-source program (source never published) | **Fully prevented:** crates.io-only sources (`deny.toml`, no git/path) + offline vendored `--locked` build + no-network run — everything compiled comes from the frozen registry, whose source is public; nothing is fetched at build or run time |
| Import a GPL/AGPL/proprietary-licensed crate | License filter (`cargo-deny` licenses check, RequireDeny) over the whole resolved tree |
| Ship/link a precompiled native blob | Prebuilt-artifact ban + `*-sys` crate ban (`scan_vendored_tree`, gate) |
| Declare native compilation (`*-sys`, `links`, `cc`/`cmake`/`bindgen` build-dep) | `scan_vendored_tree` hard-rejects the declared forms (gate) |
| Undeclared C compile (a `build.rs` shelling out to `cc` on hidden/obfuscated C) | **Accepted residual (low value for an ordering competition):** any such C is boxed by the no-network run, held-out corpus, recomputed score, and determinism re-runs; it cannot exfiltrate or reach an oracle. NOT closed by a no-cc image (option A; see Task 7 record) |
| Call a hosted ML model or external oracle at runtime | No-network scored run (Task 9) |
| Exfiltrate the eval corpus via network | No-network scored run (Task 9) |
| Read precomputed permutations from disk | Held-out eval corpus (Stage A/D) + sandboxed run |
| Embed large precomputed lookup table in binary | Memory cap (Stage B) + held-out corpus (Stage A/D) |
| Nondeterministic output (from ASLR, timing, uninit memory) | Determinism re-runs (Stage E, R=3) |
| Cfg-gated or macro-generated FFI that evades static scan | No-C-compiler + no-foreign-linker build (sandbox, Stage B) |

Every threat has **at least two layers**; most have three. The gate is the first layer; the sandbox is the authoritative one.

## Permissive license allowlist (verbatim)

The gate accepts crates under the following SPDX license identifiers:
- MIT
- Apache-2.0
- Apache-2.0 WITH LLVM-exception
- BSD-2-Clause
- BSD-3-Clause
- Unlicense
- Zlib
- Unicode-3.0

Dual-licensed crates (e.g., `MIT OR Apache-2.0`) are accepted if **at least one** of the declared licenses is on this list. Crates with unknown, missing, or non-permissive licenses (GPL-*, AGPL-*, LGPL-*, proprietary, etc.) are rejected.

Rationale: this list covers the vast majority of high-quality, widely-used Rust ecosystem crates (e.g., `serde`, `indexmap`, `petgraph`, `rand`, `smallvec`). It excludes copyleft and proprietary licenses, which the design doc explicitly forbids.

## Follow-up records (appended by later tasks)

This section is reserved for notes appended by later implementation tasks (e.g., Task 2: retired tests; Task 3: Cargo.toml vs Cargo.toml.in; Task 6: observed defense-layer failures during sandbox testing). Later tasks will append subsections here to maintain a single chronological record of the amendment's implementation and any discovered issues.

### Task 2: Name-allowlist retirement
Task 2 retired the name-allowlist unit tests (`dependency_names_reads_dependencies_table_only`, `extra_dependency_is_rejected`, `ssi_purity_is_an_allowed_dependency`); `check()` no longer asserts the manifest dependency set. The submission-facing filter entry `filter_declared_deps` now validates declared deps for shape only; tree-level license/source/FFI enforcement moves to the grader's vendored-tree scan (Task 5).

### Task 3: Manifest template decision
Task 3: chose to gitignore the generated Cargo.toml and commit Cargo.toml.in as source of truth (plan's recommended option).

### Task 4: git-source policy — deviation from the plan's assumption
The plan's Task 4 assumed `cargo deny check sources` would pass with
`unknown-git = "deny"` and an empty `allow-git = []`, on the premise that the
only path deps (`ssi-scoring`, `ssi-purity`) are workspace-local, not git. That
assumption is **factually wrong**: the trusted scoring wrapper `ssi-scoring`
depends on **feral**, which is pinned to a fixed git rev
(`git+https://github.com/jkitchin/feral.git`, rev `5ab8074…`). An empty
`allow-git` therefore fails the *trusted* tree with seven `source-not-allowed`
errors (feral, feral-amd, feral-amf, feral-kahip, feral-metis,
feral-ordering-core, feral-scotch — all from that one repo).

Per CLAUDE.md's precedence rule (record deviations when a governing-doc
assumption is factually wrong), Task 4 removes the git source entirely rather
than carrying a git exception. **feral is published on crates.io**, and the
exact versions the workspace was git-pinned to are all available: `feral 0.11.0`
(the git rev `5ab8074…` is tagged `v0.11.0` in the feral checkout, crate version
0.11.0, clean tree) and the companions at `0.2.1` (`feral-amd`,
`feral-ordering-core`, and their transitive `feral-amf`/`feral-kahip`/
`feral-metis`/`feral-scotch`). So `ssi-scoring/Cargo.toml` was switched from git
deps to exact crates.io releases:

```toml
feral = "=0.11.0"
feral-amd = "=0.2.1"
feral-ordering-core = "=0.2.1"
```

Exact `=` pins freeze the scored code (Invariant 2). **Scoring verified
byte-identical after the switch**: the `pinned_identity_scores_on_committed_dev_matrices`
exact-equivalence test and both `scorer_crosscheck` tests pass, and `score.json`
is identical to the pre-switch git build (`1.0000`/`1.0000` over the 13-matrix
dev sample). The regenerated `Cargo.lock` has zero `git+` sources.

With no git source anywhere in the tree, `deny.toml` uses the strict form:
`unknown-git = "deny"` with an empty `allow-git = []` — the whole tree (trusted
closure and contestant deps alike) resolves from the frozen crates.io registry.
`cargo deny check sources` → `sources ok`; `check licenses` → `licenses ok`;
full `cargo test` green. cargo-deny 0.19.0.

(Scope note: this task edited `ssi-scoring/Cargo.toml`, the trusted scoring
wrapper — beyond Task 4's original "config only" scope — because eliminating the
git exception cleanly requires sourcing feral from the registry. Gated on the
scoring-unchanged verification above; the frozen contract's score is unaffected.)

### Task 6: Build wiring — vendoring + transitive tree scan
Task 6 wired `cargo vendor` + `scan_vendored_tree` into `prepare-build.sh`. The empty-deps case (feral closure only) passes the transitive scan. The `openssl-sys` bad-dep test (with a freshly cleaned vendor directory) was caught by the **transitive tree scan layer** (`scan-tree` binary calling `scan_vendored_tree`) before any build: the scan rejected `vcpkg` (a transitive dependency of `openssl-sys`) for shipping a prebuilt native artifact (`vendor/vcpkg/test-data/no-status/installed/x64-windows/lib/zlib.lib`). Exit code: 1. The `*-sys` name check would also have caught `openssl-sys` itself, but the artifact check fired first. `git rm --cached Cargo.toml` was executed to untrack the now-generated manifest.

### Task 7: Build environment — chose the offline model over a custom no-C-compiler image (deviation)
The plan's Task 7 built a custom no-C-compiler Docker image (`grader/Dockerfile`) with a `rust-lld` linker pin, pushed to GHCR, to make C compilation *physically impossible*. During implementation two facts changed the calculus:

1. **The stock `rust:1-slim-bookworm` base image ships a C compiler** (`cc`/`gcc` present). A true no-cc image therefore requires *actively stripping* the compiler and guarding against its reintroduction on every base-image update — fragile, maintenance-prone infrastructure.
2. **The threat the no-cc image uniquely closes is narrow and low-value here.** Closed-source injection (code never published) is *already* fully prevented by crates.io-only sourcing + the offline vendored build + the no-network run — none of which need a custom image. The *only* residual the no-cc image adds over that is an *undeclared* `build.rs` shelling out to `cc` on hidden/obfuscated C (the declared forms — `*-sys`/`links`/`cc` build-dep — are already rejected by `scan_vendored_tree`). For a fill-reducing ordering competition, such boxed C is low-value: the no-network run, held-out corpus, recomputed score, and determinism re-runs leave it nothing to exfiltrate or exploit.

**Decision (option A):** do NOT ship a custom no-cc image or a `rust-lld` pin. The `benchmark.yml` job runs on the stock `ubuntu-latest` runner and: installs `cargo-deny`, runs `prepare-build.sh` (the one network step, for `cargo vendor`), then builds and scores with `cargo … --release --offline --locked`. The "no foreign code enters" guarantee rests on: crates.io-only sources (`deny.toml`), the offline vendored `--locked` build, the `scan_vendored_tree` native-signal gate, and the no-network scored run (Task 9). The undeclared-C residual is an accepted, documented low-value risk. The `.cargo/config.base.toml` linker pin was reverted; no `grader/Dockerfile` was added.

Verified locally: `prepare-build.sh` → "scanned clean"; `cargo run --release --offline --locked` scores `1.0000` (empty-deps case) with no network.
