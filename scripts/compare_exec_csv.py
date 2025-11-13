#!/usr/bin/env python3
import csv
import sys
from typing import List, Tuple

IGNORED_COLS = {"run_id", "exec_mode", "storage_mode", "ts_ms"}

def load_csv(path: str) -> Tuple[List[str], List[List[str]]]:
    with open(path, newline='') as f:
        reader = csv.reader(f)
        rows = list(reader)
        header = rows[0]
        data = rows[1:]
    return header, data

def main():
    if len(sys.argv) != 3:
        print("Usage: compare_exec_csv.py <embedded.csv> <remote.csv>", file=sys.stderr)
        sys.exit(2)

    p1, p2 = sys.argv[1], sys.argv[2]
    h1, d1 = load_csv(p1)
    h2, d2 = load_csv(p2)

    if h1 != h2:
        print("Header mismatch")
        print("- embedded:", h1)
        print("- remote  :", h2)
        sys.exit(1)

    # Column indices to compare
    indices = [i for i, name in enumerate(h1) if name not in IGNORED_COLS]
    name_by_idx = {i: h1[i] for i in indices}

    n = min(len(d1), len(d2))
    for i in range(n):
        r1, r2 = d1[i], d2[i]
        # quick equality check on compared fields
        same = True
        for j in indices:
            v1 = r1[j]
            v2 = r2[j]
            if v1 != v2:
                same = False
                break
        if same:
            continue

        # Found first mismatch; print compact summary
        # Try to include tx id and block number
        try:
            txid_idx = h1.index("tx_id_hex")
        except ValueError:
            txid_idx = None
        try:
            blk_idx = h1.index("block_num")
        except ValueError:
            blk_idx = None

        txid = r1[txid_idx] if txid_idx is not None else "?"
        blk = r1[blk_idx] if blk_idx is not None else "?"
        print(f"First mismatch at row {i+2} (1-based), block {blk}, tx {txid}")

        # Print differing columns
        for j in indices:
            v1 = r1[j]
            v2 = r2[j]
            if v1 != v2:
                name = name_by_idx[j]
                print(f"  - {name}:")
                print(f"    embedded: {v1}")
                print(f"    remote  : {v2}")

        # Also print entire rows if state_change_count differs
        try:
            scc_idx = h1.index("state_change_count")
        except ValueError:
            scc_idx = None
        if scc_idx is not None and r1[scc_idx] != r2[scc_idx]:
            print("\nstate_changes_json (truncated to 512 chars):")
            try:
                scj_idx = h1.index("state_changes_json")
                sc1 = r1[scj_idx]
                sc2 = r2[scj_idx]
                def trunc(s):
                    return (s[:512] + '...') if len(s) > 512 else s
                print("  embedded:", trunc(sc1))
                print("  remote  :", trunc(sc2))
            except ValueError:
                pass
        sys.exit(0)

    # If we got here, either lengths differ or no mismatches in compared fields
    if len(d1) != len(d2):
        print(f"Row count differs: embedded={len(d1)} remote={len(d2)}")
        sys.exit(0)
    print("No mismatches found in compared fields (ignoring run_id, exec_mode, storage_mode, ts_ms)")

if __name__ == "__main__":
    main()

