//! Harness entry point — THE CONTRACT. Do not modify.
//!
//! `cargo run --release -- --note "what I tried"` does, per matrix in the
//! deterministic development corpus:
//!
//!   1. run the frozen minimum-degree baseline and score it symbolically;
//!   2. run YOUR ordering (src/ordering/) twice — both runs must agree
//!      (determinism gate) and finish under the time cap;
//!   3. validate the permutation as a bijection of 0..n (Stage C);
//!   4. recompute predicted flops and nnz(L) from the permutation with the
//!      trusted symbolic analysis (Stage D) — your code never reports a
//!      number;
//!
//! then prints a per-matrix table, computes
//!
//!     score = geometric mean over the corpus of
//!             flops(yours) / flops(baseline)        (lower is better)
//!
//! and writes `score.json` plus one row of `results.tsv`.
//! Any invalid permutation, panic, nondeterminism, or cap violation makes
//! the whole run FAIL — there is no partial credit and no silent fallback.

mod baseline;
mod ordering;
mod pattern;
mod symbolic;

use std::fmt::Write as _;
use std::fs::OpenOptions;
use std::io::Write as _;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

const TIME_CAP_PER_MATRIX: Duration = Duration::from_secs(10);

fn main() {
    let note = parse_note();
    let corpus = pattern::dev_corpus();

    println!(
        "{:<16} {:>7} {:>9} {:>13} {:>13} {:>8} {:>9}",
        "matrix", "n", "nnz(A)", "flops(base)", "flops(yours)", "ratio", "time"
    );

    let mut log_ratio_sum = 0.0_f64;
    let mut log_fill_sum = 0.0_f64;
    let mut failed: Option<String> = None;
    let mut table = String::new();

    for (name, pat) in &corpus {
        // --- frozen baseline ---
        let base_perm = baseline::min_degree(pat);
        let base = symbolic::analyze(pat, &base_perm);

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
        if let Err(e) = baseline::validate_permutation(&perm1, pat.n) {
            failed = Some(format!("{name}: invalid permutation — {e}"));
            break;
        }

        // --- trusted scoring ---
        let yours = symbolic::analyze(pat, &perm1);
        let ratio = yours.flops as f64 / base.flops as f64;
        let fill_ratio = yours.nnz_l as f64 / base.nnz_l as f64;
        log_ratio_sum += ratio.ln();
        log_fill_sum += fill_ratio.ln();

        let line = format!(
            "{:<16} {:>7} {:>9} {:>13} {:>13} {:>8.3} {:>8.0}ms",
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

    let timestamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs();

    match failed {
        Some(reason) => {
            println!("\nRUN FAILED: {reason}");
            append_results(timestamp, "FAIL", f64::NAN, f64::NAN, &note);
            std::process::exit(1);
        }
        None => {
            let m = corpus.len() as f64;
            let score = (log_ratio_sum / m).exp();
            let fill = (log_fill_sum / m).exp();
            println!("\nscore (geomean flop ratio vs baseline, lower is better): {score:.4}");
            println!("tiebreak (geomean fill ratio):                            {fill:.4}");
            let json = format!(
                "{{ \"score\": {score:.6}, \"metrics\": {{ \"geomean_flop_ratio\": {score:.6}, \"geomean_fill_ratio\": {fill:.6}, \"matrices\": {} }} }}\n",
                corpus.len()
            );
            std::fs::write("score.json", json).expect("write score.json");
            append_results(timestamp, "OK", score, fill, &note);
        }
    }
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
