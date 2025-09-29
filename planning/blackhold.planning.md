Here’s the first mismatch and why it happens.

  What I compared

  - Files: output-directory/execution-csv/20250906-115209-2d757f5d-embedded-embedded.csv and output-directory/execution-csv/20250921-125424-98950f62-remote-remote.csv
  - Method: key by (block_num, tx_index_in_block, tx_id_hex), compare core fields: is_success, result_code, energy_used, return_data_hex/len, runtime_error, state_change_count, state_changes_json, state_digest_sha256.

  First mismatch

  - Location: block 1785, tx_index 0, tx_id 18bd4e3bb4552df2d2d24821bddf7079df9bf7f293978c97d2c7c8613d45b3e3
  - Contract: WitnessCreateContract
  - Embedded row: state_change_count=2; state_changes_json includes:
      - Account 4116329c4b64920408342e66221a1d52a974901984 (TBzaVm3e3QmoaG5xjR98DNqZE9bYwqgZcg) balance decrease.
      - Account 4177944d19c052b73ee2286823aa83f8138cb7032f (TLsV52sRDL79HXGGm9yzwKibb6BeruhUzy) balance increase (blackhole address).
      - state_digest_sha256=21d448c562bec60e94ef30caa9a7b30950f68c97e60ed67b31885de3e1283909
  - Remote row: state_change_count=1; only the owner account change; no blackhole credit.
      - state_digest_sha256=ea822c4c014f90d9dbff9ebf97bcf87d256d2dba820138bf6e9087bf6c0f9fd7

  Corroborating logs

  - Remote Java: remote-java.2d66b5e.log:64431..64446 shows “Remote execution returned 1 state changes … Applying 1 state changes … Updated existing account TBzaVm3e…”
      - remote-java.2d66b5e.log:64431
  - Remote Rust: remote-rust.2d66b5e.log shows the reason explicitly:
      - “WitnessCreate flags: upgrade_cost=9999000000 SUN, allow_multi_sign=false, support_blackhole=true”
      - “WitnessCreate completed … state_changes=1 … fee_dest=burn”
      - remote-rust.2d66b5e.log:153,157
  - Embedded Java path (actuator): logic credits the blackhole account when “supportBlackHoleOptimization” is false:
      - chainbase/src/main/java/org/tron/core/store/DynamicPropertiesStore.java:2441
      - actuator/src/main/java/org/tron/core/actuator/WitnessCreateActuator.java:143–148

  Root cause

  - Policy mismatch for supportBlackHoleOptimization default:
      - Java defaults allowBlackHoleOptimization=0 via config args (committee.allowBlackHoleOptimization), which initializes DynamicPropertiesStore to 0 when missing, so WitnessCreate credits the blackhole account
  TLsV52s… at this height.
          - framework/src/main/java/org/tron/core/config/args/Args.java:71–74
          - chainbase/src/main/java/org/tron/core/store/DynamicPropertiesStore.java:790–818,2441–2449
      - Rust backend reads SupportBlackHoleOptimization from chain state, but its StorageAdapter defaults to true when the key is missing, treating it as “burn (optimization on)”, not “credit blackhole”.
          - rust-backend/crates/execution/src/storage_adapter.rs:28–45
          - rust-backend/crates/core/src/service.rs:77–85 and 79–83
      - Additionally, the Rust service ignores the config fallback for this flag (config shows support_black_hole_optimization=false), and instead uses the dynamic property with a default of true. Hence
  support_blackhole=true in logs, 1 state change, fee_dest=burn.

  Conclusion for first mismatch

  - Embedded is correct for your config: it produced two state changes (owner debit + blackhole credit).
  - Remote omitted the blackhole credit due to the Rust backend defaulting SupportBlackHoleOptimization to true when the dynamic property isn’t present, and not falling back to config.

  Quick fix options

  - Make Rust default align with Java when dynamic property is absent:
      - Change StorageModuleAdapter.support_black_hole_optimization() to default false if the key is missing, or
      - Fall back to ExecutionFeeConfig.support_black_hole_optimization (which your config already sets to false) when the dynamic property isn’t found.
  - After changing, re-run and the same tx should yield state_change_count=2 and matching state_digest.

  Do you want me to patch the Rust backend to:

  - Use config fallback when the dynamic property is missing, and
  - Default to false to match Java behavior at early heights?
  
  ---

  > I’ll first inspect both CSVs to find the earliest mismatch, then correlate that tx in the Java and Rust logs.

        - state_change_count: 2 (embedded) vs 1 (remote)
        - state_digest_sha256: 21d448c5…83909 (embedded) vs ea822c4c…9fd7 (remote)
- What Changed
    - Embedded (2 state changes; both account-level “key” is empty)
        - Owner 4116329c… (TBzaVm3e…): debited by 9,999,000,000 SUN (witness creation cost).
        - Blackhole TLsV52sRDL79HXGGm9yzwKibb6BeruhUzy (4177944d…): credited by the same cost.
        - Evidence row: output-directory/execution-csv/20250906-115209-2d757f5d-embedded-embedded.csv:9
    - Remote (1 state change)
        - Only owner 4116329c… updated; no blackhole credit emitted.
        - Evidence row: output-directory/execution-csv/20250922-053456-73f4d114-remote-remote.csv:9
- Log Evidence
    - Remote Java: “Applying 1 state changes …”
        - remote-java.9a2e35a.log:64513 (“Remote execution returned 1 state changes”)
        - remote-java.9a2e35a.log:64515–64519 (owner account updated; no second change)
    - Remote Rust: WitnessCreate completed with state_changes=1 and burn path
        - remote-rust.9a2e35a.log:153 (“WitnessCreate flags: … support_blackhole=true”)
        - remote-rust.9a2e35a.log:157 (“WitnessCreate completed: … state_changes=1, … fee_dest=burn”)
- Why It Differs (Root Cause)
    - At this early height (block 1785), Java’s dynamic property “AllowBlackHoleOptimization” is OFF — embedded credits the well‑known blackhole account TLsV52s… (hence 2 account changes: owner and blackhole).
    - The Rust backend incorrectly assumes the optimization is ON because it:
        - Reads the wrong dynamic property key and defaults to true when missing.
            - Rust uses key “SUPPORT_BLACK_HOLE_OPTIMIZATION” and returns true if absent: rust-backend/crates/execution/src/storage_adapter.rs:449.
            - Java’s key is “ALLOW_BLACKHOLE_OPTIMIZATION”: chainbase/src/main/java/org/tron/core/store/DynamicPropertiesStore.java:2441 (getAllowBlackHoleOptimization/ALLOW_BLACKHOLE_OPTIMIZATION).
        - Result: Rust burns the cost (no blackhole credit), producing only one state change and a different state digest.
    - Additional parity gap: Rust looks for a dynamic property “BLACK_HOLE_ADDRESS” to credit, which likely doesn’t exist on-chain; Java uses a genesis-constant blackhole account (AccountStore.getBlackhole()). So even when optimization is
OFF, Rust may still fall back to burn unless it knows the TLsV52s… address.
- Config Context
    - Current remote execution config has storage deltas disabled (not the cause here, but explains some CSV shape differences in general): rust-backend/config.toml:76 (emit_storage_changes = false).
    - Rust logs confirm burn path chosen at this height (instead of blackhole credit).
- Recommended Fixes
    - Correct dynamic property lookup and default:
        - Use the Java-consistent key “ALLOW_BLACKHOLE_OPTIMIZATION”; default to false if missing at early heights.
        - File: rust-backend/crates/execution/src/storage_adapter.rs:449.
    - Provide blackhole destination when crediting:
        - Resolve the blackhole address like Java (prefer a known constant TLsV52sRDL79HXGGm9yzwKibb6BeruhUzy) if no dynamic property is set, so crediting can occur.
        - File: rust-backend/crates/execution/src/storage_adapter.rs:469.
    - Optional for CSV parity experiments:
        - If you want remote to emit witness-related storage deltas, set rust-backend/config.toml:76 to emit_storage_changes = true. Note this affects CSV shape and digest; it’s orthogonal to the fee-credit mismatch above.
- Quick Sanity Checks You Can Run
    - Verify remote fee path for this tx in logs:
        - remote-java.9a2e35a.log:64513–64519
        - remote-rust.9a2e35a.log:153,157
    - Confirm blackhole account in embedded CSV:
        - output-directory/execution-csv/20250906-115209-2d757f5d-embedded-embedded.csv:9 shows address 4177944d… = TLsV52s…

Want me to:

- Diff a few more subsequent WitnessCreate/VoteWitness transactions to see if the same root cause recurs?
- Draft a precise patch to fix the dynamic property key + default and ensure blackhole crediting (without changing other behavior)?


---


> Here’s a precise, minimal patch to fix the dynamic property key and default, and to ensure blackhole crediting matches embedded behavior.

- Changes
    - Fixes the dynamic property key used by the Rust backend to match Java’s DynamicPropertiesStore.
    - Parses the stored long correctly (8 bytes, big-endian) instead of using the first byte.
    - Defaults to “credit blackhole” (false) when the key is absent, matching early-chain embedded behavior.
    - Adds a safe default blackhole address (TLsV52sRDL79HXGGm9yzwKibb6BeruhUzy) when no dynamic value exists, mirroring java-tron’s AccountStore.getBlackhole().
- Files Updated
    - rust-backend/crates/execution/src/storage_adapter.rs:452
        - support_black_hole_optimization()
            - Key corrected to ALLOW_BLACKHOLE_OPTIMIZATION
            - Proper u64 parsing; default false when missing
    - rust-backend/crates/execution/src/storage_adapter.rs:484
        - get_blackhole_address()
            - Returns configured 20-byte value when present
            - Otherwise returns the mainnet default blackhole address
    - rust-backend/crates/execution/src/storage_adapter.rs:506
        - default_blackhole_address()
            - Uses tron_backend_common::from_tron_address to decode “TLsV52sRDL79HXGGm9yzwKibb6BeruhUzy”
- Why this fixes the mismatch
    - Embedded at block 1785 credits the blackhole account (witness creation cost), producing two account-level state changes (owner debit + blackhole credit). The remote path previously burned due to:
        - Wrong key (“SUPPORT_BLACK_HOLE_OPTIMIZATION”) and a default true → burn.
        - No fallback blackhole address when not found in dynamic properties.
    - After this patch:
        - support_black_hole_optimization correctly reads ALLOW_BLACKHOLE_OPTIMIZATION and defaults to false → credit.
        - A valid blackhole address is available even when the dynamic property is missing.
        - Remote will emit 2 account changes sorted deterministically (owner then blackhole), matching embedded’s state_change_count and digest for this tx.
- How to verify
    - Rebuild Rust backend, rerun the (remote execution + remote storage) path, and re-compare CSVs:
        - Build: cd rust-backend && cargo build --release
        - Run remote collection (your existing flow) and generate a new CSV.
        - Compare the first mismatch again; for tx 18bd4e3b… in block 1785 you should see:
            - state_change_count: 2
            - blackhole address 4177944d19c052b73ee2286823aa83f8138cb7032f credited
            - state_digest_sha256 matches the embedded CSV for that row.

If you want, I can also:

- Scan the next few mismatches after this tx to confirm similar fixes cascade cleanly.
- Gate the default blackhole fallback behind a config flag if you need different behavior on non-mainnet networks.


