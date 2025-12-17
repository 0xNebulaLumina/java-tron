fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Compile TRON proto files to generate Rust code
    // These protos match java-tron's protocol definitions exactly for wire-level compatibility
    prost_build::Config::new()
        .compile_protos(
            &[
                "protos/witness.proto",  // Legacy witness proto (kept for backward compatibility)
                "protos/tron.proto",     // Comprehensive TRON types (Account, Proposal, etc.)
            ],
            &["protos/"],
        )?;

    Ok(())
}
