// Standalone test to verify witness protobuf encoding/decoding
// Run with: rustc --edition 2021 test_witness_protobuf.rs && ./test_witness_protobuf

use std::process::Command;

fn main() {
    println!("Testing witness protobuf implementation...\n");

    // Run the witness protobuf tests
    let output = Command::new("cargo")
        .args(&[
            "test",
            "-p",
            "tron-backend-execution",
            "--lib",
            "test_witness_protobuf_encode_decode",
            "--",
            "--exact",
            "--nocapture"
        ])
        .current_dir("/root/gitfiles/0xNebulaLumina/java-tron/rust-backend")
        .output()
        .expect("Failed to run tests");

    println!("STDOUT:\n{}", String::from_utf8_lossy(&output.stdout));
    println!("STDERR:\n{}", String::from_utf8_lossy(&output.stderr));

    if output.status.success() {
        println!("\n✓ Test passed!");
    } else {
        println!("\n✗ Test failed");
        std::process::exit(1);
    }
}
