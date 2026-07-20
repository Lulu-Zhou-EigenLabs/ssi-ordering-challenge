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
//! then prints a per-matrix table and a per-bucket breakdown, computes
//!
//!     score = weighted mean over size buckets of
//!             geomean_within_bucket( flops(yours) / flops(AMD) )
//!             (lower is better)
//!
//! Matrices are bucketed by dimension n — lt_1k (n<1000), 1k_10k
//! (1000≤n<10000), gt_10k (n≥10000) — with weights 0.30 / 0.30 / 0.40. Empty
//! buckets are renormalized out (weights rescaled over populated buckets), so on
//! a corpus that only populates one bucket the score is just that bucket's
//! geomean. The tiebreak is the same weighted scheme over the fill ratio
//! nnz(L)(yours)/nnz(L)(AMD).
//!
//! ONE SCORING CODE PATH (Invariant 2): the baseline and your ordering are both
//! scored by `ssi_scoring::score`, the same function the private grader calls,
//! and the aggregation lives in ssi_scoring::aggregate, shared with the reference-line tools.
//! The per-matrix score is a pure function of (pattern, permutation), so the number
//! printed here is IDENTICAL to the number the grader computes for the same ordering
//! on the same matrices.
//!
//! Any invalid permutation, panic, nondeterminism, cap violation, or
//! purity/license failure makes the whole run FAIL — no partial credit, no
//! silent fallback. A panic in the trusted in-process baseline/score path
//! (feral internal error or an oversized pattern) is caught and recorded as a
//! FAIL row whose note carries the reason; the scratch dir is disposed on
//! every exit path.

mod corpus;
mod failsafe;
mod ordering;
mod pattern_io;
mod perm_io;
mod purity;
mod watchdog;

use std::fmt::Write as _;
use std::fs::OpenOptions;
use std::io::Write as _;
use std::path::Path;
use std::process::ExitCode;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use ssi_scoring::{
    combine, geomean, score, size_bucket, validate_permutation, BucketAcc, BUCKETS,
    BUCKET_KEYS, BUCKET_WEIGHTS,
};

// Re-export the contract type at the crate root so contestant code under
// src/ordering/ imports it as `crate::Pattern`. The type is defined in
// ssi_scoring (the one scoring path, Invariant 2); this is a pure alias, so a
// local `Pattern` is the identical type the grader scores. Naming the crate's
// defining type at its root is idiomatic Rust and keeps the import path honest
// (no module-name clutter, no second file named pattern.rs).
pub use ssi_scoring::Pattern;

/// Per-matrix time cap, ENFORCED: order() runs in a child process that is
/// SIGKILLed at this bound (see watchdog). 2 s is the strict end of the cost
/// doc's 2–5 s guidance band. The grader runs THIS SAME binary (Yukon
/// dispatches `cargo run --release` in the repo's own Actions), so the cap that
/// gates a submission on the server is exactly this constant — local and graded
/// runs use the identical 2 s cap by construction (Invariant 2).
const TIME_CAP_PER_MATRIX: Duration = Duration::from_secs(2);

fn main() -> ExitCode {
    // Worker mode: the ONLY process that runs contestant order(). Loads one
    // pattern by raw line index, runs order(), writes the permutation, exits.
    let raw_args: Vec<String> = std::env::args().collect();
    if raw_args.get(1).map(String::as_str) == Some("--worker") {
        return ExitCode::from(worker(&raw_args[2..]) as u8);
    }

    let note = parse_note();

    // --- Stage A analog: purity & license gate, before any scoring. ---
    let repo_root = repo_root();
    if let Err(e) = purity::check(&repo_root) {
        let reason = format!("Stage A — purity/license: {e}");
        println!("RUN FAILED ({reason})");
        let ts = now();
        append_results(ts, "FAIL", f64::NAN, f64::NAN, &failsafe::compose_note(&reason, &note));
        return ExitCode::FAILURE;
    }

    let corpus_file = corpus::corpus_path();
    let corpus = corpus::corpus();
    if corpus.is_empty() {
        let reason = format!(
            "no patterns found at {}. Run from the repo root.",
            corpus_file.display()
        );
        println!("RUN FAILED: {reason}");
        append_results(now(), "FAIL", f64::NAN, f64::NAN, &failsafe::compose_note(&reason, &note));
        return ExitCode::FAILURE;
    }

    println!(
        "{:<28} {:>8} {:>10} {:>14} {:>14} {:>8} {:>9}",
        "matrix", "n", "nnz(A)", "flops(base)", "flops(yours)", "ratio", "time"
    );

    let mut buckets = [BucketAcc::default(); BUCKETS];
    let mut failed: Option<String> = None;
    let mut table = String::new();

    // Resolve our own executable + a scratch dir for worker perm files.
    let exe = std::env::current_exe().expect("locate harness executable");
    let scratch = failsafe::ScratchDir(
        std::env::temp_dir().join(format!("ssi-harness-{}", std::process::id())),
    );
    std::fs::create_dir_all(scratch.path()).expect("create scratch dir");
    let cap = watchdog::CapConfig { time_cap: TIME_CAP_PER_MATRIX, poll: std::time::Duration::from_millis(10) };

    for (seq, (name, pat)) in corpus.iter().enumerate() {
        // --- AMD baseline (trusted, in-process) — guarded so a feral panic
        // or an i32-overflow-sized pattern becomes a recorded FAIL, not a
        // process abort that leaks scratch (issue #19). ---
        let base = match failsafe::catch(std::panic::AssertUnwindSafe(|| {
            let base_perm = ssi_scoring::amd_baseline(pat);
            score(pat, &base_perm)
        })) {
            Ok(b) => b,
            Err(msg) => {
                failed = Some(format!("{name}: trusted baseline/score panicked — {msg}"));
                break;
            }
        };

        // Serialize THIS pattern once to the scratch dir; both determinism runs
        // read it. Written by the trusted parent OUTSIDE the timed window, so its
        // cost never counts against the per-matrix cap.
        let pat_file = scratch.path().join(format!("{seq}-pat.bin"));
        let _ = std::fs::remove_file(&pat_file);
        if let Err(e) = pattern_io::write_pattern(&pat_file, pat) {
            failed = Some(format!("{name}: failed to stage pattern for worker: {e}"));
            break;
        }

        // --- contestant ordering: capped subprocess, run twice ---
        let run_once = |tag: &str| -> Result<Vec<usize>, String> {
            let out_perm = scratch.path().join(format!("{seq}-{tag}.bin"));
            let _ = std::fs::remove_file(&out_perm);
            let mut cmd = std::process::Command::new(&exe);
            cmd.arg("--worker").arg(&pat_file).arg(&out_perm);
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

        // --- trusted scoring (Stage D), same path as the grader — guarded ---
        let yours = match failsafe::catch(std::panic::AssertUnwindSafe(|| score(pat, &perm1))) {
            Ok(s) => s,
            Err(msg) => {
                failed = Some(format!("{name}: trusted scoring panicked — {msg}"));
                break;
            }
        };
        let ratio = yours.flops as f64 / base.flops as f64;
        let fill_ratio = yours.nnz_l as f64 / base.nnz_l as f64;
        let b = size_bucket(pat.n);
        buckets[b].log_ratio_sum += ratio.ln();
        buckets[b].log_fill_sum += fill_ratio.ln();
        buckets[b].count += 1;

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

    let timestamp = now();

    match failed {
        Some(reason) => {
            println!("\nRUN FAILED: {reason}");
            append_results(
                timestamp,
                "FAIL",
                f64::NAN,
                f64::NAN,
                &failsafe::compose_note(&reason, &note),
            );
            ExitCode::FAILURE
        }
        None => {
            // Per-bucket geomeans (None for an empty bucket).
            let flop_gms: [Option<f64>; BUCKETS] =
                std::array::from_fn(|i| geomean(buckets[i].log_ratio_sum, buckets[i].count));
            let fill_gms: [Option<f64>; BUCKETS] =
                std::array::from_fn(|i| geomean(buckets[i].log_fill_sum, buckets[i].count));

            let score_val = combine(&flop_gms, &BUCKET_WEIGHTS);
            let fill = combine(&fill_gms, &BUCKET_WEIGHTS);

            // Per-bucket breakdown table.
            println!("\nper-bucket (geomean of ratio vs AMD, within bucket):");
            println!(
                "{:<8} {:>6} {:>16} {:>16}",
                "bucket", "count", "flop_geomean", "fill_geomean"
            );
            for i in 0..BUCKETS {
                let fmt = |g: Option<f64>| g.map_or_else(|| "—".to_string(), |v| format!("{v:.4}"));
                println!(
                    "{:<8} {:>6} {:>16} {:>16}",
                    BUCKET_KEYS[i],
                    buckets[i].count,
                    fmt(flop_gms[i]),
                    fmt(fill_gms[i]),
                );
            }

            println!(
                "\nscore (weighted mean of per-bucket geomean flop ratios, lower is better): {score_val:.4}"
            );
            println!("tiebreak (weighted mean of per-bucket geomean fill ratios):                    {fill:.4}");

            // score.json — top-level `score` is what the grader ranks on;
            // `metrics` is passthrough detail (Yukon captures it whole and shows
            // it in the PR report). All three buckets are always listed; an empty
            // bucket has count 0 and null geomeans.
            let mut buckets_json = String::new();
            for i in 0..BUCKETS {
                let jf = |g: Option<f64>| g.map_or_else(|| "null".to_string(), |v| format!("{v:.6}"));
                let sep = if i + 1 < BUCKETS { "," } else { "" };
                let _ = write!(
                    buckets_json,
                    "\"{}\": {{ \"count\": {}, \"geomean_flop_ratio\": {}, \"geomean_fill_ratio\": {} }}{}",
                    BUCKET_KEYS[i],
                    buckets[i].count,
                    jf(flop_gms[i]),
                    jf(fill_gms[i]),
                    sep,
                );
            }
            let total: usize = buckets.iter().map(|b| b.count).sum();
            let json = format!(
                "{{ \"score\": {score_val:.6}, \"metrics\": {{ \
                 \"geomean_flop_ratio\": {score_val:.6}, \
                 \"geomean_fill_ratio\": {fill:.6}, \
                 \"matrices\": {total}, \
                 \"weights\": {{ \"lt_1k\": {:.2}, \"1k_10k\": {:.2}, \"gt_10k\": {:.2} }}, \
                 \"buckets\": {{ {buckets_json} }} }} }}\n",
                BUCKET_WEIGHTS[0], BUCKET_WEIGHTS[1], BUCKET_WEIGHTS[2],
            );
            std::fs::write("score.json", json).expect("write score.json");
            append_results(timestamp, "OK", score_val, fill, &note);
            ExitCode::SUCCESS
        }
    }
}

/// `--worker <pattern_file> <out_perm>`: read the one pattern the parent
/// serialized for this matrix, run the contestant order(), write the
/// permutation. The parent supervises this under the time cap and SIGKILLs it on
/// breach. A panic aborts the process (non-zero exit, no perm file) — the parent
/// treats that as a FAIL.
fn worker(args: &[String]) -> i32 {
    let (Some(pattern_file), Some(out)) = (args.first(), args.get(1)) else {
        eprintln!("--worker: usage: --worker <pattern_file> <out_perm>");
        return 2;
    };
    let pat = match pattern_io::read_pattern(Path::new(pattern_file)) {
        Ok(p) => p,
        Err(e) => {
            eprintln!("worker: failed to read pattern from {pattern_file}: {e}");
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
