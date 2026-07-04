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
//! and the aggregation above lives only here. The per-matrix score is a pure
//! function of (pattern, permutation), so the number printed here is IDENTICAL
//! to the number the grader computes for the same ordering on the same matrices.
//!
//! Any invalid permutation, panic, nondeterminism, cap violation, or
//! purity/license failure makes the whole run FAIL — no partial credit, no
//! silent fallback.

mod corpus;
mod ordering;
mod perm_io;
mod purity;
mod watchdog;

use std::fmt::Write as _;
use std::fs::OpenOptions;
use std::io::Write as _;
use std::path::Path;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use ssi_scoring::score;

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

/// Number of size buckets the score is aggregated over.
const BUCKETS: usize = 3;
/// Stable metric keys for the buckets, in index order (see `size_bucket`).
const BUCKET_KEYS: [&str; BUCKETS] = ["lt_1k", "1k_10k", "gt_10k"];
/// Weights per bucket. Real-world value and algorithmic difficulty concentrate
/// in the large matrices, so `gt_10k` carries the most weight. Empty buckets are
/// renormalized out in `combine`, so these need not be pre-normalized.
const BUCKET_WEIGHTS: [f64; BUCKETS] = [0.30, 0.30, 0.40];

/// Classify a matrix by its dimension `n` into a bucket index (half-open):
/// `n < 1000 → 0` (lt_1k), `1000 ≤ n < 10000 → 1` (1k_10k), `n ≥ 10000 → 2` (gt_10k).
fn size_bucket(n: usize) -> usize {
    if n < 1_000 {
        0
    } else if n < 10_000 {
        1
    } else {
        2
    }
}

/// Per-bucket accumulator: sums of log-ratios (for the geomean) and a count.
#[derive(Default, Clone, Copy)]
struct BucketAcc {
    log_ratio_sum: f64,
    log_fill_sum: f64,
    count: usize,
}

/// Geometric mean from a sum of natural logs and a count. `None` for an empty
/// bucket (no matrices), so `combine` can renormalize it out.
fn geomean(log_sum: f64, count: usize) -> Option<f64> {
    if count == 0 {
        None
    } else {
        Some((log_sum / count as f64).exp())
    }
}

/// Weighted mean of the per-bucket geomeans, renormalizing the weights over the
/// populated (`Some`) buckets. Returns `NaN` if every bucket is empty.
fn combine(geomeans: &[Option<f64>; BUCKETS], weights: &[f64; BUCKETS]) -> f64 {
    let mut num = 0.0_f64;
    let mut den = 0.0_f64;
    for i in 0..BUCKETS {
        if let Some(g) = geomeans[i] {
            num += weights[i] * g;
            den += weights[i];
        }
    }
    if den == 0.0 {
        f64::NAN
    } else {
        num / den
    }
}

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

    let corpus_file = corpus::corpus_path();
    let corpus = corpus::corpus_indexed();
    if corpus.is_empty() {
        println!(
            "RUN FAILED: no patterns found at {}. Run from the repo root.",
            corpus_file.display()
        );
        std::process::exit(1);
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
    let scratch = std::env::temp_dir().join(format!("ssi-harness-{}", std::process::id()));
    std::fs::create_dir_all(&scratch).expect("create scratch dir");
    // The worker must load from the SAME corpus file the parent just indexed —
    // otherwise the parent's raw line index would resolve a different matrix in
    // the worker. corpus_path() honors $SSI_CORPUS_FILE (the grader's eval seam).
    let jsonl_path = corpus_file.to_string_lossy().into_owned();
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

    let _ = std::fs::remove_dir_all(&scratch);

    let timestamp = now();

    match failed {
        Some(reason) => {
            println!("\nRUN FAILED: {reason}");
            append_results(timestamp, "FAIL", f64::NAN, f64::NAN, &note);
            std::process::exit(1);
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

#[cfg(test)]
mod scoring_tests {
    use super::*;

    #[test]
    fn size_bucket_boundaries() {
        assert_eq!(size_bucket(0), 0);
        assert_eq!(size_bucket(999), 0);
        assert_eq!(size_bucket(1000), 1);
        assert_eq!(size_bucket(9999), 1);
        assert_eq!(size_bucket(10000), 2);
        assert_eq!(size_bucket(340_000), 2);
    }

    #[test]
    fn geomean_empty_is_none() {
        assert_eq!(geomean(0.0, 0), None);
    }

    #[test]
    fn geomean_matches_exp_mean() {
        // two ratios 0.5 and 0.8 → geomean = sqrt(0.4) ≈ 0.632455
        let ls = 0.5_f64.ln() + 0.8_f64.ln();
        let g = geomean(ls, 2).unwrap();
        assert!((g - (0.4_f64).sqrt()).abs() < 1e-12, "g = {g}");
    }

    #[test]
    fn combine_all_populated_matches_worked_example() {
        // user's example: 0.8, 0.9, 0.7 with weights 0.3, 0.3, 0.4
        let gms = [Some(0.8), Some(0.9), Some(0.7)];
        let got = combine(&gms, &BUCKET_WEIGHTS);
        let want = 0.30 * 0.8 + 0.30 * 0.9 + 0.40 * 0.7;
        assert!((got - want).abs() < 1e-12, "got = {got}, want = {want}");
    }

    #[test]
    fn combine_one_empty_renormalizes() {
        // lt_1k empty → weighted mean over {1k_10k: 0.9, gt_10k: 0.7} with
        // weights {0.3, 0.4} renormalized by 0.7.
        let gms = [None, Some(0.9), Some(0.7)];
        let got = combine(&gms, &BUCKET_WEIGHTS);
        let want = (0.30 * 0.9 + 0.40 * 0.7) / (0.30 + 0.40);
        assert!((got - want).abs() < 1e-12, "got = {got}, want = {want}");
    }

    #[test]
    fn combine_only_one_populated_is_that_geomean() {
        // dev-corpus case: only lt_1k populated → score == its geomean.
        let gms = [Some(0.873), None, None];
        let got = combine(&gms, &BUCKET_WEIGHTS);
        assert!((got - 0.873).abs() < 1e-12, "got = {got}");
    }

    #[test]
    fn combine_all_empty_is_nan() {
        let gms = [None, None, None];
        assert!(combine(&gms, &BUCKET_WEIGHTS).is_nan());
    }
}
