# Development corpus

`patterns.jsonl` here is a **small in-repo sample** (13 matrices, ~20 KB), one
CSC sparsity pattern per line. Its purpose is to exercise the harness pipeline
end to end on a tiny, fast, version-controlled set — *not* to be the corpus you
tune against for a competitive score.

The sample is a deterministic draw from the full dev corpus: the three smallest
matrices in each family (NLP / QCP / QP / QCQP) plus one mid-size sparse matrix
(`gilbert`, n=1001) so a non-trivial ordering case is covered.

## Line format

Each line is one symmetric sparsity pattern as compressed-sparse-column (CSC):

```json
{"n": 4, "nnz": 12, "indptr": [...], "indices": [...], "hash": "...", "source": "st_e09"}
```

- `n` — matrix dimension. `indptr` (len n+1) / `indices` (len nnz) — CSC columns.
- The stored pattern is the **full symmetrized** pattern and **includes the
  diagonal**; the harness reader (`ssi_scoring::pattern_from_jsonl_line`) drops
  the diagonal to produce the off-diagonal contract `Pattern`.
- `hash` (SHA-256 of the canonical pattern) and `source` (origin problem) are
  metadata; the harness uses `source` as the display name.

## The full corpus

The full development corpus (~279 patterns, up to n≈340k) is produced by the
`corpus-generation` pipeline and is **not committed here** (it is ~225 MB). It
is published for download separately; fetch it and replace this `patterns.jsonl`
to tune against the full set. The competition's hidden evaluation corpus is
never published.
