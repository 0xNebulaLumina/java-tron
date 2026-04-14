#!/usr/bin/env python3
"""Compare two execution CSV files row-by-row and classify mismatches.

This script is the Phase 2.A verification-rebuild follow-up from
`planning/close_loop.verification.md` §"Follow-up implementation items":
the original script did a row-level diff and stopped at the first
mismatch, which is fine for an interactive "open both files" workflow
but is not CI-suitable.

The new shape adds a classification layer on top of the row diff so
CI can surface an aggregate mismatch summary with one of the frozen
4 categories from `close_loop.verification.md` §6.4 #5:

    - state-change / sidecar difference — any of the domain digest
                                or change-count columns differ. Most
                                serious; blocks RR canonical-ready.
    - result-code only       — is_success / result_code / runtime_error
                                differ, nothing else.
    - energy only            — energy_used (or bandwidth_used when
                                present) differs, nothing else.
    - return-data only       — return_data_hex / return_data_len differ,
                                nothing else.

Unknown columns default to the most-serious `state-change / sidecar
difference` bucket so a newly-added CSV column cannot silently pass
a regression.

Usage:

    compare_exec_csv.py <embedded.csv> <remote.csv>
        Default mode. Prints the first mismatch with its classification
        tag plus a context snippet, and exits 0 whether or not a
        mismatch is found. The zero exit is intentional — existing
        shell wrappers (e.g. collect_remote_results.sh) run with
        `set -e` and invoke this script directly, so a non-zero exit
        would abort the wrapper at step 11. Callers that want a
        non-zero exit on mismatch must use --classify-all or --json.

    compare_exec_csv.py --classify-all <embedded.csv> <remote.csv>
        Walk every row, classify every mismatch, print a per-category
        summary plus the list of (tx_id, category) pairs, and exit 1
        if any mismatch is found. Suitable for CI jobs that want to
        see the full drift picture in a single run.

    compare_exec_csv.py --json <embedded.csv> <remote.csv>
        Same as --classify-all but emits machine-readable JSON so a
        downstream dashboard generator can consume it directly. Implies
        --classify-all. Exits 2 on missing or empty input files, 1 on
        header mismatch or row mismatch, 0 otherwise.

Column families used by the classification:

    RESULT_CODE_COLUMNS: is_success, result_code, runtime_error
    ENERGY_COLUMNS:      energy_used
    RETURN_DATA_COLUMNS: return_data_hex, return_data_len
    STATE_DIGEST_COLUMNS: state_digest_sha256, state_change_count,
                          account_digest_sha256, account_change_count,
                          evm_storage_digest_sha256, evm_storage_change_count,
                          trc10_balance_digest_sha256, trc10_balance_change_count,
                          trc10_issuance_digest_sha256, trc10_issuance_change_count,
                          vote_digest_sha256, vote_change_count,
                          freeze_digest_sha256, freeze_change_count,
                          global_resource_digest_sha256, global_resource_change_count,
                          account_resource_usage_digest_sha256,
                          account_resource_usage_change_count,
                          log_entries_digest_sha256, log_entry_count

    The raw `*_json` columns are deliberately EXCLUDED from classification
    because the digest columns carry the same signal and are O(1) to
    compare. They remain useful for printing the delta in the first-mismatch
    context dump.
"""

from __future__ import annotations

import csv
import json
import sys
from dataclasses import dataclass, field
from typing import Dict, Iterable, List, Optional, Tuple

# Columns that vary between runs but do not indicate a real mismatch.
# `run_id` / `exec_mode` / `storage_mode` / `ts_ms` are expected to differ
# because they are per-run metadata, not per-execution semantics.
IGNORED_COLS = {"run_id", "exec_mode", "storage_mode", "ts_ms"}

# Domain-family column groups. Each group maps to one of the 4
# classification categories from close_loop.verification.md §6.4 #5.
RESULT_CODE_COLUMNS = {"is_success", "result_code", "runtime_error"}
ENERGY_COLUMNS = {"energy_used"}
RETURN_DATA_COLUMNS = {"return_data_hex", "return_data_len"}
# `bandwidth_used` (S10 in close_loop.sidecar_parity.md) is a
# resource-counter field that belongs to the same "the execution
# consumed a different amount of a resource" family as `energy_used`.
# Section 6.4 #5 only defines `energy only` at the spec level, so we
# fold bandwidth into the same category here. If the spec ever grows
# a separate `bandwidth only` tag, split this out.
ENERGY_FAMILY_COLUMNS = ENERGY_COLUMNS | {"bandwidth_used"}

STATE_DIGEST_COLUMNS = {
    "state_digest_sha256",
    "state_change_count",
    "account_digest_sha256",
    "account_change_count",
    "evm_storage_digest_sha256",
    "evm_storage_change_count",
    "trc10_balance_digest_sha256",
    "trc10_balance_change_count",
    "trc10_issuance_digest_sha256",
    "trc10_issuance_change_count",
    "vote_digest_sha256",
    "vote_change_count",
    "freeze_digest_sha256",
    "freeze_change_count",
    "global_resource_digest_sha256",
    "global_resource_change_count",
    "account_resource_usage_digest_sha256",
    "account_resource_usage_change_count",
    "log_entries_digest_sha256",
    "log_entry_count",
}

# The `*_json` columns are excluded from category-level classification
# because the digest columns carry the same signal at O(1) cost; the
# JSON columns are only used for human-readable context when a mismatch
# is printed. Keeping them out of every comparison family means a human
# who happens to reformat JSON whitespace doesn't trip classification.
JSON_CONTEXT_COLUMNS = {
    "state_changes_json",
    "account_changes_json",
    "evm_storage_changes_json",
    "trc10_balance_changes_json",
    "trc10_issuance_changes_json",
    "vote_changes_json",
    "freeze_changes_json",
    "global_resource_changes_json",
    "account_resource_usage_changes_json",
    "log_entries_json",
}

# Columns excluded from any comparison at all. This is the union of
# the run-metadata columns and the JSON context columns.
COMPARE_EXCLUDED = IGNORED_COLS | JSON_CONTEXT_COLUMNS

# Classification categories. This is the frozen 4-category contract
# from `close_loop.verification.md` §6.4 #5. No additional public tags
# exist; any column that doesn't fit the three "only" families below
# is treated as the most-serious state-change / sidecar category so
# unknown columns default to FAIL, not silent pass. (Missing a
# regression is worse than a false positive.)
CATEGORY_STATE_CHANGE = "state-change / sidecar difference"
CATEGORY_RESULT_CODE = "result-code only"
CATEGORY_ENERGY = "energy only"
CATEGORY_RETURN_DATA = "return-data only"

# Severity order for picking the "most serious" category on a row
# where multiple families differ. The spec explicitly marks
# state-change as the most serious; the ordering among the three
# "only" categories is internal policy, not frozen spec — it affects
# the display label on rows that trip multiple families but does not
# change the overall pass/fail signal.
CATEGORY_SEVERITY = [
    CATEGORY_STATE_CHANGE,
    CATEGORY_RESULT_CODE,
    CATEGORY_ENERGY,
    CATEGORY_RETURN_DATA,
]


@dataclass
class RowMismatch:
    """A single row-level mismatch."""

    row_index: int  # 0-based data row index (i.e. file line number = idx + 2)
    block: str
    tx_id: str
    category: str
    differing_columns: List[Tuple[str, str, str]] = field(default_factory=list)
    # list of (column_name, embedded_value, remote_value)

    def to_dict(self) -> Dict[str, object]:
        return {
            "row_index": self.row_index,
            "block": self.block,
            "tx_id": self.tx_id,
            "category": self.category,
            "differing_columns": [
                {"column": c, "embedded": v1, "remote": v2}
                for (c, v1, v2) in self.differing_columns
            ],
        }


class EmptyCsvError(ValueError):
    """Raised when an input CSV is zero-byte or has no header row.

    A broken artifact-collection step (e.g. a runner crashed before
    writing any output) produces zero-byte CSVs. Treating those as
    "no mismatches" would silently pass a totally broken run, which
    is the opposite of what verification is for. Reject explicitly
    and let `main()` translate it into a clean exit code.
    """


def load_csv(path: str) -> Tuple[List[str], List[List[str]]]:
    with open(path, newline="") as f:
        reader = csv.reader(f)
        rows = list(reader)
    # `not rows` catches zero-byte files. A file containing just "\n"
    # parses as `[[]]` — a single row with no fields — which would
    # otherwise sneak through as `header=[]` and trip a false
    # "Header mismatch" or a false "no mismatches" depending on the
    # other side. Reject anything that doesn't have a real header row.
    if not rows or not rows[0]:
        raise EmptyCsvError(path)
    header = rows[0]
    data = rows[1:]
    return header, data


def _indices_for(header: List[str], names: Iterable[str]) -> List[int]:
    return [i for i, name in enumerate(header) if name in names]


def _classify_row(
    header: List[str],
    compare_idx: List[int],
    r1: List[str],
    r2: List[str],
) -> Tuple[Optional[str], List[Tuple[str, str, str]]]:
    """Return (category or None, [(col, v1, v2), ...]) for a single row.

    `None` means the row matches on every compared column.
    """
    diffs: List[Tuple[str, str, str]] = []
    families_hit: set = set()

    for j in compare_idx:
        v1 = r1[j] if j < len(r1) else ""
        v2 = r2[j] if j < len(r2) else ""
        if v1 == v2:
            continue
        col = header[j]
        diffs.append((col, v1, v2))

        if col in STATE_DIGEST_COLUMNS:
            families_hit.add(CATEGORY_STATE_CHANGE)
        elif col in RESULT_CODE_COLUMNS:
            families_hit.add(CATEGORY_RESULT_CODE)
        elif col in ENERGY_FAMILY_COLUMNS:
            families_hit.add(CATEGORY_ENERGY)
        elif col in RETURN_DATA_COLUMNS:
            families_hit.add(CATEGORY_RETURN_DATA)
        else:
            # Unknown column. Default to the most-serious category so
            # a newly-added column cannot silently pass a regression.
            # This matches the Section 6.4 #5 "state-change / sidecar
            # difference" semantic for "anything that looks like a
            # real state divergence".
            families_hit.add(CATEGORY_STATE_CHANGE)

    if not families_hit:
        return None, diffs

    # Pick the most serious category that appears in this row.
    for cat in CATEGORY_SEVERITY:
        if cat in families_hit:
            return cat, diffs
    # Unreachable: families_hit is non-empty and every category we add
    # to it is in CATEGORY_SEVERITY. Fall back to state-change rather
    # than a removed tag if that invariant is ever broken.
    return CATEGORY_STATE_CHANGE, diffs


def walk_mismatches(
    header: List[str],
    d1: List[List[str]],
    d2: List[List[str]],
) -> List[RowMismatch]:
    """Walk every row and return all classified mismatches.

    Rows past the common prefix are not compared; row-count mismatches
    are reported by the caller after this function returns.
    """
    compare_idx = [
        i for i, name in enumerate(header) if name not in COMPARE_EXCLUDED
    ]
    try:
        txid_col = header.index("tx_id_hex")
    except ValueError:
        txid_col = None
    try:
        blk_col = header.index("block_num")
    except ValueError:
        blk_col = None

    mismatches: List[RowMismatch] = []
    n = min(len(d1), len(d2))
    for i in range(n):
        r1, r2 = d1[i], d2[i]
        category, diffs = _classify_row(header, compare_idx, r1, r2)
        if category is None:
            continue
        tx_id = r1[txid_col] if (txid_col is not None and txid_col < len(r1)) else "?"
        block = r1[blk_col] if (blk_col is not None and blk_col < len(r1)) else "?"
        mismatches.append(
            RowMismatch(
                row_index=i,
                block=block,
                tx_id=tx_id,
                category=category,
                differing_columns=diffs,
            )
        )
    return mismatches


def print_first_mismatch(
    header: List[str],
    d1: List[List[str]],
    d2: List[List[str]],
    first: RowMismatch,
) -> None:
    """Print the legacy first-mismatch context block."""
    print(
        f"First mismatch at row {first.row_index + 2} (1-based), "
        f"block {first.block}, tx {first.tx_id}"
    )
    print(f"  classification: {first.category}")
    for col, v1, v2 in first.differing_columns:
        print(f"  - {col}:")
        print(f"    embedded: {v1}")
        print(f"    remote  : {v2}")

    # Pull the raw JSON context columns for any differing digest family.
    # Printing the full JSON is too noisy; truncate to 512 chars and only
    # include families that actually differ.
    differing_cols = {c for (c, _v1, _v2) in first.differing_columns}
    json_pairs = [
        ("state_change_count", "state_changes_json"),
        ("account_change_count", "account_changes_json"),
        ("evm_storage_change_count", "evm_storage_changes_json"),
        ("trc10_balance_change_count", "trc10_balance_changes_json"),
        ("trc10_issuance_change_count", "trc10_issuance_changes_json"),
        ("vote_change_count", "vote_changes_json"),
        ("freeze_change_count", "freeze_changes_json"),
        ("global_resource_change_count", "global_resource_changes_json"),
        ("account_resource_usage_change_count", "account_resource_usage_changes_json"),
        ("log_entry_count", "log_entries_json"),
    ]
    r1, r2 = d1[first.row_index], d2[first.row_index]

    def trunc(s: str) -> str:
        return (s[:512] + "...") if len(s) > 512 else s

    for count_col, json_col in json_pairs:
        if count_col not in differing_cols:
            continue
        try:
            j = header.index(json_col)
        except ValueError:
            continue
        sc1 = r1[j] if j < len(r1) else ""
        sc2 = r2[j] if j < len(r2) else ""
        print(f"\n{json_col} (truncated to 512 chars):")
        print("  embedded:", trunc(sc1))
        print("  remote  :", trunc(sc2))


def print_summary(mismatches: List[RowMismatch], total_rows: int) -> None:
    """Print a per-category summary table and the per-tx list."""
    if not mismatches:
        print(f"No mismatches found across {total_rows} compared rows.")
        return

    per_category: Dict[str, int] = {}
    for m in mismatches:
        per_category[m.category] = per_category.get(m.category, 0) + 1

    print(f"Mismatches: {len(mismatches)} / {total_rows} rows")
    print("By category (most serious first):")
    for cat in CATEGORY_SEVERITY:
        n = per_category.get(cat, 0)
        if n == 0:
            continue
        print(f"  {cat:40}  {n}")

    # Per-tx details — compact, one line per mismatch.
    print("\nPer-tx mismatches:")
    for m in mismatches:
        cols = ",".join(c for (c, _v1, _v2) in m.differing_columns)
        print(
            f"  row={m.row_index + 2:>6} block={m.block} tx={m.tx_id} "
            f"category=\"{m.category}\" columns=[{cols}]"
        )


def emit_json(
    mismatches: List[RowMismatch],
    total_rows: int,
    embedded_path: str,
    remote_path: str,
    row_count_mismatch: Optional[Tuple[int, int]],
) -> None:
    per_category: Dict[str, int] = {}
    for m in mismatches:
        per_category[m.category] = per_category.get(m.category, 0) + 1
    report = {
        "embedded_path": embedded_path,
        "remote_path": remote_path,
        "rows_compared": total_rows,
        "mismatch_count": len(mismatches),
        "row_count_mismatch": {
            "embedded": row_count_mismatch[0],
            "remote": row_count_mismatch[1],
        }
        if row_count_mismatch
        else None,
        "per_category": per_category,
        "mismatches": [m.to_dict() for m in mismatches],
    }
    json.dump(report, sys.stdout, indent=2, sort_keys=True)
    sys.stdout.write("\n")


def _parse_args(argv: List[str]) -> Tuple[str, str, bool, bool]:
    """Return (embedded_path, remote_path, classify_all, json_mode)."""
    classify_all = False
    json_mode = False
    positional: List[str] = []
    for arg in argv[1:]:
        if arg == "--classify-all":
            classify_all = True
        elif arg == "--json":
            classify_all = True
            json_mode = True
        elif arg in ("-h", "--help"):
            print(__doc__)
            sys.exit(0)
        elif arg.startswith("--"):
            print(f"Unknown flag: {arg}", file=sys.stderr)
            sys.exit(2)
        else:
            positional.append(arg)
    if len(positional) != 2:
        print(
            "Usage: compare_exec_csv.py [--classify-all|--json] "
            "<embedded.csv> <remote.csv>",
            file=sys.stderr,
        )
        sys.exit(2)
    return positional[0], positional[1], classify_all, json_mode


def main() -> None:
    p1, p2, classify_all, json_mode = _parse_args(sys.argv)

    # Guard against missing files up front so CI gets a clean
    # message instead of a raw Python traceback.
    import os as _os
    for path in (p1, p2):
        if not _os.path.exists(path):
            print(
                f"compare_exec_csv.py: file not found: {path}",
                file=sys.stderr,
            )
            sys.exit(2)

    try:
        h1, d1 = load_csv(p1)
        h2, d2 = load_csv(p2)
    except EmptyCsvError as e:
        print(
            f"compare_exec_csv.py: empty or headerless CSV: {e}",
            file=sys.stderr,
        )
        sys.exit(2)

    if h1 != h2:
        if json_mode:
            json.dump(
                {
                    "embedded_path": p1,
                    "remote_path": p2,
                    "header_mismatch": True,
                    "embedded_header": h1,
                    "remote_header": h2,
                },
                sys.stdout,
                indent=2,
                sort_keys=True,
            )
            sys.stdout.write("\n")
        else:
            print("Header mismatch")
            print("- embedded:", h1)
            print("- remote  :", h2)
        sys.exit(1)

    mismatches = walk_mismatches(h1, d1, d2)
    n = min(len(d1), len(d2))
    row_count_mismatch: Optional[Tuple[int, int]] = (
        (len(d1), len(d2)) if len(d1) != len(d2) else None
    )

    if json_mode:
        emit_json(mismatches, n, p1, p2, row_count_mismatch)
        sys.exit(1 if mismatches or row_count_mismatch else 0)

    if classify_all:
        print_summary(mismatches, n)
        if row_count_mismatch:
            print(
                f"\nRow count differs: embedded={row_count_mismatch[0]} "
                f"remote={row_count_mismatch[1]}"
            )
        sys.exit(1 if mismatches or row_count_mismatch else 0)

    # Legacy default: print the first mismatch with full context and
    # exit 0. This is intentional — `collect_remote_results.sh` runs
    # with `set -e` and invokes this script directly, so a non-zero
    # exit would abort the wrapper at step 11 and swallow the
    # follow-up debug output the wrapper prints afterward. Callers
    # that want a non-zero exit on mismatch must use
    # `--classify-all` or `--json`, which are the CI-shaped modes
    # added in iter 12.
    if mismatches:
        print_first_mismatch(h1, d1, d2, mismatches[0])
        # Also print a single-line category tag to stderr so a script
        # can grep it without parsing the full context block.
        print(f"classification={mismatches[0].category}", file=sys.stderr)
        sys.exit(0)

    if row_count_mismatch:
        print(
            f"No mismatches found in compared fields for the common prefix ({n} data rows)."
        )
        print(
            f"Row count differs: embedded={row_count_mismatch[0]} "
            f"remote={row_count_mismatch[1]}"
        )
        sys.exit(0)

    print(
        "No mismatches found in compared fields "
        "(ignoring run_id, exec_mode, storage_mode, ts_ms, *_json context columns)"
    )
    sys.exit(0)


if __name__ == "__main__":
    main()
