#!/usr/bin/env python3
"""Tests for scripts/compare_exec_csv.py mismatch classification.

Run with: python3 scripts/test_compare_exec_csv.py

The tests use unittest (stdlib only) so they can run without any
Python package manager or venv. They lock the 4-category
classification contract from `close_loop.verification.md` §6.4 #5:

    state-change / sidecar difference  (most serious — frozen by spec)
    result-code only
    energy only                         (includes bandwidth_used)
    return-data only

No additional public tags exist. Unknown columns fall back to the
state-change category so a newly-added column cannot silently pass
a regression.

The test CSVs use the exact column layout from
`framework/.../execution/reporting/ExecutionCsvRecord.java:getCsvHeader()`
so the classification family membership stays faithful to production.
"""

from __future__ import annotations

import io
import json
import os
import sys
import tempfile
import unittest

HERE = os.path.dirname(os.path.abspath(__file__))
sys.path.insert(0, HERE)

import compare_exec_csv as cmp  # noqa: E402


# The 50-column header from
# `framework/.../execution/reporting/ExecutionCsvRecord.java:702`.
# Kept as a tuple so the tests stay hermetic — if production adds a
# new column, these tests need to be updated deliberately rather
# than silently drifting.
CSV_HEADER = (
    "run_id,exec_mode,storage_mode,block_num,block_id_hex,is_witness_signed,"
    "block_timestamp,tx_index_in_block,tx_id_hex,owner_address_hex,contract_type,"
    "is_constant,fee_limit,is_success,result_code,energy_used,return_data_hex,"
    "return_data_len,runtime_error,"
    "state_changes_json,state_change_count,state_digest_sha256,"
    "account_changes_json,account_change_count,account_digest_sha256,"
    "evm_storage_changes_json,evm_storage_change_count,evm_storage_digest_sha256,"
    "trc10_balance_changes_json,trc10_balance_change_count,trc10_balance_digest_sha256,"
    "trc10_issuance_changes_json,trc10_issuance_change_count,trc10_issuance_digest_sha256,"
    "vote_changes_json,vote_change_count,vote_digest_sha256,"
    "freeze_changes_json,freeze_change_count,freeze_digest_sha256,"
    "global_resource_changes_json,global_resource_change_count,global_resource_digest_sha256,"
    "account_resource_usage_changes_json,account_resource_usage_change_count,"
    "account_resource_usage_digest_sha256,"
    "log_entries_json,log_entry_count,log_entries_digest_sha256,"
    "ts_ms"
).split(",")


def base_row() -> dict:
    """Return a dict with every column pre-populated with a stable value.

    Tests mutate individual fields to simulate specific mismatches;
    starting from a fully-populated row means the "no mismatch" baseline
    is the common case.
    """
    return {
        # Base + metadata
        "run_id": "run-embedded",
        "exec_mode": "EMBEDDED",
        "storage_mode": "EMBEDDED",
        "block_num": "100",
        "block_id_hex": "0000000000000064",
        "is_witness_signed": "true",
        "block_timestamp": "1700000000",
        "tx_index_in_block": "0",
        "tx_id_hex": "deadbeef",
        "owner_address_hex": "41aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
        "contract_type": "TransferContract",
        "is_constant": "false",
        "fee_limit": "1000000",
        "is_success": "true",
        "result_code": "SUCCESS",
        "energy_used": "0",
        "return_data_hex": "",
        "return_data_len": "0",
        "runtime_error": "",
        "ts_ms": "1700000001000",
        # All domain JSON + count + digest columns — use stable defaults.
        "state_changes_json": "[]",
        "state_change_count": "0",
        "state_digest_sha256": "sha_empty",
        "account_changes_json": "[]",
        "account_change_count": "0",
        "account_digest_sha256": "acc_empty",
        "evm_storage_changes_json": "[]",
        "evm_storage_change_count": "0",
        "evm_storage_digest_sha256": "evm_empty",
        "trc10_balance_changes_json": "[]",
        "trc10_balance_change_count": "0",
        "trc10_balance_digest_sha256": "trc10_bal_empty",
        "trc10_issuance_changes_json": "[]",
        "trc10_issuance_change_count": "0",
        "trc10_issuance_digest_sha256": "trc10_iss_empty",
        "vote_changes_json": "[]",
        "vote_change_count": "0",
        "vote_digest_sha256": "vote_empty",
        "freeze_changes_json": "[]",
        "freeze_change_count": "0",
        "freeze_digest_sha256": "freeze_empty",
        "global_resource_changes_json": "[]",
        "global_resource_change_count": "0",
        "global_resource_digest_sha256": "gres_empty",
        "account_resource_usage_changes_json": "[]",
        "account_resource_usage_change_count": "0",
        "account_resource_usage_digest_sha256": "aext_empty",
        "log_entries_json": "[]",
        "log_entry_count": "0",
        "log_entries_digest_sha256": "log_empty",
    }


def make_row(overrides: dict) -> list:
    """Materialize a row in CSV_HEADER order with the given overrides."""
    row = base_row()
    row.update(overrides)
    return [row[name] for name in CSV_HEADER]


class ClassifyRowTests(unittest.TestCase):
    """Unit tests for the `_classify_row` helper."""

    def setUp(self) -> None:
        self.compare_idx = [
            i
            for i, name in enumerate(CSV_HEADER)
            if name not in cmp.COMPARE_EXCLUDED
        ]

    def classify(self, overrides: dict) -> tuple:
        r1 = make_row({})
        r2 = make_row(overrides)
        return cmp._classify_row(CSV_HEADER, self.compare_idx, r1, r2)

    def test_identical_rows_return_none(self) -> None:
        cat, diffs = self.classify({})
        self.assertIsNone(cat)
        self.assertEqual(diffs, [])

    def test_only_run_id_differs_is_ignored(self) -> None:
        # run_id / exec_mode / storage_mode / ts_ms are in IGNORED_COLS
        # and must NOT trigger any classification.
        cat, diffs = self.classify({
            "run_id": "run-remote",
            "exec_mode": "REMOTE",
            "storage_mode": "REMOTE",
            "ts_ms": "1700000099999",
        })
        self.assertIsNone(cat)
        self.assertEqual(diffs, [])

    def test_result_code_only_classification(self) -> None:
        cat, diffs = self.classify({
            "is_success": "false",
            "result_code": "REVERT",
            "runtime_error": "Call reverted",
        })
        self.assertEqual(cat, cmp.CATEGORY_RESULT_CODE)
        cols = {c for (c, _v1, _v2) in diffs}
        self.assertEqual(cols, {"is_success", "result_code", "runtime_error"})

    def test_energy_only_classification(self) -> None:
        cat, diffs = self.classify({"energy_used": "42"})
        self.assertEqual(cat, cmp.CATEGORY_ENERGY)
        self.assertEqual(diffs, [("energy_used", "0", "42")])

    def test_return_data_only_classification(self) -> None:
        cat, diffs = self.classify({
            "return_data_hex": "deadbeef",
            "return_data_len": "4",
        })
        self.assertEqual(cat, cmp.CATEGORY_RETURN_DATA)
        cols = {c for (c, _v1, _v2) in diffs}
        self.assertEqual(cols, {"return_data_hex", "return_data_len"})

    def test_state_change_classification_on_state_digest(self) -> None:
        cat, diffs = self.classify({"state_digest_sha256": "sha_different"})
        self.assertEqual(cat, cmp.CATEGORY_STATE_CHANGE)
        self.assertEqual(len(diffs), 1)

    def test_state_change_classification_on_account_digest(self) -> None:
        cat, _diffs = self.classify({
            "account_digest_sha256": "acc_different",
            "account_change_count": "1",
        })
        self.assertEqual(cat, cmp.CATEGORY_STATE_CHANGE)

    def test_state_change_classification_on_freeze_family(self) -> None:
        cat, _diffs = self.classify({
            "freeze_digest_sha256": "freeze_different",
            "freeze_change_count": "2",
        })
        self.assertEqual(cat, cmp.CATEGORY_STATE_CHANGE)

    def test_state_change_classification_on_trc10_issuance(self) -> None:
        cat, _diffs = self.classify({
            "trc10_issuance_digest_sha256": "issue_different",
            "trc10_issuance_change_count": "1",
        })
        self.assertEqual(cat, cmp.CATEGORY_STATE_CHANGE)

    def test_state_change_beats_energy_when_both_differ(self) -> None:
        # Severity ordering: state-change is more serious than energy,
        # so a row where both differ reports as state-change.
        cat, _diffs = self.classify({
            "energy_used": "99",
            "state_digest_sha256": "sha_different",
        })
        self.assertEqual(cat, cmp.CATEGORY_STATE_CHANGE)

    def test_state_change_beats_result_code_when_both_differ(self) -> None:
        cat, _diffs = self.classify({
            "is_success": "false",
            "state_digest_sha256": "sha_different",
        })
        self.assertEqual(cat, cmp.CATEGORY_STATE_CHANGE)

    def test_bandwidth_used_folds_into_energy_family(self) -> None:
        # bandwidth_used is not in the production header today, but the
        # spec only freezes 4 categories — so when the column is added
        # it must classify as "energy only", not its own tag.
        header = CSV_HEADER + ["bandwidth_used"]
        r1 = make_row({}) + ["0"]
        r2 = make_row({}) + ["42"]
        compare_idx = [
            i for i, name in enumerate(header) if name not in cmp.COMPARE_EXCLUDED
        ]
        cat, diffs = cmp._classify_row(header, compare_idx, r1, r2)
        self.assertEqual(cat, cmp.CATEGORY_ENERGY)
        self.assertEqual(diffs, [("bandwidth_used", "0", "42")])

    def test_unknown_column_defaults_to_state_change(self) -> None:
        # A newly-added column with no family mapping must FAIL into the
        # most-serious bucket so it cannot silently mask a regression.
        header = CSV_HEADER + ["future_extension_column"]
        r1 = make_row({}) + ["a"]
        r2 = make_row({}) + ["b"]
        compare_idx = [
            i for i, name in enumerate(header) if name not in cmp.COMPARE_EXCLUDED
        ]
        cat, diffs = cmp._classify_row(header, compare_idx, r1, r2)
        self.assertEqual(cat, cmp.CATEGORY_STATE_CHANGE)
        self.assertEqual(diffs, [("future_extension_column", "a", "b")])

    def test_json_columns_are_ignored_for_classification(self) -> None:
        # state_changes_json differs but state_digest_sha256 + count do not.
        # The JSON column is a context-only column; the classification
        # must NOT report a mismatch from it alone.
        cat, diffs = self.classify({
            "state_changes_json": '[{"kind":"StorageChange"}]',
        })
        self.assertIsNone(cat)
        self.assertEqual(diffs, [])


class WalkMismatchesTests(unittest.TestCase):
    """End-to-end tests that exercise the walk + summary pipeline."""

    def _write_csv(self, path: str, rows: list) -> None:
        with open(path, "w", newline="") as f:
            f.write(",".join(CSV_HEADER))
            f.write("\n")
            for row in rows:
                f.write(",".join(row))
                f.write("\n")

    def test_walk_returns_empty_when_identical(self) -> None:
        r = make_row({})
        m = cmp.walk_mismatches(CSV_HEADER, [r], [r])
        self.assertEqual(m, [])

    def test_walk_classifies_mixed_mismatches(self) -> None:
        d1 = [
            make_row({"tx_id_hex": "tx0"}),
            make_row({"tx_id_hex": "tx1"}),
            make_row({"tx_id_hex": "tx2"}),
            make_row({"tx_id_hex": "tx3"}),
        ]
        d2 = [
            # tx0: state-change (digest differs)
            make_row({"tx_id_hex": "tx0", "state_digest_sha256": "sha_drift"}),
            # tx1: energy only
            make_row({"tx_id_hex": "tx1", "energy_used": "99"}),
            # tx2: result-code only (is_success + runtime_error differ)
            make_row({
                "tx_id_hex": "tx2",
                "is_success": "false",
                "runtime_error": "Call reverted",
            }),
            # tx3: identical
            make_row({"tx_id_hex": "tx3"}),
        ]
        mismatches = cmp.walk_mismatches(CSV_HEADER, d1, d2)
        self.assertEqual(len(mismatches), 3)
        by_tx = {m.tx_id: m.category for m in mismatches}
        self.assertEqual(by_tx["tx0"], cmp.CATEGORY_STATE_CHANGE)
        self.assertEqual(by_tx["tx1"], cmp.CATEGORY_ENERGY)
        self.assertEqual(by_tx["tx2"], cmp.CATEGORY_RESULT_CODE)
        self.assertNotIn("tx3", by_tx)

    def test_walk_records_block_and_tx_id(self) -> None:
        r1 = make_row({"block_num": "1234", "tx_id_hex": "feedface"})
        r2 = make_row({
            "block_num": "1234",
            "tx_id_hex": "feedface",
            "energy_used": "1",
        })
        mismatches = cmp.walk_mismatches(CSV_HEADER, [r1], [r2])
        self.assertEqual(len(mismatches), 1)
        self.assertEqual(mismatches[0].block, "1234")
        self.assertEqual(mismatches[0].tx_id, "feedface")

    def test_end_to_end_via_load_csv(self) -> None:
        with tempfile.TemporaryDirectory() as d:
            p1 = os.path.join(d, "embedded.csv")
            p2 = os.path.join(d, "remote.csv")
            self._write_csv(p1, [make_row({"tx_id_hex": "aa"})])
            self._write_csv(
                p2,
                [make_row({
                    "tx_id_hex": "aa",
                    "run_id": "run-remote",  # ignored
                    "state_digest_sha256": "drift",
                })],
            )
            h1, d1 = cmp.load_csv(p1)
            h2, d2 = cmp.load_csv(p2)
            self.assertEqual(h1, h2)
            mismatches = cmp.walk_mismatches(h1, d1, d2)
            self.assertEqual(len(mismatches), 1)
            self.assertEqual(mismatches[0].category, cmp.CATEGORY_STATE_CHANGE)

    def test_ignored_columns_do_not_trigger_mismatch(self) -> None:
        r1 = make_row({"run_id": "embedded", "ts_ms": "1"})
        r2 = make_row({"run_id": "remote", "ts_ms": "999"})
        m = cmp.walk_mismatches(CSV_HEADER, [r1], [r2])
        self.assertEqual(m, [])

    def test_summary_aggregates_by_category(self) -> None:
        d1 = [
            make_row({"tx_id_hex": f"t{i}"}) for i in range(5)
        ]
        d2 = [
            make_row({"tx_id_hex": "t0", "state_digest_sha256": "a"}),
            make_row({"tx_id_hex": "t1", "state_digest_sha256": "b"}),
            make_row({"tx_id_hex": "t2", "energy_used": "1"}),
            make_row({"tx_id_hex": "t3", "is_success": "false"}),
            make_row({"tx_id_hex": "t4"}),  # identical
        ]
        mismatches = cmp.walk_mismatches(CSV_HEADER, d1, d2)
        # Count by category
        per_cat = {}
        for m in mismatches:
            per_cat[m.category] = per_cat.get(m.category, 0) + 1
        self.assertEqual(per_cat[cmp.CATEGORY_STATE_CHANGE], 2)
        self.assertEqual(per_cat[cmp.CATEGORY_ENERGY], 1)
        self.assertEqual(per_cat[cmp.CATEGORY_RESULT_CODE], 1)


class SeverityOrderTests(unittest.TestCase):
    """Lock the parts of the severity ordering that the spec freezes.

    Section 6.4 #5 only freezes "state-change / sidecar difference" as
    the most-serious category. The relative order of the three "only"
    categories is internal display policy and is intentionally NOT
    locked here so it can be tuned without churning the test suite.
    """

    def test_severity_list_is_exactly_the_four_spec_categories(self) -> None:
        self.assertEqual(
            set(cmp.CATEGORY_SEVERITY),
            {
                cmp.CATEGORY_STATE_CHANGE,
                cmp.CATEGORY_RESULT_CODE,
                cmp.CATEGORY_ENERGY,
                cmp.CATEGORY_RETURN_DATA,
            },
        )
        self.assertEqual(len(cmp.CATEGORY_SEVERITY), 4)

    def test_state_change_is_most_serious(self) -> None:
        self.assertEqual(cmp.CATEGORY_SEVERITY[0], cmp.CATEGORY_STATE_CHANGE)


class CliTests(unittest.TestCase):
    """Exercise `_parse_args` and `main` directly.

    The previous iteration broke the default-mode exit code (it
    started returning 1 on mismatch instead of 0, breaking
    `collect_remote_results.sh` which runs with `set -e`). The pure
    classifier tests above did not catch it because they never
    exercise the CLI surface. These tests lock the wrapper-visible
    behavior so a regression there fails loudly.
    """

    def _write_csv(self, path: str, header: list, rows: list) -> None:
        with open(path, "w", newline="") as f:
            f.write(",".join(header))
            f.write("\n")
            for row in rows:
                f.write(",".join(row))
                f.write("\n")

    def test_parse_args_default_mode(self) -> None:
        p1, p2, classify_all, json_mode = cmp._parse_args(
            ["compare_exec_csv.py", "a.csv", "b.csv"]
        )
        self.assertEqual((p1, p2), ("a.csv", "b.csv"))
        self.assertFalse(classify_all)
        self.assertFalse(json_mode)

    def test_parse_args_classify_all(self) -> None:
        _, _, classify_all, json_mode = cmp._parse_args(
            ["compare_exec_csv.py", "--classify-all", "a.csv", "b.csv"]
        )
        self.assertTrue(classify_all)
        self.assertFalse(json_mode)

    def test_parse_args_json_implies_classify_all(self) -> None:
        _, _, classify_all, json_mode = cmp._parse_args(
            ["compare_exec_csv.py", "--json", "a.csv", "b.csv"]
        )
        self.assertTrue(classify_all)
        self.assertTrue(json_mode)

    def test_parse_args_unknown_flag_exits_2(self) -> None:
        with self.assertRaises(SystemExit) as ctx:
            cmp._parse_args(
                ["compare_exec_csv.py", "--bogus", "a.csv", "b.csv"]
            )
        self.assertEqual(ctx.exception.code, 2)

    def test_parse_args_missing_positional_exits_2(self) -> None:
        with self.assertRaises(SystemExit) as ctx:
            cmp._parse_args(["compare_exec_csv.py", "only-one.csv"])
        self.assertEqual(ctx.exception.code, 2)

    def _run_main(self, argv: list) -> int:
        old_argv = sys.argv
        sys.argv = argv
        try:
            cmp.main()
            return 0
        except SystemExit as e:
            return int(e.code) if e.code is not None else 0
        finally:
            sys.argv = old_argv

    def test_main_default_mode_exits_0_on_mismatch(self) -> None:
        # CRITICAL regression test: default mode MUST exit 0 on
        # mismatch so `collect_remote_results.sh` (which runs with
        # `set -e`) does not abort at step 11.
        with tempfile.TemporaryDirectory() as d:
            p1 = os.path.join(d, "embedded.csv")
            p2 = os.path.join(d, "remote.csv")
            self._write_csv(p1, CSV_HEADER, [make_row({"tx_id_hex": "aa"})])
            self._write_csv(
                p2,
                CSV_HEADER,
                [make_row({"tx_id_hex": "aa", "state_digest_sha256": "drift"})],
            )
            rc = self._run_main(["compare_exec_csv.py", p1, p2])
            self.assertEqual(rc, 0)

    def test_main_default_mode_exits_0_when_identical(self) -> None:
        with tempfile.TemporaryDirectory() as d:
            p1 = os.path.join(d, "embedded.csv")
            p2 = os.path.join(d, "remote.csv")
            row = make_row({"tx_id_hex": "aa"})
            self._write_csv(p1, CSV_HEADER, [row])
            self._write_csv(p2, CSV_HEADER, [row])
            rc = self._run_main(["compare_exec_csv.py", p1, p2])
            self.assertEqual(rc, 0)

    def test_main_classify_all_exits_1_on_mismatch(self) -> None:
        with tempfile.TemporaryDirectory() as d:
            p1 = os.path.join(d, "embedded.csv")
            p2 = os.path.join(d, "remote.csv")
            self._write_csv(p1, CSV_HEADER, [make_row({"tx_id_hex": "aa"})])
            self._write_csv(
                p2,
                CSV_HEADER,
                [make_row({"tx_id_hex": "aa", "energy_used": "99"})],
            )
            rc = self._run_main(
                ["compare_exec_csv.py", "--classify-all", p1, p2]
            )
            self.assertEqual(rc, 1)

    def test_main_classify_all_exits_0_when_identical(self) -> None:
        with tempfile.TemporaryDirectory() as d:
            p1 = os.path.join(d, "embedded.csv")
            p2 = os.path.join(d, "remote.csv")
            row = make_row({"tx_id_hex": "aa"})
            self._write_csv(p1, CSV_HEADER, [row])
            self._write_csv(p2, CSV_HEADER, [row])
            rc = self._run_main(
                ["compare_exec_csv.py", "--classify-all", p1, p2]
            )
            self.assertEqual(rc, 0)

    def test_main_json_mode_exits_1_on_mismatch(self) -> None:
        with tempfile.TemporaryDirectory() as d:
            p1 = os.path.join(d, "embedded.csv")
            p2 = os.path.join(d, "remote.csv")
            self._write_csv(p1, CSV_HEADER, [make_row({"tx_id_hex": "aa"})])
            self._write_csv(
                p2,
                CSV_HEADER,
                [make_row({"tx_id_hex": "aa", "is_success": "false"})],
            )
            buf = io.StringIO()
            old_stdout = sys.stdout
            sys.stdout = buf
            try:
                rc = self._run_main(
                    ["compare_exec_csv.py", "--json", p1, p2]
                )
            finally:
                sys.stdout = old_stdout
            self.assertEqual(rc, 1)
            payload = json.loads(buf.getvalue())
            self.assertEqual(payload["mismatch_count"], 1)
            self.assertEqual(
                payload["per_category"][cmp.CATEGORY_RESULT_CODE], 1
            )

    def test_main_json_mode_header_mismatch(self) -> None:
        with tempfile.TemporaryDirectory() as d:
            p1 = os.path.join(d, "embedded.csv")
            p2 = os.path.join(d, "remote.csv")
            self._write_csv(p1, CSV_HEADER, [make_row({})])
            self._write_csv(
                p2, CSV_HEADER + ["extra_col"], [make_row({}) + ["x"]]
            )
            buf = io.StringIO()
            old_stdout = sys.stdout
            sys.stdout = buf
            try:
                rc = self._run_main(
                    ["compare_exec_csv.py", "--json", p1, p2]
                )
            finally:
                sys.stdout = old_stdout
            self.assertEqual(rc, 1)
            payload = json.loads(buf.getvalue())
            self.assertTrue(payload["header_mismatch"])

    def test_main_empty_csv_exits_2(self) -> None:
        # Regression: a zero-byte CSV is a sign that a runner crashed
        # before writing any output. Treating it as "no mismatches"
        # would silently pass a totally broken collection step. The
        # script must reject it explicitly.
        with tempfile.TemporaryDirectory() as d:
            p1 = os.path.join(d, "embedded.csv")
            p2 = os.path.join(d, "remote.csv")
            open(p1, "w").close()
            open(p2, "w").close()
            buf = io.StringIO()
            old_stderr = sys.stderr
            sys.stderr = buf
            try:
                rc = self._run_main(["compare_exec_csv.py", p1, p2])
            finally:
                sys.stderr = old_stderr
            self.assertEqual(rc, 2)
            self.assertIn("empty or headerless CSV", buf.getvalue())

    def test_main_blank_line_csv_exits_2(self) -> None:
        # `csv.reader` on a file containing just "\n" returns `[[]]` —
        # a single empty row, not zero rows. The fix must reject it
        # the same way as a zero-byte file, otherwise a runner that
        # opened the output file but crashed before writing the
        # header would slip through as `header=[]`.
        with tempfile.TemporaryDirectory() as d:
            p1 = os.path.join(d, "embedded.csv")
            p2 = os.path.join(d, "remote.csv")
            with open(p1, "w") as f:
                f.write("\n")
            with open(p2, "w") as f:
                f.write("\n")
            buf = io.StringIO()
            old_stderr = sys.stderr
            sys.stderr = buf
            try:
                rc = self._run_main(["compare_exec_csv.py", p1, p2])
            finally:
                sys.stderr = old_stderr
            self.assertEqual(rc, 2)
            self.assertIn("empty or headerless CSV", buf.getvalue())

    def test_main_one_empty_csv_exits_2(self) -> None:
        with tempfile.TemporaryDirectory() as d:
            p1 = os.path.join(d, "embedded.csv")
            p2 = os.path.join(d, "remote.csv")
            self._write_csv(p1, CSV_HEADER, [make_row({})])
            open(p2, "w").close()
            buf = io.StringIO()
            old_stderr = sys.stderr
            sys.stderr = buf
            try:
                rc = self._run_main(["compare_exec_csv.py", p1, p2])
            finally:
                sys.stderr = old_stderr
            self.assertEqual(rc, 2)
            self.assertIn("empty or headerless CSV", buf.getvalue())

    def test_main_missing_file_exits_2(self) -> None:
        with tempfile.TemporaryDirectory() as d:
            p1 = os.path.join(d, "embedded.csv")
            p2 = os.path.join(d, "does-not-exist.csv")
            self._write_csv(p1, CSV_HEADER, [make_row({})])
            buf = io.StringIO()
            old_stderr = sys.stderr
            sys.stderr = buf
            try:
                rc = self._run_main(["compare_exec_csv.py", p1, p2])
            finally:
                sys.stderr = old_stderr
            self.assertEqual(rc, 2)
            self.assertIn("file not found", buf.getvalue())


class AlignmentBreakTests(unittest.TestCase):
    """Tests for row-alignment drift detection.

    When one CSV inserts or drops a row, comparing by strict row index
    misclassifies every later row as a `state-change / sidecar` diff
    and drowns the real signal. `find_alignment_break()` detects the
    first row where `tx_id_hex` or `block_num` diverge so `main()` can
    stop classifying and surface the break explicitly.
    """

    def _write_csv(self, path: str, rows: list) -> None:
        with open(path, "w", newline="") as f:
            f.write(",".join(CSV_HEADER))
            f.write("\n")
            for row in rows:
                f.write(",".join(row))
                f.write("\n")

    def _run_main(self, argv: list) -> int:
        old_argv = sys.argv
        sys.argv = argv
        try:
            cmp.main()
            return 0
        except SystemExit as e:
            return int(e.code) if e.code is not None else 0
        finally:
            sys.argv = old_argv

    def test_find_returns_none_when_aligned(self) -> None:
        d1 = [make_row({"tx_id_hex": "a"}), make_row({"tx_id_hex": "b"})]
        d2 = [make_row({"tx_id_hex": "a"}), make_row({"tx_id_hex": "b"})]
        self.assertIsNone(cmp.find_alignment_break(CSV_HEADER, d1, d2))

    def test_find_detects_inserted_row(self) -> None:
        d1 = [
            make_row({"tx_id_hex": "a"}),
            make_row({"tx_id_hex": "b"}),
            make_row({"tx_id_hex": "c"}),
        ]
        # d2 inserts a new row at index 1 (x), shifting b/c down by one.
        d2 = [
            make_row({"tx_id_hex": "a"}),
            make_row({"tx_id_hex": "x"}),
            make_row({"tx_id_hex": "b"}),
        ]
        ab = cmp.find_alignment_break(CSV_HEADER, d1, d2)
        self.assertIsNotNone(ab)
        self.assertEqual(ab.row_index, 1)
        self.assertEqual(ab.embedded_tx_id, "b")
        self.assertEqual(ab.remote_tx_id, "x")

    def test_find_detects_dropped_row(self) -> None:
        d1 = [
            make_row({"tx_id_hex": "a"}),
            make_row({"tx_id_hex": "b"}),
            make_row({"tx_id_hex": "c"}),
        ]
        d2 = [
            make_row({"tx_id_hex": "a"}),
            make_row({"tx_id_hex": "c"}),
        ]
        ab = cmp.find_alignment_break(CSV_HEADER, d1, d2)
        self.assertIsNotNone(ab)
        self.assertEqual(ab.row_index, 1)
        self.assertEqual(ab.embedded_tx_id, "b")
        self.assertEqual(ab.remote_tx_id, "c")

    def test_main_classify_all_exits_1_on_alignment_break(self) -> None:
        with tempfile.TemporaryDirectory() as d:
            p1 = os.path.join(d, "embedded.csv")
            p2 = os.path.join(d, "remote.csv")
            self._write_csv(
                p1,
                [
                    make_row({"tx_id_hex": "a"}),
                    make_row({"tx_id_hex": "b"}),
                    make_row({"tx_id_hex": "c"}),
                ],
            )
            self._write_csv(
                p2,
                [
                    make_row({"tx_id_hex": "a"}),
                    make_row({"tx_id_hex": "x"}),
                    make_row({"tx_id_hex": "b"}),
                    make_row({"tx_id_hex": "c"}),
                ],
            )
            rc = self._run_main(
                ["compare_exec_csv.py", "--classify-all", p1, p2]
            )
            # Alignment break alone — no content mismatches before it —
            # must still fail classify-all mode so CI doesn't silently
            # pass a drifted run.
            self.assertEqual(rc, 1)

    def test_main_alignment_break_does_not_flood_mismatches(self) -> None:
        # Regression: without the pre-walk alignment check, a single
        # inserted row would produce N-1 false `state-change / sidecar`
        # mismatches (one per row past the break). Assert that only the
        # alignment break is reported when the post-break rows would
        # otherwise align to entirely different transactions.
        with tempfile.TemporaryDirectory() as d:
            p1 = os.path.join(d, "embedded.csv")
            p2 = os.path.join(d, "remote.csv")
            # Embedded: 4 rows. Remote: same 4 rows with a new row at
            # index 1. Comparing by strict index would give rows 1..3
            # a `state-change / sidecar` classification each, even
            # though every tx in both files executed identically.
            self._write_csv(
                p1,
                [
                    make_row({"tx_id_hex": "t0"}),
                    make_row({"tx_id_hex": "t1"}),
                    make_row({"tx_id_hex": "t2"}),
                    make_row({"tx_id_hex": "t3"}),
                ],
            )
            self._write_csv(
                p2,
                [
                    make_row({"tx_id_hex": "t0"}),
                    make_row({"tx_id_hex": "INSERTED"}),
                    make_row({"tx_id_hex": "t1"}),
                    make_row({"tx_id_hex": "t2"}),
                    make_row({"tx_id_hex": "t3"}),
                ],
            )
            buf = io.StringIO()
            old_stdout = sys.stdout
            sys.stdout = buf
            try:
                rc = self._run_main(
                    ["compare_exec_csv.py", "--json", p1, p2]
                )
            finally:
                sys.stdout = old_stdout
            self.assertEqual(rc, 1)
            report = json.loads(buf.getvalue())
            # The alignment break is surfaced separately.
            self.assertIsNotNone(report["alignment_break"])
            self.assertEqual(report["alignment_break"]["row_index"], 1)
            # Crucially, no content mismatches are reported after the
            # break — every row before it is identical.
            self.assertEqual(report["mismatch_count"], 0)


if __name__ == "__main__":
    unittest.main(verbosity=2)
