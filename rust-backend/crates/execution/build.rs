fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Compile TRON proto files to generate Rust code
    // These protos match java-tron's protocol definitions exactly for wire-level compatibility
    let mut config = prost_build::Config::new();
    // Conformance fixtures assert raw DB bytes. java-tron serializes protobuf map fields
    // deterministically (sorted by key), while prost's default `HashMap` encoding order is
    // non-deterministic. Use `BTreeMap` so encode order is stable and matches fixtures.
    config.btree_map(&[
        ".protocol.Proposal.parameters",
        ".protocol.Account.asset",
        ".protocol.Account.latest_asset_operation_time",
        ".protocol.Account.free_asset_net_usage",
        ".protocol.Account.assetV2",
        ".protocol.Account.latest_asset_operation_timeV2",
        ".protocol.Account.free_asset_net_usageV2",
        ".protocol.TransactionResult.cancel_unfreezeV2_amount",
    ]);
    config
        .compile_protos(
            &[
                "protos/witness.proto",  // Legacy witness proto (kept for backward compatibility)
                "protos/tron.proto",     // Comprehensive TRON types (Account, Proposal, etc.)
            ],
            &["protos/"],
        )?;

    Ok(())
}
