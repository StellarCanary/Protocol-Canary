"""Validate the JSON report produced by the canonical offline pack check."""

from __future__ import annotations

import argparse
import json
import sys
from pathlib import Path
from typing import Any


class ReportValidationError(ValueError):
    """The canonical pack report does not prove the requested checks ran."""


def _fixture_ids(csv: str) -> tuple[str, ...]:
    ids = tuple(part.strip() for part in csv.split(",") if part.strip())
    if len(set(ids)) != len(ids):
        raise ReportValidationError("expected XDR fixture ids contain duplicates")
    return ids


def validate_report(
    report: Any,
    *,
    protocol: int,
    minimum_xdr: int,
    expected_xdr_ids: tuple[str, ...] = (),
    expected_skipped_id: str = "",
) -> dict[str, int]:
    """Require successful, uncached XDR results and optional exact pack IDs."""
    if not isinstance(report, dict):
        raise ReportValidationError("report must be a JSON object")
    if report.get("targetProtocol") != protocol:
        raise ReportValidationError(
            f"expected target protocol {protocol}, got {report.get('targetProtocol')!r}"
        )
    if report.get("status") != "pass":
        raise ReportValidationError(f"report status is {report.get('status')!r}, not 'pass'")

    counts = report.get("counts")
    results = report.get("results")
    skipped = report.get("skipped")
    if not isinstance(counts, dict) or not isinstance(results, list) or not isinstance(skipped, list):
        raise ReportValidationError("report must contain counts, results, and skipped arrays")

    required_zero = ("failed", "errors")
    for key in required_zero:
        if counts.get(key) != 0:
            raise ReportValidationError(f"counts.{key} must be zero, got {counts.get(key)!r}")
    if counts.get("total") != len(results):
        raise ReportValidationError(
            f"counts.total ({counts.get('total')!r}) does not match {len(results)} results"
        )
    if counts.get("passed") != len(results):
        raise ReportValidationError(
            f"counts.passed ({counts.get('passed')!r}) does not match {len(results)} results"
        )
    if counts.get("skipped") != len(skipped):
        raise ReportValidationError(
            f"counts.skipped ({counts.get('skipped')!r}) does not match {len(skipped)} skips"
        )
    observed_ids: list[str] = []
    for index, result in enumerate(results):
        if not isinstance(result, dict):
            raise ReportValidationError(f"results[{index}] must be an object")
        fixture_id = result.get("fixtureId")
        if not isinstance(fixture_id, str) or not fixture_id:
            raise ReportValidationError(f"results[{index}] has no fixtureId")
        if result.get("protocol") != protocol:
            raise ReportValidationError(f"result {fixture_id} targets the wrong protocol")
        if result.get("surface") != "xdr":
            raise ReportValidationError(
                f"result {fixture_id} has surface {result.get('surface')!r}; only XDR may run"
            )
        if result.get("status") != "pass":
            raise ReportValidationError(
                f"XDR fixture {fixture_id} has status {result.get('status')!r}"
            )
        if result.get("source") != "live":
            raise ReportValidationError(
                f"XDR fixture {fixture_id} did not produce an uncached result"
            )
        observed_ids.append(fixture_id)

    if len(set(observed_ids)) != len(observed_ids):
        raise ReportValidationError("the report contains duplicate XDR fixture results")

    if expected_xdr_ids:
        expected = set(expected_xdr_ids)
        observed = set(observed_ids)
        missing = sorted(expected - observed)
        extra = sorted(observed - expected)
        if missing or extra:
            raise ReportValidationError(
                f"executed XDR fixture ids differ; missing={missing}, extra={extra}"
            )

    if len(results) < minimum_xdr:
        raise ReportValidationError(
            f"expected at least {minimum_xdr} executed XDR fixtures, got {len(results)}"
        )

    skipped_ids: list[str] = []
    for index, item in enumerate(skipped):
        if not isinstance(item, dict):
            raise ReportValidationError(f"skipped[{index}] must be an object")
        fixture_id = item.get("fixtureId")
        if not isinstance(fixture_id, str) or not fixture_id:
            raise ReportValidationError(f"skipped[{index}] has no fixtureId")
        skipped_ids.append(fixture_id)

    if expected_skipped_id:
        if expected_skipped_id in observed_ids:
            raise ReportValidationError(
                f"non-XDR fixture {expected_skipped_id} must not appear in results"
            )
        if tuple(skipped_ids) != (expected_skipped_id,):
            raise ReportValidationError(
                f"expected only skipped fixture {expected_skipped_id}, got {skipped_ids}"
            )
        skip = skipped[0]
        if (
            skip.get("surface") != "rpc"
            or skip.get("code") != "SURFACE_DISABLED"
            or "rpc checks are disabled in configuration" not in skip.get("reason", "")
        ):
            raise ReportValidationError(
                f"fixture {expected_skipped_id} was not explicitly skipped as disabled RPC"
            )

    return {key: counts[key] for key in ("total", "passed", "failed", "errors", "skipped")}


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("report", type=Path)
    parser.add_argument("--protocol", type=int, required=True)
    parser.add_argument("--minimum-xdr", type=int, required=True)
    parser.add_argument("--expected-xdr-ids", default="")
    parser.add_argument("--expected-skipped-id", default="")
    args = parser.parse_args(argv)

    try:
        report = json.loads(args.report.read_text(encoding="utf-8"))
        ids = _fixture_ids(args.expected_xdr_ids)
        counts = validate_report(
            report,
            protocol=args.protocol,
            minimum_xdr=args.minimum_xdr,
            expected_xdr_ids=ids,
            expected_skipped_id=args.expected_skipped_id,
        )
    except (OSError, UnicodeError, json.JSONDecodeError, ReportValidationError) as exc:
        print(f"canonical pack report invalid: {exc}", file=sys.stderr)
        return 1

    print(f"ok: protocol {args.protocol} offline results {counts}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
