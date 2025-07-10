use std::env;
use std::path::PathBuf;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let out_dir = PathBuf::from(env::var("OUT_DIR").unwrap());
    
    // Get the workspace root directory (rust-backend crate is in rust-backend/crates/core)
    let workspace_root = env::var("CARGO_MANIFEST_DIR")
        .map(PathBuf::from)
        .unwrap()
        .parent()
        .unwrap()
        .parent()
        .unwrap()
        .parent()
        .unwrap()
        .to_path_buf();
    
    let proto_dir = workspace_root.join("proto");
    let proto_file = proto_dir.join("backend.proto");
    
    // Compile the unified backend proto
    tonic_build::configure()
        .file_descriptor_set_path(out_dir.join("backend_descriptor.bin"))
        .compile(&[proto_file.clone()], &[proto_dir.clone()])?;
    
    println!("cargo:rerun-if-changed={}", proto_file.display());
    println!("cargo:rerun-if-changed={}", proto_dir.display());
    
    Ok(())
} 