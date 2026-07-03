#!/usr/bin/env bash
# Best-effort sanity check that outbound network is blocked in the current
# (network-namespaced) context of the scored run. LOGGED, NOT GATING: it does
# not affect the score — it exists purely to make a misconfigured isolation
# visible in the CI log. The authoritative isolation is `unshare -n` in
# benchmark.yml's Benchmark step (docs/DECISION-crate-policy.md, Task 9).
#
# We attempt a short TCP connection to a well-known host+port. If it SUCCEEDS,
# egress is NOT blocked — we print a loud warning. If it FAILS (the expected,
# desired outcome), we note that isolation looks effective.
set -uo pipefail

target_host="1.1.1.1"
target_port="443"

if timeout 5 bash -c "cat < /dev/null > /dev/tcp/${target_host}/${target_port}" 2>/dev/null; then
  echo "::warning::assert-no-network: outbound connection to ${target_host}:${target_port} SUCCEEDED — the scored run is NOT network-isolated. Check the unshare -n step." >&2
else
  echo "assert-no-network: outbound to ${target_host}:${target_port} blocked — network isolation looks effective."
fi
