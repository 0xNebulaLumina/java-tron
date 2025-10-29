fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Compile witness.proto to generate Rust code
    prost_build::Config::new()
        .compile_protos(&["protos/witness.proto"], &["protos/"])?;

    Ok(())
}
