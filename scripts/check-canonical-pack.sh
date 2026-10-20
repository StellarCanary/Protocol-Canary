#!/usr/bin/env bash
# Checks that a built stellar-canary can load a canonical fixture pack and that
# its offline fixtures pass. Used by CI against ProtocolCanary-Fixtures, and
# runnable by hand:
#
#   scripts/check-canonical-pack.sh <path-to-stellar-canary> <pack-dir> <min-xdr-fixtures> [malformed-vector]
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
# It makes no network call: RPC and Soroban checks are switched off in the
# throwaway project, so only the offline XDR fixtures execute. It does not say
# anything about the RPC or Soroban fixtures.
set -euo pipefail

bin=$(realpath "$1")
pack=$(realpath "$2")
min_xdr=${3:-1}
malformed=${4:-}
[ -z "$malformed" ] || malformed=$(realpath "$malformed")

work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT
cd "$work"
printf 'version = 1\nprotocol = 28\n\n[tests]\nxdr = true\nrpc = false\nsoroban = false\n' > .stellar-canary.toml

echo "== fixtures listing loads"
"$bin" fixtures --fixtures-dir "$pack" --protocol 28 > listing.txt
cat listing.txt

echo "== offline check passes"
"$bin" check --fixtures-dir "$pack" --protocol 28 --json --no-cache > report.json
python3 - "$min_xdr" <<'PY'
import json, sys
report = json.load(open("report.json"))
need = int(sys.argv[1])
counts = report["counts"]
assert report["status"] == "pass", report["status"]
assert counts["failed"] == 0 and counts["errors"] == 0, counts
assert counts["total"] >= need, f"expected at least {need} executed fixtures, got {counts}"
assert all(r["surface"] == "xdr" for r in report["results"]), "only offline fixtures should run"
assert all(r.get("source") == "live" for r in report["results"]), "--no-cache results must be live"
print("ok:", counts)
PY

echo "== a protocol the pack has no fixtures for is refused, not passed"
set +e
"$bin" check --fixtures-dir "$pack" --protocol 29 > out29.txt 2> err29.txt
code=$?
set -e
[ "$code" -eq 2 ] || { echo "expected exit 2, got $code"; cat err29.txt; exit 1; }
grep -q "no checks ran" err29.txt || { echo "missing diagnostic"; cat err29.txt; exit 1; }
[ ! -s out29.txt ] || { echo "stdout should be empty"; exit 1; }
echo "ok: exit 2 with diagnostic"

echo "== --allow-empty restores the old exit code"
"$bin" check --fixtures-dir "$pack" --protocol 29 --allow-empty --json > allow.json 2> allow.err
grep -q '"total": 0' allow.json
grep -q "not evidence of compatibility" allow.err
echo "ok"

if [ -n "$malformed" ]; then
  echo "== a malformed vector is still rejected when loaded directly"
  mkdir vector
  cp "$malformed" vector/
  set +e
  "$bin" fixtures --fixtures-dir vector --protocol 28 > vec.txt 2> vec.err
  code=$?
  set -e
  [ "$code" -eq 4 ] || { echo "expected exit 4, got $code"; cat vec.err; exit 1; }
  grep -q "TOML parse error" vec.err || { echo "missing parse error"; cat vec.err; exit 1; }
  echo "ok: exit 4 with parse error"
fi
