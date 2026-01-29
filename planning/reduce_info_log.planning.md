• Why this is a “next likely win”                   
                                                                                                                                                                                                                  
  - In the current code, “sync” emits per-transaction INFO logs on the Rust backend; it does expensive formatting and even extra DB reads purely for
    logging.                                                                                                                                                                                                      
Rust-side code plan (biggest win is eliminating per-tx INFO + extra DB reads)

  - In the gRPC execution hot path, convert per-tx INFO to DEBUG and avoid doing work when not logging:
      - rust-backend/crates/core/src/service/grpc/mod.rs:1065 (buffered writes message) should log once at startup/config load, not per tx.
      - rust-backend/crates/core/src/service/grpc/mod.rs:1096 and rust-backend/crates/core/src/service/grpc/mod.rs:1212 (blackhole BEFORE/AFTER) are worse than “just logs”: they read from storage and do address
        conversions per tx. Put the entire block behind if tracing::enabled!(tracing::Level::DEBUG) { ... } (or a config flag) so those DB reads disappear during sync.
      - Demote rust-backend/crates/core/src/service/grpc/mod.rs:1107, rust-backend/crates/core/src/service/grpc/mod.rs:1111, rust-backend/crates/core/src/service/grpc/mod.rs:1222, rust-backend/crates/core/src/
        service/grpc/mod.rs:1236, rust-backend/crates/core/src/service/grpc/mod.rs:1248 to DEBUG, and replace with periodic INFO summary (tx/sec, avg energy, commit ops, etc.).
  - Remove state-change spam in the EVM layer:
      - rust-backend/crates/execution/src/tron_evm.rs:1059, rust-backend/crates/execution/src/tron_evm.rs:1124, and especially the per-change loop at rust-backend/crates/execution/src/tron_evm.rs:1128 / rust-
        backend/crates/execution/src/tron_evm.rs:1133 should be DEBUG/TRACE + sampling, not INFO.
  - Keep INFO logs for lifecycle only (startup, module init, config summary) and move high-volume execution internals to DEBUG.

  Step 4: Verify the win and keep it from regressing

  - Re-run the same fixed-height window in remote-remote:
      - Compare blocks/sec and tx/sec.
      - Compare log line counts and max log write throughput.
  - Put “no per-tx INFO in gRPC execute path” into a simple checklist for future PRs (this repo has a history of adding Phase-B debug at INFO).
