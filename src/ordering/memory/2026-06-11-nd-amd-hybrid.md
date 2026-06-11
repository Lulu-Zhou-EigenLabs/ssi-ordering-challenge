# 2026-06-11 — ND+AMD hybrid (0.9496 → 0.8086)

## Architecture
- Arrow detection: hub vertex (deg > n/2, 10x gap to second) → eliminate last
- Grid detection: n>=1000, max_deg<=6, avg_deg>=3 → nested dissection
- Else: AMD with min-fill tie-break

## ND separator strategies (pick best partition_score)
1. Level-set from pseudo-peripheral start BFS
2. Level-set from end BFS  
3. Distance-difference bisection (d_start - d_end, sorted, split at half)
4. BFS from midpoint of diameter (best of 3 so far)

## Key tuning parameters
- ND_THRESHOLD = 200 (leaf AMD size)
- best_level_cut balance penalty = 0.12 (lower = prefer smaller separators)
- partition_score balance penalty = 0.1
- Min balance = 0.10 for both scoring functions
- AMD fill_threshold: 20 for ns<=200, 15 for ns<=500, 10 else

## Per-matrix ratios (vs exact min-degree baseline)
- arrow_2000: 1.000 (perfect match)
- grid2d_30: 0.977 (AMD only, n=900 < threshold)
- grid2d_60: 0.857 (ND)
- grid2d_90: 0.605 (ND)
- grid3d_10: 0.624 (ND)
- grid3d_14: 0.544 (ND)
- kkt_600_200: 0.930 (AMD)
- kkt_2000_700: 0.968 (AMD)
- kkt_4000_1500: 0.956 (AMD)

## What didn't work
- Coordinate-based ND for grids (1.1-1.4x — AMD on subgrid separator 
  doesn't account for cross-boundary fill)
- KKT detection/separation (hard to identify dual block reliably; ND 
  catastrophic on irregular graphs)
- Supervariable absorption (changes elimination order, massive regressions)
- External-degree tie-break (mixed: helps KKTs, hurts grid2d_30, too slow)
- ND threshold 100 (more recursion helps large grids, hurts medium)
- BFS from 1/4 and 3/4 diameter (helps some, hurts grid3d_14)

## Open leads
- grid2d_60 at 0.857 is the biggest ND opportunity — separator quality
  at the 2nd recursion level is poor
- KKTs at 0.93-0.97 — needs fundamentally different tie-break strategy
  since min-fill fires only at low degrees
- Multi-level coarsening (METIS-style) would improve separator quality
  but complex to implement within 10s cap
