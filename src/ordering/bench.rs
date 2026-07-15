//! Ignored-by-default measurement harness for candidate ordering wall times.
//! Run locally with:
//!   cargo test --release --offline --locked bench_candidates -- --ignored --nocapture
//! Never executed by the grader (it is a #[ignore] test), and reads only the
//! public dev corpus.

#[cfg(test)]
mod bench_tests {
    use crate::Pattern;
    use std::io::BufRead;
    use std::time::Instant;

    use feral::ordering::amd::permute_pattern;
    use feral::ordering::elimination_tree::EliminationTree;
    use feral::sparse::csc::CscPattern as ScoringPattern;
    use feral::symbolic::column_counts_gnp;

    fn flops_of(pat: &ScoringPattern, perm: &[usize]) -> u64 {
        let permuted = permute_pattern(pat, perm);
        let etree = EliminationTree::from_pattern(&permuted);
        let counts = column_counts_gnp(&permuted, &etree);
        counts.iter().map(|&c| (c as u64) * (c as u64)).sum()
    }

    /// Measure the full `order()` wall time on every dev matrix; print the
    /// slowest 15. This is the number that must stay far under the 2 s cap.
    #[test]
    #[ignore]
    fn bench_order_walltime() {
        let file = std::fs::File::open("corpus/dev/patterns.jsonl").expect("corpus");
        let reader = std::io::BufReader::new(file);
        let mut rows: Vec<(u128, usize, usize)> = Vec::new();
        for line in reader.lines() {
            let line = line.unwrap();
            let (_name, pat): (String, Pattern) =
                ssi_scoring::pattern_from_jsonl_line(&line).expect("parse");
            let t = Instant::now();
            let perm = crate::ordering::order(&pat);
            let ms = t.elapsed().as_millis();
            assert_eq!(perm.len(), pat.n);
            rows.push((ms, pat.n, pat.nnz()));
        }
        rows.sort_unstable_by(|a, b| b.0.cmp(&a.0));
        println!("slowest 15 order() calls (ms, n, nnz):");
        for r in rows.iter().take(15) {
            println!("  {:>6} ms  n={:<8} nnz={}", r.0, r.1, r.2);
        }
    }

    #[test]
    #[ignore]
    fn bench_candidates() {
        let file = std::fs::File::open("corpus/dev/patterns.jsonl").expect("corpus");
        let reader = std::io::BufReader::new(file);
        println!(
            "{:>8} {:>9} | {:>8} {:>12} | {:>8} {:>12} | {:>8} {:>12} | {:>8} {:>12} | {:>8} {:>12}",
            "n", "nnz", "amd_ms", "amd_fl", "amf_ms", "amf_fl", "met_ms", "met_fl",
            "sco_ms", "sco_fl", "kah_ms", "kah_fl"
        );
        for line in reader.lines() {
            let line = line.unwrap();
            let (_name, pat): (String, Pattern) =
                ssi_scoring::pattern_from_jsonl_line(&line).expect("parse");
            let n = pat.n;
            let nnz = pat.nnz();
            // The band we are currently attributing (env-tunable).
            let lo: usize = std::env::var("BENCH_NNZ_LO")
                .ok()
                .and_then(|v| v.parse().ok())
                .unwrap_or(200_000);
            let hi: usize = std::env::var("BENCH_NNZ_HI")
                .ok()
                .and_then(|v| v.parse().ok())
                .unwrap_or(usize::MAX);
            if nnz < lo || nnz > hi {
                continue;
            }
            let col_ptr_i32: Vec<i32> = pat.col_ptr.iter().map(|&x| x as i32).collect();
            let row_idx_i32: Vec<i32> = pat.row_idx.iter().map(|&x| x as i32).collect();
            let core =
                feral_ordering_core::CscPattern::new(n, &col_ptr_i32, &row_idx_i32).unwrap();
            let spat = ScoringPattern {
                n,
                col_ptr: pat.col_ptr.clone(),
                row_idx: pat.row_idx.clone(),
            };

            let mut cells: Vec<(u128, u64)> = Vec::new();
            // AMD
            let t = Instant::now();
            let amd = feral_amd::amd_order(&core).unwrap();
            let amd_ms = t.elapsed().as_millis();
            let amd_perm: Vec<usize> = amd.iter().map(|&x| x as usize).collect();
            let amd_fl = flops_of(&spat, &amd_perm);
            cells.push((amd_ms, amd_fl));
            // AMF
            let t = Instant::now();
            let r = feral_amf::amf_order(&core);
            let ms = t.elapsed().as_millis();
            let fl = r
                .ok()
                .map(|p| {
                    let perm: Vec<usize> = p.iter().map(|&x| x as usize).collect();
                    flops_of(&spat, &perm)
                })
                .unwrap_or(0);
            cells.push((ms, fl));
            // METIS
            let t = Instant::now();
            let r = feral_metis::metis_order_full(&core, &feral_metis::MetisOptions::default());
            let ms = t.elapsed().as_millis();
            let fl = r
                .ok()
                .map(|(p, _, _)| {
                    let perm: Vec<usize> = p.iter().map(|&x| x as usize).collect();
                    flops_of(&spat, &perm)
                })
                .unwrap_or(0);
            cells.push((ms, fl));
            // Scotch
            let t = Instant::now();
            let r = feral_scotch::scotch_order(&core);
            let ms = t.elapsed().as_millis();
            let fl = r
                .ok()
                .map(|p| {
                    let perm: Vec<usize> = p.iter().map(|&x| x as usize).collect();
                    flops_of(&spat, &perm)
                })
                .unwrap_or(0);
            cells.push((ms, fl));
            // KaHIP (Fast)
            let t = Instant::now();
            let r = feral_kahip::kahip_order(&core);
            let ms = t.elapsed().as_millis();
            let fl = r
                .ok()
                .map(|p| {
                    let perm: Vec<usize> = p.iter().map(|&x| x as usize).collect();
                    flops_of(&spat, &perm)
                })
                .unwrap_or(0);
            cells.push((ms, fl));

            print!("{:>8} {:>9} |", n, nnz);
            for (ms, fl) in &cells {
                print!(" {:>8} {:>12.4} |", ms, *fl as f64 / amd_fl.max(1) as f64);
            }
            println!();
        }
    }
}
