#!/usr/bin/env bash
set -euo pipefail

root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
compiler="${DARGO_BIN:-$root/target/release/dargo}"
[[ "$compiler" = /* ]] || compiler="$PWD/$compiler"
[[ -x "$compiler" ]] || { echo "Build dargo first: $compiler" >&2; exit 1; }
work="$(mktemp -d "${TMPDIR:-/tmp}/psy-withdrawal-determinism.XXXXXX")"
echo "Determinism evidence: $work"

# Separate directories and processes rule out reuse of prior compiler output.
for run in 1 2 3; do
  dir="$work/run-$run"
  mkdir -p "$dir"
  cp "$root/psy-precompiles/withdrawal_tree/Dargo.toml" "$dir/"
  cp -R "$root/psy-precompiles/withdrawal_tree/src" "$dir/"
  (
    cd "$dir"
    DARGO_STD_PATH="$root/psy-std/std.psy" "$compiler" compile \
      --contract-name=PsyWithdrawalTreeContractRef \
      --method-names set_chain_root get_root get_chain_root append_leaf \
      append_withdrawal batch_append_withdrawals_2 batch_append_withdrawals_5 \
      >compile.log 2>&1
  ) || { echo "Compilation failed; see $dir/compile.log" >&2; exit 1; }
  test -s "$dir/target/withdrawal_tree.json"
  if ! cmp -s "$work/run-1/target/withdrawal_tree.json" "$dir/target/withdrawal_tree.json"; then
    echo "FAIL: withdrawal artifacts differ between runs 1 and $run; see $work" >&2
    exit 1
  fi
done
echo "PASS: three independent withdrawal compilations are byte-identical"
