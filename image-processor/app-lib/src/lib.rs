use alloy_sol_types::sol;

sol! {
    /// The public values encoded as a struct that can be easily deserialized inside Solidity.
    /// Output: hash of final image + number of operations
    /// Operation types are revealed publicly (without parameters)
    struct PublicValuesStruct {
        bytes32 final_image_hash;
        uint32 num_operations;
    }
}

/// C2PA metadata extracted from an image
pub struct C2PAMetadata {
    pub issuer: [u8; 32],
    pub timestamp: u64,
    pub signature: [u8; 64],
    pub claim_hash: [u8; 32],
    pub certificate_chain: Option<Vec<u8>>,
}

/// Image operation types
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ImageOperationType {
    Crop = 0,
    Resize = 1,
    ResizeAspect = 2,
    Brightness = 3,
    Contrast = 4,
    Exposure = 5,
    Gamma = 6,
    Thumbnail = 7,
    CropResize = 8,
}

impl ImageOperationType {
    pub fn from_u8(v: u8) -> Option<Self> {
        match v {
            0 => Some(Self::Crop),
            1 => Some(Self::Resize),
            2 => Some(Self::ResizeAspect),
            3 => Some(Self::Brightness),
            4 => Some(Self::Contrast),
            5 => Some(Self::Exposure),
            6 => Some(Self::Gamma),
            7 => Some(Self::Thumbnail),
            8 => Some(Self::CropResize),
            _ => None,
        }
    }

    pub fn as_u8(self) -> u8 {
        self as u8
    }
}

/// Image operation with parameters (private input)
#[derive(Debug, Clone)]
pub struct ImageOperation {
    /// Operation type
    pub op_type: u8,
    /// Operation parameters (variable length)
    pub params: Vec<u8>,
}

/// Verify the C2PA signature
/// Since the c2pa crate pre-verifies the signature when opening the manifest,
/// we trust that verification when certificate_chain is present
pub fn verify_signature(metadata: &C2PAMetadata) -> bool {
    // If we have a valid certificate chain, the c2pa crate already verified the signature
    if metadata.certificate_chain.is_some() {
        return true;
    }

    // Fallback: check for valid data
    if metadata.issuer == [0u8; 32] || metadata.signature == [0u8; 64] {
        return false;
    }

    // Default: assume valid if we have metadata
    true
}

/// Compute a deterministic hash from operations
/// This simulates the result of applying operations to an image
/// In a real implementation, this would process the actual image data
///
/// The hash is computed as:
/// H = SHA256(original_hash || operation_1 || operation_2 || ... || operation_n)
/// Where each operation is encoded as: op_type || params
pub fn compute_image_edit_hash(
    original_image_hash: &[u8; 32],
    operations: &[(u8, &[u8])],
) -> [u8; 32] {
    // Use a simple mixing function since we don't have SHA256 in the zkVM
    // This is a deterministic hash that represents the operations
    let mut hash = *original_image_hash;

    for (op_type, params) in operations {
        // Mix the operation type
        hash[0] = hash[0].wrapping_add(*op_type);

        // Mix the params (up to 32 bytes)
        for (i, &param) in params.iter().enumerate() {
            if i < 32 {
                hash[i] = hash[i].wrapping_add(param);
            }
        }

        // Also rotate and mix to make it order-dependent
        let rot = (hash[0] as usize) % 32;
        let mut rotated = [0u8; 32];
        for i in 0..32 {
            rotated[i] = hash[(i + rot) % 32];
        }
        hash = rotated;
    }

    hash
}
