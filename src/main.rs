//! Harness entry point — THE CONTRACT. Do not modify.
//!
//! `cargo run --release -- --note "what I tried"` does:
//!
//!   0. run the local purity & license gate (Stage A analog) over
//!      src/ordering/ — a stdlib-only, license-clean submission passes; any
//!      foreign-code escape or extra dependency FAILs the run before scoring;
//!
//! then, per matrix in the development corpus (the real dev patterns in
//! corpus/dev/patterns.jsonl, loaded with the shared ssi-scoring JSONL reader):
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
mod perm_io;
mod purity;
mod watchdog;

use std::fmt::Write as _;
use std::fs::OpenOptions;
use std::io::Write as _;
use std::path::Path;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use ssi_scoring::{score, Pattern};

/// Per-matrix time cap, ENFORCED: order() runs in a child process that is
/// SIGKILLed at this bound (see watchdog). 2 s is the strict end of the cost
/// doc's 2–5 s band; it is stricter than the grader's current 5 s default, so a
/// submission that passes locally passes the server gate.
const TIME_CAP_PER_MATRIX: Duration = Duration::from_secs(2);

fn main() {
    // Worker mode: the ONLY process that runs contestant order(). Loads one
    // pattern by raw line index, runs order(), writes the permutation, exits.
    let raw_args: Vec<String> = std::env::args().collect();
    if raw_args.get(1).map(String::as_str) == Some("--worker") {
        std::process::exit(worker(&raw_args[2..]));
    }

    let note = parse_note();

    // --- Stage A analog: purity & license gate, before any scoring. ---
    let repo_root = repo_root();
    if let Err(e) = purity::check(&repo_root) {
        println!("RUN FAILED (Stage A — purity/license): {e}");
        let ts = now();
        append_results(ts, "FAIL", f64::NAN, f64::NAN, &note);
        std::process::exit(1);
    }

    let corpus = pattern::dev_corpus_indexed();
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

    // Resolve our own executable + a scratch dir for worker perm files.
    let exe = std::env::current_exe().expect("locate harness executable");
    let scratch = std::env::temp_dir().join(format!("ssi-harness-{}", std::process::id()));
    std::fs::create_dir_all(&scratch).expect("create scratch dir");
    let jsonl_path = pattern::DEV_CORPUS_FILE.to_string();
    let cap = watchdog::CapConfig { time_cap: TIME_CAP_PER_MATRIX, poll: std::time::Duration::from_millis(10) };

    for (line_index, name, pat) in &corpus {
        // --- AMD baseline (trusted, in-process) ---
        let base_perm = ssi_scoring::amd_baseline(pat);
        let base = score(pat, &base_perm);

        // --- contestant ordering: capped subprocess, run twice ---
        let run_once = |tag: &str| -> Result<Vec<usize>, String> {
            let out_perm = scratch.join(format!("{line_index}-{tag}.bin"));
            let _ = std::fs::remove_file(&out_perm);
            let mut cmd = std::process::Command::new(&exe);
            cmd.arg("--worker").arg(&jsonl_path).arg(line_index.to_string()).arg(&out_perm);
            let t0 = Instant::now();
            match watchdog::run_capped(&mut cmd, &cap) {
                watchdog::WorkerOutcome::Ok => perm_io::read_perm(&out_perm)
                    .map_err(|e| format!("worker produced no readable permutation: {e}")),
                watchdog::WorkerOutcome::Timeout => Err(format!(
                    "order() exceeded the {:.1}s per-matrix cap and was killed (took ≥ {:.1}s). \
                     Your ordering must return within {:.0}s on every matrix. This matrix is \
                     n={}, nnz={}, nnz/n≈{} — if it is dense, the cost is in order() itself; \
                     gate expensive paths by BOTH n and nnz.",
                    TIME_CAP_PER_MATRIX.as_secs_f64(),
                    t0.elapsed().as_secs_f64(),
                    TIME_CAP_PER_MATRIX.as_secs_f64(),
                    pat.n, pat.nnz(), pat.nnz() / pat.n.max(1)
                )),
                watchdog::WorkerOutcome::Crashed(why) => Err(format!("order() crashed: {why}")),
            }
        };

        let perm1 = match run_once("a") {
            Ok(p) => p,
            Err(e) => { failed = Some(format!("{name}: {e}")); break; }
        };
        let perm2 = match run_once("b") {
            Ok(p) => p,
            Err(e) => { failed = Some(format!("{name}: {e}")); break; }
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
            "{:<28} {:>8} {:>10} {:>14} {:>14} {:>8.3} {:>9}",
            name,
            pat.n,
            pat.nnz(),
            base.flops,
            yours.flops,
            ratio,
            "(capped)"
        );
        println!("{line}");
        let _ = writeln!(table, "{line}");
    }

    let _ = std::fs::remove_dir_all(&scratch);

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

/// `--worker <jsonl_path> <line_index> <out_perm>`: load one pattern, run the
/// contestant order(), write the permutation. The parent supervises this under
/// the time cap and SIGKILLs it on breach. A panic aborts the process (non-zero
/// exit, no perm file) — the parent treats that as a FAIL.
fn worker(args: &[String]) -> i32 {
    let (Some(jsonl), Some(idx_s), Some(out)) = (args.first(), args.get(1), args.get(2)) else {
        eprintln!("--worker: usage: --worker <jsonl_path> <line_index> <out_perm>");
        return 2;
    };
    let Ok(line_index) = idx_s.parse::<usize>() else {
        eprintln!("--worker: line_index must be a non-negative integer");
        return 2;
    };
    let pat = match ssi_scoring::load_pattern_jsonl_line(Path::new(jsonl), line_index) {
        Ok(p) => p,
        Err(e) => {
            eprintln!("worker: failed to load pattern {line_index} from {jsonl}: {e}");
            return 3;
        }
    };
    let perm = ordering::order(&pat);
    match perm_io::write_perm(Path::new(out), &perm) {
        Ok(()) => 0,
        Err(e) => {
            eprintln!("worker: failed to write perm to {out}: {e}");
            4
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
