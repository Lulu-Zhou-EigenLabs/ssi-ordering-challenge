# Best-of-k with exact self-scoring

## What it is
A meta-ordering framework, not a single algorithm. `order()` generates several
**candidate permutations**, computes the *exact* grader flop proxy
`flops = Σⱼ cⱼ²` for each, and returns the cheapest. Because **AMD-default is
always a candidate**, the result is never worse than AMD on any matrix: every
matrix either ties AMD (1.0 in its bucket geomean) or improves it. The overall
score is therefore guaranteed **≤ 1.0**, strictly < 1.0 as soon as any
alternative wins anywhere.

Its durable value: any future heuristic (nested dissection, RCM, an AMD variant)
slots in as **just one more candidate** — its wins are captured automatically,
its losses cost nothing because it is never selected when worse. See
[nested-dissection.md](nested-dissection.md) — this page is the host for that
candidate when it lands.

## Why the self-computed score is exact (no drift)
The benchmark score is a pure function of `(pattern, permutation)`.
`ssi-scoring::score` computes it as:

```
Pattern → feral CscPattern
        → feral::ordering::amd::permute_pattern(pattern, perm)
        → feral::ordering::elimination_tree::EliminationTree::from_pattern
        → feral::symbolic::column_counts_gnp
        → flops = Σ cⱼ²   (nnz_l = Σ cⱼ, the tiebreak)
```

Every symbol is **public in `feral = "0.11.0"`** — a self-contained crate
(`build = false`, no `build.rs`, no `*-sys` dep) already vetted by the workspace
`deny.toml`. We declare the same crates in `deps.toml` and call the identical
functions, so the flops we rank candidates by are **byte-for-byte** what the
grader computes. No separate code path exists that could drift.

## How it works (enough to implement)

### Candidate set (cheap-first)
All near-linear in nnz. AMD-default is produced first and unconditionally; the
rest are additive and behind the gate below, so the worst case is "return AMD
default" = today's 1.0.

1. **AMD default** — `AmdOptions { aggressive: true, dense_alpha: 10.0 }`
   (`feral_amd::amd_order`). The anchor; guarantees no regression.
2. **AMD variants** — `feral_amd::amd_order_opts` with different `AmdOptions`:
   `dense_alpha ∈ {5.0, 20.0}` (earlier/later dense-row deferral),
   `dense_alpha < 0` (suppress deferral except true hubs), `aggressive: false`.
   Genuinely different orderings, free to try; matter most on dense-KKT / hub
   families where the dense-row threshold changes the result. Exact list is a
   tunable — trim by measured wins.
3. **RCM** — reverse Cuthill–McKee, pure-Rust in `rcm.rs`, near-linear. The
   classic banded / grid win; kept only when it beats AMD on that matrix.

### Selection rule (deterministic)
Among scored candidates pick the min by (1) `flops` ascending, (2) `nnz_l`
ascending (mirrors the benchmark tiebreak), (3) fixed candidate index — a total,
deterministic order.

### Module layout (`src/ordering/`)
| file | responsibility | key interface |
|------|----------------|---------------|
| `mod.rs` | orchestrate: gather candidates under the gate, score each, pick min | `pub fn order(&Pattern) -> Vec<usize>` |
| `candidates.rs` | candidate generators + density gate deciding which to emit | `fn candidates(&Pattern, budget) -> Iterator<(Label, Vec<usize>)>` |
| `scoring.rs` | thin wrapper: `(flops, nnz_l)` for `(pattern, perm)` via feral's exact path | `fn score(&Pattern, &[usize]) -> (u64, u64)` |
| `rcm.rs` | pure-Rust reverse Cuthill–McKee | `fn rcm_order(&Pattern) -> Vec<usize>` |

`deps.toml` gains `feral = "0.11.0"`, `feral-amd = "0.2.1"`,
`feral-ordering-core = "0.2.1"` (matched to the workspace lock; re-run
`prepare-build.sh`). Preserve the `SSI_TEST_SLEEP_MS` hook in `mod.rs`.

## Cost profile vs the cap
Each candidate = one near-linear ordering pass + one near-linear scoring pass.
Cap-safety is **purely deterministic** (no wall-clock — see below):
- **AMD default computed first, unconditionally**, retained as the fallback
  answer — if nothing else runs we return it (baseline's own known-safe cost).
- **Deterministic size/density gate.** Optional candidates are admitted by tiers
  keyed only on `(n, nnz)`: small → AMD default + 3 variants + RCM; mid → default
  + 1 variant + RCM; large/dense → default + 1 variant. Thresholds are first
  guesses (`SMALL_N=20k/SMALL_NNZ=500k`, `MID_N=100k/MID_NNZ=2M`) to be
  **measured and tuned from the first full run's per-matrix timing**.

**Why no wall-clock guard (planning decision, 2026-07-08).** The approved design
floated a wall-clock backstop; it was **dropped**. If candidate *inclusion*
depends on elapsed time, the harness's two runs of `order()` can pick different
candidate sets under timing jitter → different permutations → **determinism gate
FAILS and the whole run is rejected**. So inclusion is decided only by
deterministic `(n, nnz)` tiers, and cap-safety comes from (a) AMD default alone
being known cap-safe on the whole corpus and (b) conservative tiers bounding the
near-linear pass count. Determinism must still be tested on a large matrix.

## Where it wins / loses
Measured by **size bucket** (per-family attribution not yet done — the harness
table keys by matrix name, not family; see open-questions):

| bucket | weight | flop geomean vs AMD |
|--------|--------|---------------------|
| lt_1k  | 0.30   | 0.9706 |
| 1k_10k | 0.30   | 0.9865 |
| gt_10k | 0.40   | 0.9671 |
| **weighted** | | **0.9740** |

Improved all three buckets; strongest in the high-weight large bucket and the
small bucket, weakest in `1k_10k`. Most matrices still tie AMD (already strong on
KKT); wins are concentrated on a subset. The remaining headroom is a
structurally different ordering — nested dissection — added as one more candidate.

## Testing
- Each candidate returns a valid bijection on grid/arrow/tridiagonal/disjoint-
  clique/empty/singleton fixtures.
- `scoring::score` reproduces `ssi-scoring`'s closed forms: dense 3×3 → flops 14,
  nnz 6; star-5 hub-first → 55 / 15; tridiagonal → nnz 2n−1.
- **Best-of-k never exceeds AMD-only flops** on every fixture (no-regression).
- **Determinism**: `order()` twice byte-identical, incl. a large matrix.
- Integration: `prepare-build.sh && cargo run --release … --note "best-of-k"` →
  score ≤ 1.0, no FAIL, record per-bucket/family breakdown; `cargo test --release`.

## Status in `src/ordering/`
**Implemented 2026-07-08**, score 1.0000 → **0.9740**. Live in `mod.rs` +
`candidates.rs` + `scoring.rs` + `rcm.rs`; `deps.toml` declares `feral 0.11.0`.
Gate thresholds unchanged after the first run (no cap pressure observed).

## Links
- Compare: [amd.md](amd.md), [nested-dissection.md](nested-dissection.md)
- Experiments: [../experiments/0002-best-of-k.md](../experiments/0002-best-of-k.md)
