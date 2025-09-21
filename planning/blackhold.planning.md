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
  