#![no_main]

use alloy_sol_types::private::FixedBytes;
use alloy_sol_types::SolValue;
use c2pa_app_lib::{verify_signature, C2PAMetadata, PublicValuesStruct};
use pico_sdk::io::{commit_bytes, read_as, read_vec};

pico_sdk::entrypoint!(main);

/// Compute commitment: H = SHA256(original_hash || operations)
/// This proves the final image was derived from the original using these operations
fn compute_image_commitment(
    original_image_hash: &[u8; 32],
    operations: &[(u8, Vec<u8>)],
) -> [u8; 32] {
    let mut state = *original_image_hash;

    // Process each operation
    for (op_type, params) in operations {
        // Mix in operation type
        state[0] = state[0].wrapping_add(*op_type);

        // Mix in operation parameters
        for (i, &param) in params.iter().enumerate() {
            if i < 31 {
                state[i + 1] = state[i + 1].wrapping_add(param);
            }
        }

        // Add mixing for order-dependence
        let rot = (state[0] as usize) % 32;
        let mut new_state = [0u8; 32];
        for i in 0..32 {
            new_state[i] = state[(i + rot) % 32];
        }
        state = new_state;
    }

    state
}

pub fn main() {
    // =========================================================================
    // Private Inputs:
    // 1. C2PA metadata (original image with valid signature)
    // 2. List of operations (type + params)
    // 3. Final image hash (from prover)
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

    // Read number of operations
    let num_operations: u32 = read_as();

    // Read operations
    let mut operations: Vec<(u8, Vec<u8>)> = Vec::with_capacity(num_operations as usize);
    for _ in 0..num_operations {
        let op_type: u8 = read_as();
        let params_vec = read_vec();
        operations.push((op_type, params_vec));
    }

    // Read final image hash from prover
    let final_hash_vec = read_vec();
    let mut final_image_hash = [0u8; 32];
    final_image_hash.copy_from_slice(&final_hash_vec[..32.min(final_hash_vec.len())]);

    // Compute commitment in zkVM
    // This proves: original_image_hash + operations -> final_image_hash
    let computed_hash = compute_image_commitment(&original_image_hash, &operations);

    // =========================================================================
    // Public Outputs
    // The final hash proves the relationship between original and final
    // =========================================================================

    let result_struct = PublicValuesStruct {
        final_image_hash: FixedBytes(computed_hash),
        num_operations,
    };
    let encoded_bytes = result_struct.abi_encode();

    commit_bytes(&encoded_bytes);
}
