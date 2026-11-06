"""Regression tests for exact canonical-pack result verification."""

from __future__ import annotations

import sys
import unittest
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT / "scripts"))

import canonical_pack_report as report_guard

P30_IDS = (
    "p30-xdr-cap0084-muxed-contract-address-roundtrip",
    "p30-xdr-cap0088-stellar-value-empty-tx-set-ms-roundtrip",
    "p30-xdr-cap0088-stellar-value-signed-ms-roundtrip",
)
P30_RPC_ID = "p30-rpc-version-info-protocol-30"


def p30_report() -> dict:
    return {
        "targetProtocol": 30,
        "status": "pass",
        "counts": {"total": 3, "passed": 3, "failed": 0, "errors": 0, "skipped": 1},
        "results": [
            {
                "fixtureId": fixture_id,
                "protocol": 30,
                "surface": "xdr",
                "status": "pass",
                "source": "live",
            }
            for fixture_id in P30_IDS
        ],
        "skipped": [
            {
                "fixtureId": P30_RPC_ID,
                "surface": "rpc",
                "code": "SURFACE_DISABLED",
                "reason": "rpc checks are disabled in configuration",
            }
        ],
    }


class CanonicalPackReportTests(unittest.TestCase):
    def assert_p30_valid(self, report: dict) -> None:
        report_guard.validate_report(
            report,
            protocol=30,
            minimum_xdr=3,
            expected_xdr_ids=P30_IDS,
            expected_skipped_id=P30_RPC_ID,
        )

    def test_all_expected_protocol30_xdr_fixtures_pass_and_rpc_is_skipped(self) -> None:
        self.assert_p30_valid(p30_report())

    def test_missing_expected_protocol30_fixture_fails(self) -> None:
        report = p30_report()
        report["results"].pop()
        report["counts"].update(total=2, passed=2)
        with self.assertRaisesRegex(report_guard.ReportValidationError, "missing=.*stellar-value-signed-ms"):
            self.assert_p30_valid(report)

    def test_skipped_expected_protocol30_fixture_fails(self) -> None:
        report = p30_report()
        missing = report["results"].pop()
        report["counts"].update(total=2, passed=2, skipped=2)
        report["skipped"].append(
            {
                "fixtureId": missing["fixtureId"],
                "surface": "xdr",
                "code": "SURFACE_DISABLED",
                "reason": "fixture skipped",
            }
        )
        with self.assertRaisesRegex(report_guard.ReportValidationError, "missing=.*stellar-value-signed-ms"):
            self.assert_p30_valid(report)

    def test_failed_expected_protocol30_fixture_fails(self) -> None:
        report = p30_report()
        report["status"] = "fail"
        report["counts"].update(passed=2, failed=1)
        report["results"][0]["status"] = "fail"
        with self.assertRaisesRegex(report_guard.ReportValidationError, "status is 'fail'"):
            self.assert_p30_valid(report)

    def test_rpc_fixture_in_results_fails_even_if_it_is_also_skipped(self) -> None:
        report = p30_report()
        report["counts"].update(total=4, passed=4)
        report["results"].append(
            {
                "fixtureId": P30_RPC_ID,
                "protocol": 30,
                "surface": "rpc",
                "status": "pass",
                "source": "live",
            }
        )
        with self.assertRaisesRegex(report_guard.ReportValidationError, "only XDR may run"):
            self.assert_p30_valid(report)

    def test_legacy_protocol28_minimum_check_still_accepts_xdr_results(self) -> None:
        report = {
            "targetProtocol": 28,
            "status": "pass",
            "counts": {"total": 3, "passed": 3, "failed": 0, "errors": 0, "skipped": 0},
            "results": [
                {
                    "fixtureId": f"p28-xdr-{index}",
                    "protocol": 28,
                    "surface": "xdr",
                    "status": "pass",
                    "source": "live",
                }
                for index in range(3)
            ],
            "skipped": [],
        }
        counts = report_guard.validate_report(report, protocol=28, minimum_xdr=3)
        self.assertEqual(counts["total"], 3)


if __name__ == "__main__":
    unittest.main()
