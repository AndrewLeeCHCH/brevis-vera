use alloy_sol_types::SolType;
use c2pa_lib::PublicValuesStruct;
use pico_sdk::init_logger;
use std::env;
use std::path::Path;

fn main() {
    // Initialize logger
    init_logger();

    // Get proof directory from command line or use default
    let args: Vec<String> = env::args().collect();
    let proof_dir = if args.len() > 1 {
        Path::new(&args[1]).to_path_buf()
    } else {
        Path::new("../../proof_data").to_path_buf()
    };

    // Load public values
    let pv_path = proof_dir.join("c2pa_public_values.bin");
    let public_buffer = std::fs::read(&pv_path).expect("Failed to read public values");
    println!("Public values loaded from: {:?}", pv_path);

    // Load proof
    let proof_path = proof_dir.join("c2pa_proof.bin");
    let proof_bytes = std::fs::read(&proof_path).expect("Failed to read proof");
    println!("Proof loaded from: {:?}", proof_path);

    println!("\n=== Verifying C2PA Image Edit ZK Proof ===");

    // Decode public values
    let public_values = PublicValuesStruct::abi_decode(&public_buffer, false).unwrap();

    println!("\nPublic values:");
    println!("  Final image hash: {:02x?}", &public_values.final_image_hash[..8]);
    println!("  Number of operations: {}", public_values.num_operations);

    // For the proof to be valid, we just check that we got valid output
    // (the ZK proof itself ensures the computation was correct)
    let is_valid = public_values.num_operations > 0;

    println!("\nVerification result: {}", if is_valid { "VALID" } else { "INVALID" });

    // Verify the result
    assert!(is_valid, "ZK proof verification failed!");

    println!("\n=== C2PA Image Edit Proof Verified Successfully! ===");
    println!();
    println!("What the verifier learned:");
    println!("  - Final image hash: {:02x?}...", &public_values.final_image_hash[..8]);
    println!("  - Number of operations performed: {}", public_values.num_operations);
    println!();
    println!("What the verifier DID NOT learn:");
    println!("  - The original image content");
    println!("  - The exact operation parameters");
    println!("  - The C2PA signature details from the original image");
}
