//! Harness entry point — THE CONTRACT. Do not modify.
//!
//! `cargo run --release -- --note "what I tried"` does:
//!
//!   0. run the local purity & license gate (Stage A analog) over
//!      src/ordering/ — a stdlib-only, license-clean submission passes; any
//!      foreign-code escape or extra dependency FAILs the run before scoring;
//!
//! then, per matrix in the development corpus (the real dev matrices under
//! corpus/dev/, loaded with feral's reference reader):
//!
//!   1. run the AMD baseline (feral_amd::amd_order) and score it through the
//!      trusted scoring wrapper;
//!   2. run YOUR ordering (src/ordering/) twice — both runs must agree
//!      (determinism gate, Stage E analog) and finish under the time cap;
//!   3. validate the permutation as a bijection of 0..n (Stage C analog);
//!   4. recompute predicted flops and nnz(L) from the permutation with the
//!      trusted scoring wrapper (Stage D) — your code never reports a number;
//!
//! then prints a per-matrix table, computes
//!
//!     score = geometric mean over the corpus of
//!             flops(yours) / flops(AMD)             (lower is better)
//!
//! and writes `score.json` plus one row of `results.tsv`. The tiebreak is the
//! geomean of the fill ratio nnz(L)(yours)/nnz(L)(AMD).
//!
//! ONE SCORING CODE PATH (Invariant 2): both the baseline and your ordering are
//! scored by `ssi_scoring::score`, the same function the private grader calls.
//! The score is a pure function of (pattern, permutation), so the number printed
//! here is IDENTICAL to the number the grader computes for the same ordering on
//! the same matrices (exact-grader equivalence, proposal §6/§7).
//!
//! Any invalid permutation, panic, nondeterminism, cap violation, or
//! purity/license failure makes the whole run FAIL — no partial credit, no
//! silent fallback.

mod ordering;
mod pattern;
mod purity;

use std::fmt::Write as _;
use std::fs::OpenOptions;
use std::io::Write as _;
use std::path::Path;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use ssi_scoring::{score, Pattern};

/// Per-matrix time cap. The dev corpus reaches n ≈ 160k; AMD + symbolic scoring
/// of the largest matrices is well under a second (Phase 2 §4, cost doc §1),
/// so a cap of 5 s leaves ample room for annealing/learned orderings while
/// killing runaways. (COMPETITION-VERIFIER-COST §1 recommends 2–5 s.)
const TIME_CAP_PER_MATRIX: Duration = Duration::from_secs(5);

fn main() {
    let note = parse_note();

    // --- Stage A analog: purity & license gate, before any scoring. ---
    let repo_root = repo_root();
    if let Err(e) = purity::check(&repo_root) {
        println!("RUN FAILED (Stage A — purity/license): {e}");
        let ts = now();
        append_results(ts, "FAIL", f64::NAN, f64::NAN, &note);
        std::process::exit(1);
    }

    let corpus = pattern::dev_corpus();
    if corpus.is_empty() {
        println!(
            "RUN FAILED: no patterns found at {}. Run from the repo root.",
            pattern::DEV_CORPUS_FILE
        );
        std::process::exit(1);
    }

    println!(
        "{:<28} {:>8} {:>10} {:>14} {:>14} {:>8} {:>9}",
        "matrix", "n", "nnz(A)", "flops(base)", "flops(yours)", "ratio", "time"
    );

    let mut log_ratio_sum = 0.0_f64;
    let mut log_fill_sum = 0.0_f64;
    let mut failed: Option<String> = None;
    let mut table = String::new();

    for (name, pat) in &corpus {
        // --- AMD baseline ---
        let base_perm = ssi_scoring::amd_baseline(pat);
        let base = score(pat, &base_perm);

        // --- contestant ordering: timed, run twice, validated ---
        let t0 = Instant::now();
        let perm1 = match std::panic::catch_unwind(|| ordering::order(pat)) {
            Ok(p) => p,
            Err(_) => {
                failed = Some(format!("{name}: ordering panicked"));
                break;
            }
        };
        let elapsed = t0.elapsed();
        if elapsed > TIME_CAP_PER_MATRIX {
            failed = Some(format!(
                "{name}: ordering took {:.2}s, cap is {:.0}s",
                elapsed.as_secs_f64(),
                TIME_CAP_PER_MATRIX.as_secs_f64()
            ));
            break;
        }
        let perm2 = match std::panic::catch_unwind(|| ordering::order(pat)) {
            Ok(p) => p,
            Err(_) => {
                failed = Some(format!("{name}: ordering panicked on re-run"));
                break;
            }
        };
        if perm1 != perm2 {
            failed = Some(format!("{name}: nondeterministic ordering (two runs differ)"));
            break;
        }
        if let Err(e) = validate_permutation(&perm1, pat.n) {
            failed = Some(format!("{name}: invalid permutation — {e}"));
            break;
        }

        // --- trusted scoring (Stage D), same path as the grader ---
        let yours = score(pat, &perm1);
        let ratio = yours.flops as f64 / base.flops as f64;
        let fill_ratio = yours.nnz_l as f64 / base.nnz_l as f64;
        log_ratio_sum += ratio.ln();
        log_fill_sum += fill_ratio.ln();

        let line = format!(
            "{:<28} {:>8} {:>10} {:>14} {:>14} {:>8.3} {:>7.0}ms",
            name,
            pat.n,
            pat.nnz(),
            base.flops,
            yours.flops,
            ratio,
            elapsed.as_secs_f64() * 1e3
        );
        println!("{line}");
        let _ = writeln!(table, "{line}");
    }

    let timestamp = now();

    match failed {
        Some(reason) => {
            println!("\nRUN FAILED: {reason}");
            append_results(timestamp, "FAIL", f64::NAN, f64::NAN, &note);
            std::process::exit(1);
        }
        None => {
            let m = corpus.len() as f64;
            let score_val = (log_ratio_sum / m).exp();
            let fill = (log_fill_sum / m).exp();
            println!("\nscore (geomean flop ratio vs AMD baseline, lower is better): {score_val:.4}");
            println!("tiebreak (geomean fill ratio):                                {fill:.4}");
            let json = format!(
                "{{ \"score\": {score_val:.6}, \"metrics\": {{ \"geomean_flop_ratio\": {score_val:.6}, \"geomean_fill_ratio\": {fill:.6}, \"matrices\": {} }} }}\n",
                corpus.len()
            );
            std::fs::write("score.json", json).expect("write score.json");
            append_results(timestamp, "OK", score_val, fill, &note);
        }
    }
}

/// Stage C analog: the returned permutation must be a true bijection of 0..n.
fn validate_permutation(perm: &[usize], n: usize) -> Result<(), String> {
    if perm.len() != n {
        return Err(format!("permutation has length {}, expected {}", perm.len(), n));
    }
    let mut seen = vec![false; n];
    for &v in perm {
        if v >= n {
            return Err(format!("index {} out of range 0..{}", v, n));
        }
        if seen[v] {
            return Err(format!("index {} appears more than once", v));
        }
        seen[v] = true;
    }
    Ok(())
}

/// Resolve the repo root so the gate finds src/ordering/ and deny.toml whether
/// the binary is launched from the repo root or elsewhere. Uses CARGO_MANIFEST_DIR
/// at compile time (the harness package dir), falling back to ".".
fn repo_root() -> std::path::PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).to_path_buf()
}

fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs()
}

fn append_results(ts: u64, status: &str, score: f64, fill: f64, note: &str) {
    let mut f = OpenOptions::new()
        .create(true)
        .append(true)
        .open("results.tsv")
        .expect("open results.tsv");
    writeln!(f, "{ts}\t{status}\t{score:.6}\t{fill:.6}\t{note}").expect("append results.tsv");
}

fn parse_note() -> String {
    let args: Vec<String> = std::env::args().collect();
    for i in 0..args.len() {
        if args[i] == "--note" && i + 1 < args.len() {
            return args[i + 1].replace(['\t', '\n'], " ");
        }
    }
    String::new()
}

// `_` reference so `Pattern` import is used even if the type is only named in
// closures above; keeps the contract type visible at the harness boundary.
const _: fn(&Pattern) -> Vec<usize> = ordering::order;
