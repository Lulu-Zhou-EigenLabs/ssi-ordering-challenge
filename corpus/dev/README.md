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
is published as a **GitHub release asset** (a release keeps a file this large off
the git tree, so clones stay small). Download the latest and verify it:

```sh
BASE=https://github.com/Lulu-Zhou-EigenLabs/ssi-ordering-challenge/releases/latest/download
curl -L -o patterns.jsonl        "$BASE/patterns.jsonl"
curl -L -o patterns.jsonl.sha256 "$BASE/patterns.jsonl.sha256"
shasum -a 256 -c patterns.jsonl.sha256   # Linux: sha256sum -c patterns.jsonl.sha256
```

`/releases/latest/download/` always resolves to the newest release (the corpus
rotates per round); pin a round with `.../releases/download/<tag>/patterns.jsonl`.
Then tune against the full set either by replacing this `patterns.jsonl`, or by
leaving it in place and pointing the harness at the download for one run:

```sh
SSI_CORPUS_FILE=$PWD/patterns.jsonl cargo run --release
```

`SSI_CORPUS_FILE` overrides the corpus path; unset, the harness grades this
in-repo sample. The competition's hidden evaluation corpus is never published.
