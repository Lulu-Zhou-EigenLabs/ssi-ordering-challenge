#!/usr/bin/env bash
# Regenerate Cargo.toml from Cargo.toml.in + validated src/ordering/deps.toml.
# Runs in BOTH the local harness and the grader (Invariant 2). Exit non-zero on
# any validation failure; the full transitive-tree license/FFI scan is added in
# Task 6 (after `cargo vendor`).
set -euo pipefail
ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT"

DEPS_TOML="src/ordering/deps.toml"
TEMPLATE="Cargo.toml.in"
OUT="Cargo.toml"

[ -f "$TEMPLATE" ] || { echo "prepare-build: missing $TEMPLATE" >&2; exit 2; }

# Helper: write $OUT = template up-to-and-including the marker, then the given
# generated dep lines (may be empty), then the remainder of the template.
write_manifest() {
  local gen="$1"
  awk '1; /=== GENERATED DEPS BELOW/ {exit}' "$TEMPLATE" > "$OUT"
  if [ -n "$gen" ]; then
    while IFS='=' read -r name version; do
      [ -z "$name" ] && continue
      printf '%s = "%s"\n' "$name" "$version" >> "$OUT"
    done <<< "$gen"
  fi
  awk 'f; /=== GENERATED DEPS BELOW/ {f=1}' "$TEMPLATE" >> "$OUT"
}

# BOOTSTRAP: `Cargo.toml` is generated (git-ignored), so a fresh checkout has
# none — but the emit-deps step below needs a manifest for `cargo` to resolve
# the workspace. Write a valid deps-free manifest from the template first, so
# `cargo run -p ssi-purity` works even on a clean clone.
write_manifest ""

# Shape-validate deps.toml via ssi-purity (the ONE parser). Emits one
# `name=version` line per validated dep to stdout, or exits non-zero.
GEN="$(cargo run --quiet -p ssi-purity --bin emit-deps -- "$DEPS_TOML")" || {
  echo "prepare-build: deps.toml rejected (see error above)" >&2
  exit 1
}

# Rewrite Cargo.toml with the validated declared deps.
write_manifest "$GEN"
echo "prepare-build: wrote $OUT"

# Freeze exact dependency versions, then vendor the full transitive tree. This
# step needs network (to fetch declared crates); the later build is offline.
CARGO_NET_OFFLINE=false cargo generate-lockfile
mkdir -p .cargo
CARGO_NET_OFFLINE=false cargo vendor vendor > .cargo/vendor-source.toml
cat .cargo/config.base.toml .cargo/vendor-source.toml > .cargo/config.toml

# Scan the vendored tree for native/FFI escapes BEFORE any build.
cargo run --quiet -p ssi-purity --bin scan-tree -- vendor || {
  echo "prepare-build: vendored dependency tree failed the FFI/native scan" >&2
  exit 1
}
echo "prepare-build: vendored tree scanned clean"
