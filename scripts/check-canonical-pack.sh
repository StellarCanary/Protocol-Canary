#!/usr/bin/env bash
# Checks that a built stellar-canary can load a canonical fixture pack and that
# its offline fixtures pass. Used by CI against ProtocolCanary-Fixtures, and
# runnable by hand:
#
#   scripts/check-canonical-pack.sh <path-to-stellar-canary> <pack-dir> <min-xdr-fixtures> [malformed-vector] [protocol] [expected-xdr-ids] [expected-skipped-id]
#
# <pack-dir> is the executable pack (the `protocol-28` directory of
# ProtocolCanary-Fixtures), not the repository root: the loader walks every
# `*.toml` below the directory it is given, and the repository root also holds
# the negative conformance vectors under `tests/vectors`, which are malformed
# on purpose.
#
# [malformed-vector] is an optional path to one such vector. When given, the
# script also loads it on its own and requires the loader to reject it, so
# keeping the vectors out of <pack-dir> cannot hide a loader that stopped
# rejecting them.
#
# [protocol] defaults to 28 to preserve existing callers. The optional
# expected-id arguments make a protocol-specific leg prove that those exact
# XDR fixtures ran and that a named non-XDR fixture remained skipped.
#
# It makes no network call: RPC and Soroban checks are switched off in the
# throwaway project, so only the offline XDR fixtures execute. It does not say
# anything about the RPC or Soroban fixtures.
set -euo pipefail

bin=$(realpath "$1")
pack=$(realpath "$2")
min_xdr=${3:-1}
malformed=${4:-}
protocol=${5:-28}
expected_xdr_ids=${6:-}
expected_skipped_id=${7:-}
[ -z "$malformed" ] || malformed=$(realpath "$malformed")
[[ "$protocol" =~ ^[1-9][0-9]*$ ]] || { echo "protocol must be a positive integer" >&2; exit 2; }
script_dir=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)

work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT
cd "$work"
printf 'version = 1\nprotocol = %s\n\n[tests]\nxdr = true\nrpc = false\nsoroban = false\n' "$protocol" > .stellar-canary.toml

echo "== fixtures listing loads"
"$bin" fixtures --fixtures-dir "$pack" --protocol "$protocol" > listing.txt
cat listing.txt

echo "== offline check passes"
"$bin" check --fixtures-dir "$pack" --protocol "$protocol" --json --no-cache > report.json
python3 "$script_dir/canonical_pack_report.py" report.json \
  --protocol "$protocol" \
  --minimum-xdr "$min_xdr" \
  --expected-xdr-ids "$expected_xdr_ids" \
  --expected-skipped-id "$expected_skipped_id"

echo "== a protocol the pack has no fixtures for is refused, not passed"
empty_protocol=$((protocol + 1))
set +e
"$bin" check --fixtures-dir "$pack" --protocol "$empty_protocol" > out-empty.txt 2> err-empty.txt
code=$?
set -e
[ "$code" -eq 2 ] || { echo "expected exit 2, got $code"; cat err-empty.txt; exit 1; }
grep -q "no checks ran" err-empty.txt || { echo "missing diagnostic"; cat err-empty.txt; exit 1; }
[ ! -s out-empty.txt ] || { echo "stdout should be empty"; exit 1; }
echo "ok: exit 2 with diagnostic"

echo "== --allow-empty restores the old exit code"
"$bin" check --fixtures-dir "$pack" --protocol "$empty_protocol" --allow-empty --json > allow.json 2> allow.err
grep -q '"total": 0' allow.json
grep -q "not evidence of compatibility" allow.err
echo "ok"

if [ -n "$malformed" ]; then
  echo "== a malformed vector is still rejected when loaded directly"
  mkdir vector
  cp "$malformed" vector/
  set +e
  "$bin" fixtures --fixtures-dir vector --protocol "$protocol" > vec.txt 2> vec.err
  code=$?
  set -e
  [ "$code" -eq 4 ] || { echo "expected exit 4, got $code"; cat vec.err; exit 1; }
  grep -q "TOML parse error" vec.err || { echo "missing parse error"; cat vec.err; exit 1; }
  echo "ok: exit 4 with parse error"
fi
