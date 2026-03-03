#![no_main]

use alloy_sol_types::private::FixedBytes;
use alloy_sol_types::SolValue;
use c2pa_app_lib::{
    apply_operations_and_hash, verify_signature, C2PAMetadata, ImageData, PublicValuesStruct,
};
use pico_sdk::io::{commit_bytes, read_as, read_vec};

pico_sdk::entrypoint!(main);

pub fn main() {
    // =========================================================================
    // Private Inputs:
    // 1. C2PA metadata (original image with valid signature)
    // 2. Original image dimensions and pixel data
    // 3. List of operations (type + params)
    // 4. Final image hash (from prover) - for comparison
    // =========================================================================

    // Read has_manifest flag
    let has_manifest: u8 = read_as();
    let has_c2pa = has_manifest != 0;

    // Read cert_chain_verified flag
    let cert_verified: u8 = read_as();
    let cert_chain = if cert_verified != 0 {
        Some(vec![])
    } else {
        None
    };

    if !has_c2pa {
        let result_struct = PublicValuesStruct {
            final_image_hash: FixedBytes([0u8; 32]),
            num_operations: 0,
        };
        let encoded_bytes = result_struct.abi_encode();
        commit_bytes(&encoded_bytes);
        return;
    }

    // Read C2PA metadata
    let issuer_vec = read_vec();
    let mut issuer = [0u8; 32];
    issuer.copy_from_slice(&issuer_vec[..32.min(issuer_vec.len())]);

    let timestamp: u64 = read_as();

    let sig_vec = read_vec();
    let mut signature = [0u8; 64];
    signature.copy_from_slice(&sig_vec[..64.min(sig_vec.len())]);

    // This is the original image hash from C2PA - it's part of the signed claim
    let claim_hash_vec = read_vec();
    let mut original_image_hash = [0u8; 32];
    original_image_hash.copy_from_slice(&claim_hash_vec[..32.min(claim_hash_vec.len())]);

    let metadata = C2PAMetadata {
        issuer,
        timestamp,
        signature,
        claim_hash: original_image_hash,
        certificate_chain: cert_chain,
    };

    // Verify C2PA signature
    let is_valid = verify_signature(&metadata);
    if !is_valid {
        let result_struct = PublicValuesStruct {
            final_image_hash: FixedBytes([0u8; 32]),
            num_operations: 0,
        };
        let encoded_bytes = result_struct.abi_encode();
        commit_bytes(&encoded_bytes);
        return;
    }

    // =========================================================================
    // Read original image data
    // =========================================================================
    let img_width: u32 = read_as();
    let img_height: u32 = read_as();
    let pixel_data = read_vec();

    println!("Received image: {}x{} ({} pixels)", img_width, img_height, pixel_data.len());

    // Create image data structure
    let image_data = ImageData {
        width: img_width,
        height: img_height,
        pixels: pixel_data,
    };

    // =========================================================================
    // Read and apply operations
    // =========================================================================
    let num_operations: u32 = read_as();

    // Read operations
    let mut operations: Vec<(u8, Vec<u8>)> = Vec::with_capacity(num_operations as usize);
    for _ in 0..num_operations {
        let op_type: u8 = read_as();
        let params_vec = read_vec();
        operations.push((op_type, params_vec));
    }

    println!("Applying {} operations in zkVM...", num_operations);

    // Apply operations and compute hash
    let computed_hash = apply_operations_and_hash(
        &image_data,
        &operations.iter().map(|(t, p)| (*t, p.as_slice())).collect::<Vec<_>>(),
    );

    // Read final image hash from prover (for comparison, optional)
    let _final_hash_vec = read_vec();
    let _prover_hash: [u8; 32] = if _final_hash_vec.len() >= 32 {
        let mut h = [0u8; 32];
        h.copy_from_slice(&_final_hash_vec[..32]);
        h
    } else {
        [0u8; 32]
    };

    // =========================================================================
    // Public Outputs
    // The final hash is computed from actual image operations in zkVM
    // =========================================================================

    println!("Computed final image hash: {:02x?}", &computed_hash[..8]);

    let result_struct = PublicValuesStruct {
        final_image_hash: FixedBytes(computed_hash),
        num_operations,
    };
    let encoded_bytes = result_struct.abi_encode();

    commit_bytes(&encoded_bytes);
}
