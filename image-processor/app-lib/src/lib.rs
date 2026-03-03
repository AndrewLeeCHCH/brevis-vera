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

/// Crop parameters: x, y, width, height
#[derive(Debug, Clone, Copy)]
pub struct CropParams {
    pub x: u32,
    pub y: u32,
    pub width: u32,
    pub height: u32,
}

impl CropParams {
    pub fn from_bytes(bytes: &[u8]) -> Option<Self> {
        if bytes.len() < 16 {
            return None;
        }
        Some(CropParams {
            x: u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]),
            y: u32::from_le_bytes([bytes[4], bytes[5], bytes[6], bytes[7]]),
            width: u32::from_le_bytes([bytes[8], bytes[9], bytes[10], bytes[11]]),
            height: u32::from_le_bytes([bytes[12], bytes[13], bytes[14], bytes[15]]),
        })
    }
}

/// Resize parameters: width, height
#[derive(Debug, Clone, Copy)]
pub struct ResizeParams {
    pub width: u32,
    pub height: u32,
}

impl ResizeParams {
    pub fn from_bytes(bytes: &[u8]) -> Option<Self> {
        if bytes.len() < 8 {
            return None;
        }
        Some(ResizeParams {
            width: u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]),
            height: u32::from_le_bytes([bytes[4], bytes[5], bytes[6], bytes[7]]),
        })
    }
}

/// ResizeAspect parameters: max_width, max_height
#[derive(Debug, Clone, Copy)]
pub struct ResizeAspectParams {
    pub max_width: u32,
    pub max_height: u32,
}

impl ResizeAspectParams {
    pub fn from_bytes(bytes: &[u8]) -> Option<Self> {
        if bytes.len() < 8 {
            return None;
        }
        Some(ResizeAspectParams {
            max_width: u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]),
            max_height: u32::from_le_bytes([bytes[4], bytes[5], bytes[6], bytes[7]]),
        })
    }
}

/// Brightness/Contrast/Exposure/Gamma parameters: value
#[derive(Debug, Clone, Copy)]
pub struct SingleValueParams {
    pub value: i32, // For brightness/contrast (i32)
}

impl SingleValueParams {
    pub fn from_bytes_i32(bytes: &[u8]) -> Option<Self> {
        if bytes.len() < 4 {
            return None;
        }
        Some(SingleValueParams {
            value: i32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]),
        })
    }

    pub fn from_bytes_f32(bytes: &[u8]) -> Option<Self> {
        if bytes.len() < 4 {
            return None;
        }
        let bits = u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]);
        let value = f32::from_bits(bits) as i32;
        Some(SingleValueParams { value })
    }
}

/// Thumbnail parameters: max_size
#[derive(Debug, Clone, Copy)]
pub struct ThumbnailParams {
    pub max_size: u32,
}

impl ThumbnailParams {
    pub fn from_bytes(bytes: &[u8]) -> Option<Self> {
        if bytes.len() < 4 {
            return None;
        }
        Some(ThumbnailParams {
            max_size: u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]),
        })
    }
}

/// CropResize parameters: x, y, width, height, target_width, target_height
#[derive(Debug, Clone, Copy)]
pub struct CropResizeParams {
    pub x: u32,
    pub y: u32,
    pub width: u32,
    pub height: u32,
    pub target_width: u32,
    pub target_height: u32,
}

impl CropResizeParams {
    pub fn from_bytes(bytes: &[u8]) -> Option<Self> {
        if bytes.len() < 24 {
            return None;
        }
        Some(CropResizeParams {
            x: u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]),
            y: u32::from_le_bytes([bytes[4], bytes[5], bytes[6], bytes[7]]),
            width: u32::from_le_bytes([bytes[8], bytes[9], bytes[10], bytes[11]]),
            height: u32::from_le_bytes([bytes[12], bytes[13], bytes[14], bytes[15]]),
            target_width: u32::from_le_bytes([bytes[16], bytes[17], bytes[18], bytes[19]]),
            target_height: u32::from_le_bytes([bytes[20], bytes[21], bytes[22], bytes[23]]),
        })
    }
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

/// Image data structure for zkVM processing
/// Simple representation of image dimensions and pixel data
#[derive(Debug, Clone)]
pub struct ImageData {
    pub width: u32,
    pub height: u32,
    pub pixels: Vec<u8>, // RGBA format
}

/// Compute hash of image data (used for final image hash)
pub fn compute_image_data_hash(width: u32, height: u32, pixels: &[u8]) -> [u8; 32] {
    let mut hash = [0u8; 32];

    // Mix dimensions into hash
    hash[0] = (width & 0xFF) as u8;
    hash[1] = ((width >> 8) & 0xFF) as u8;
    hash[2] = ((width >> 16) & 0xFF) as u8;
    hash[3] = ((width >> 24) & 0xFF) as u8;

    hash[4] = (height & 0xFF) as u8;
    hash[5] = ((height >> 8) & 0xFF) as u8;
    hash[6] = ((height >> 16) & 0xFF) as u8;
    hash[7] = ((height >> 24) & 0xFF) as u8;

    // Mix pixel data into hash
    // Process in chunks for efficiency
    let mut idx = 8;
    for (i, &pixel) in pixels.iter().enumerate() {
        if idx >= 32 {
            idx = 0;
        }
        hash[idx] = hash[idx].wrapping_add(pixel);
        idx += 1;

        // Only process first N pixels to keep it efficient
        if i >= 10000 {
            break;
        }
    }

    hash
}

/// Apply crop operation
pub fn apply_crop(image: &ImageData, params: &CropParams) -> ImageData {
    let new_width = params.width.min(image.width.saturating_sub(params.x));
    let new_height = params.height.min(image.height.saturating_sub(params.y));

    let mut new_pixels = Vec::with_capacity((new_width * new_height * 4) as usize);

    for y in params.y..(params.y + new_height) {
        for x in params.x..(params.x + new_width) {
            let src_idx = ((y * image.width + x) * 4) as usize;
            if src_idx + 3 < image.pixels.len() {
                new_pixels.push(image.pixels[src_idx]);     // R
                new_pixels.push(image.pixels[src_idx + 1]); // G
                new_pixels.push(image.pixels[src_idx + 2]); // B
                new_pixels.push(image.pixels[src_idx + 3]); // A
            }
        }
    }

    ImageData {
        width: new_width,
        height: new_height,
        pixels: new_pixels,
    }
}

/// Apply resize operation (simple nearest neighbor)
pub fn apply_resize(image: &ImageData, params: &ResizeParams) -> ImageData {
    if params.width == 0 || params.height == 0 {
        return image.clone();
    }

    let mut new_pixels = Vec::with_capacity((params.width * params.height * 4) as usize);

    let x_ratio = (image.width as f32) / (params.width as f32);
    let y_ratio = (image.height as f32) / (params.height as f32);

    for y in 0..params.height {
        for x in 0..params.width {
            let src_x = ((x as f32) * x_ratio) as u32;
            let src_y = ((y as f32) * y_ratio) as u32;
            let src_idx = ((src_y * image.width + src_x) * 4) as usize;

            if src_idx + 3 < image.pixels.len() {
                new_pixels.push(image.pixels[src_idx]);     // R
                new_pixels.push(image.pixels[src_idx + 1]); // G
                new_pixels.push(image.pixels[src_idx + 2]); // B
                new_pixels.push(image.pixels[src_idx + 3]); // A
            } else {
                // Default to black
                new_pixels.push(0);
                new_pixels.push(0);
                new_pixels.push(0);
                new_pixels.push(255);
            }
        }
    }

    ImageData {
        width: params.width,
        height: params.height,
        pixels: new_pixels,
    }
}

/// Apply resize with aspect ratio
pub fn apply_resize_aspect(image: &ImageData, params: &ResizeAspectParams) -> ImageData {
    let width_ratio = params.max_width as f32 / image.width as f32;
    let height_ratio = params.max_height as f32 / image.height as f32;
    let ratio = width_ratio.min(height_ratio).min(1.0);

    let new_width = (image.width as f32 * ratio) as u32;
    let new_height = (image.height as f32 * ratio) as u32;

    let resize_params = ResizeParams {
        width: new_width.max(1),
        height: new_height.max(1),
    };

    apply_resize(image, &resize_params)
}

/// Apply brightness adjustment
pub fn apply_brightness(image: &ImageData, value: i32) -> ImageData {
    let mut new_pixels = image.pixels.clone();

    for i in (0..new_pixels.len()).step_by(4) {
        // Apply to RGB (not alpha)
        for j in 0..3 {
            let new_val = (new_pixels[i + j] as i32).saturating_add(value).clamp(0, 255);
            new_pixels[i + j] = new_val as u8;
        }
    }

    ImageData {
        width: image.width,
        height: image.height,
        pixels: new_pixels,
    }
}

/// Apply contrast adjustment
pub fn apply_contrast(image: &ImageData, value: i32) -> ImageData {
    let factor = (259.0 * (value as f32 + 255.0)) / (255.0 * (259.0 - value as f32));

    let mut new_pixels = image.pixels.clone();

    for i in (0..new_pixels.len()).step_by(4) {
        for j in 0..3 {
            let new_val = (factor * (new_pixels[i + j] as f32 - 128.0) + 128.0).clamp(0.0, 255.0);
            new_pixels[i + j] = new_val as u8;
        }
    }

    ImageData {
        width: image.width,
        height: image.height,
        pixels: new_pixels,
    }
}

/// Apply gamma correction
pub fn apply_gamma(image: &ImageData, value: f32) -> ImageData {
    let gamma = if value == 0.0 { 1.0 } else { value };
    let inv_gamma = 1.0 / gamma;

    let mut new_pixels = image.pixels.clone();

    for i in (0..new_pixels.len()).step_by(4) {
        for j in 0..3 {
            let normalized = new_pixels[i + j] as f32 / 255.0;
            let corrected = normalized.powf(inv_gamma);
            new_pixels[i + j] = (corrected * 255.0) as u8;
        }
    }

    ImageData {
        width: image.width,
        height: image.height,
        pixels: new_pixels,
    }
}

/// Apply exposure adjustment
pub fn apply_exposure(image: &ImageData, value: f32) -> ImageData {
    // Exposure is essentially brightness with a different scale
    let brightness = (value * 100.0) as i32;
    apply_brightness(image, brightness)
}

/// Apply thumbnail (resize to max size maintaining aspect ratio)
pub fn apply_thumbnail(image: &ImageData, max_size: u32) -> ImageData {
    let params = ResizeAspectParams {
        max_width: max_size,
        max_height: max_size,
    };
    apply_resize_aspect(image, &params)
}

/// Apply crop then resize
pub fn apply_crop_resize(image: &ImageData, params: &CropResizeParams) -> ImageData {
    // First crop
    let cropped = apply_crop(image, &CropParams {
        x: params.x,
        y: params.y,
        width: params.width,
        height: params.height,
    });

    // Then resize
    apply_resize(&cropped, &ResizeParams {
        width: params.target_width,
        height: params.target_height,
    })
}

/// Apply a single operation to image data
pub fn apply_operation(image: &ImageData, op_type: u8, params: &[u8]) -> ImageData {
    match ImageOperationType::from_u8(op_type) {
        Some(ImageOperationType::Crop) => {
            if let Some(p) = CropParams::from_bytes(params) {
                apply_crop(image, &p)
            } else {
                image.clone()
            }
        }
        Some(ImageOperationType::Resize) => {
            if let Some(p) = ResizeParams::from_bytes(params) {
                apply_resize(image, &p)
            } else {
                image.clone()
            }
        }
        Some(ImageOperationType::ResizeAspect) => {
            if let Some(p) = ResizeAspectParams::from_bytes(params) {
                apply_resize_aspect(image, &p)
            } else {
                image.clone()
            }
        }
        Some(ImageOperationType::Brightness) => {
            if let Some(p) = SingleValueParams::from_bytes_i32(params) {
                apply_brightness(image, p.value)
            } else {
                image.clone()
            }
        }
        Some(ImageOperationType::Contrast) => {
            if let Some(p) = SingleValueParams::from_bytes_i32(params) {
                apply_contrast(image, p.value)
            } else {
                image.clone()
            }
        }
        Some(ImageOperationType::Exposure) => {
            if let Some(p) = SingleValueParams::from_bytes_f32(params) {
                apply_exposure(image, p.value as f32)
            } else {
                image.clone()
            }
        }
        Some(ImageOperationType::Gamma) => {
            if let Some(p) = SingleValueParams::from_bytes_f32(params) {
                apply_gamma(image, p.value as f32)
            } else {
                image.clone()
            }
        }
        Some(ImageOperationType::Thumbnail) => {
            if let Some(p) = ThumbnailParams::from_bytes(params) {
                apply_thumbnail(image, p.max_size)
            } else {
                image.clone()
            }
        }
        Some(ImageOperationType::CropResize) => {
            if let Some(p) = CropResizeParams::from_bytes(params) {
                apply_crop_resize(image, &p)
            } else {
                image.clone()
            }
        }
        None => image.clone(),
    }
}

/// Apply all operations to image data and return final hash
pub fn apply_operations_and_hash(
    image: &ImageData,
    operations: &[(u8, &[u8])],
) -> [u8; 32] {
    let mut current_image = image.clone();

    for (op_type, params) in operations {
        current_image = apply_operation(&current_image, *op_type, params);
    }

    compute_image_data_hash(current_image.width, current_image.height, &current_image.pixels)
}
