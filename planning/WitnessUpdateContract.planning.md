• I compared the two CSVs, then traced mismatches through the Java and Rust logs.

What I Compared

- Files: output-directory/execution-csv/20251117-102033-01aa61f2-embedded-embedded.csv vs output-directory/execution-csv/20251123-090003-68fed2e4-remote-remote.csv
- Common transactions by tx_id_hex: 1287
- Mismatches found: 3

Mismatches Found

- All 3 are WitnessUpdateContract and differ only in is_success/result_code:
    - bc3284e33345d681a9b49bde07b878a45c9ea01172213ec06407d5d3d67e8467 (block 4386, idx 1): embedded=SUCCESS, remote=REVERT
    - f7670dea96c3c1094af18a9395c5f652ff4959c5c706d578f4f32a15ef222051 (block 4564, idx 0): embedded=SUCCESS, remote=REVERT
    - a3b4b0ed4979a742bc944226c322c744e573cd82466119b08d1258288a1e72f3 (block 4649, idx 0): embedded=SUCCESS, remote=REVERT
- For all 3: energy_used=0, state_change_count=0, state_digest_sha256=e3b0c442... (empty digest) in both runs.

Root Cause

- The Rust backend has witness_update_enabled = false, so it rejects WitnessUpdateContract (marks as REVERT), even though embedded Java treats these as successful no-ops.
    - Config flag: rust-backend/config.toml:69
    - Confirmed in backend startup: remote-rust.a14322a.log:2 shows witness_update_enabled: false
    - Runtime errors for these txs:
        - remote-rust.a14322a.log:21177 Non-VM contract execution failed: WITNESS_UPDATE_CONTRACT execution is disabled - falling back to Java
        - remote-rust.a14322a.log:21412 same for f7670...
        - remote-rust.a14322a.log:21441 same for a3b4b0...
- The Java CSV for the remote run still records REVERT, i.e., it does not reflect a successful fallback result for these system contracts.

---

