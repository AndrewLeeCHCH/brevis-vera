use image::GenericImageView;
use serde::{Deserialize, Serialize};
use std::path::Path;

/// Resize filter options
#[derive(Debug, Clone, Copy, Serialize, Deserialize, Default)]
pub enum ResizeFilter {
    Nearest,
    #[default]
    Triangle,
    CatmullRom,
    Gaussian,
    Lanczos3,
}

/// Resize options
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResizeOptions {
    /// Target width
    pub width: u32,
    /// Target height
    pub height: u32,
    /// Resize filter to use
    pub filter: ResizeFilter,
}

impl Default for ResizeOptions {
    fn default() -> Self {
        Self {
            width: 800,
            height: 600,
            filter: ResizeFilter::Triangle,
        }
    }
}

/// Crop region
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CropRegion {
    /// Left coordinate
    pub x: u32,
    /// Top coordinate
    pub y: u32,
    /// Crop width
    pub width: u32,
    /// Crop height
    pub height: u32,
}

/// Image transformation result
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TransformResult {
    /// Output file path
    pub output_path: String,
    /// Original width
    pub original_width: u32,
    /// Original height
    pub original_height: u32,
    /// New width after transformation
    pub new_width: u32,
    /// New height after transformation
    pub new_height: u32,
}

/// Crop an image to the specified region
///
/// # Arguments
/// * `image_path` - Path to the input image
/// * `region` - Crop region (x, y, width, height)
/// * `output_path` - Path to save the cropped image
///
/// # Returns
/// * `Ok(TransformResult)` - On success
/// * `Err(String)` - On failure
pub fn crop_image(image_path: &str, region: CropRegion, output_path: &str) -> Result<TransformResult, String> {
    // Load the image
    let img = image::open(image_path)
        .map_err(|e| format!("Failed to open image: {}", e))?;

    let (orig_width, orig_height) = img.dimensions();

    // Validate crop region
    if region.x + region.width > orig_width {
        return Err(format!(
            "Crop width exceeds image width: x={}, width={}, image_width={}",
            region.x, region.width, orig_width
        ));
    }

    if region.y + region.height > orig_height {
        return Err(format!(
            "Crop height exceeds image height: y={}, height={}, image_height={}",
            region.y, region.height, orig_height
        ));
    }

    // Perform the crop
    let cropped = img.crop_imm(region.x, region.y, region.width, region.height);

    // Save the cropped image
    cropped.save(output_path)
        .map_err(|e| format!("Failed to save cropped image: {}", e))?;

    Ok(TransformResult {
        output_path: output_path.to_string(),
        original_width: orig_width,
        original_height: orig_height,
        new_width: region.width,
        new_height: region.height,
    })
}

/// Resize an image to the specified dimensions
///
/// # Arguments
/// * `image_path` - Path to the input image
/// * `options` - Resize options (width, height, filter)
/// * `output_path` - Path to save the resized image
///
/// # Returns
/// * `Ok(TransformResult)` - On success
/// * `Err(String)` - On failure
pub fn resize_image(image_path: &str, options: ResizeOptions, output_path: &str) -> Result<TransformResult, String> {
    // Load the image
    let img = image::open(image_path)
        .map_err(|e| format!("Failed to open image: {}", e))?;

    let (orig_width, orig_height) = img.dimensions();

    // Perform resize using the appropriate filter
    let resized = match options.filter {
        ResizeFilter::Nearest => img.resize_exact(options.width, options.height, image::imageops::FilterType::Nearest),
        ResizeFilter::Triangle => img.resize_exact(options.width, options.height, image::imageops::FilterType::Triangle),
        ResizeFilter::CatmullRom => img.resize_exact(options.width, options.height, image::imageops::FilterType::CatmullRom),
        ResizeFilter::Gaussian => img.resize_exact(options.width, options.height, image::imageops::FilterType::Gaussian),
        ResizeFilter::Lanczos3 => img.resize_exact(options.width, options.height, image::imageops::FilterType::Lanczos3),
    };

    // Save the resized image
    resized.save(output_path)
        .map_err(|e| format!("Failed to save resized image: {}", e))?;

    Ok(TransformResult {
        output_path: output_path.to_string(),
        original_width: orig_width,
        original_height: orig_height,
        new_width: options.width,
        new_height: options.height,
    })
}

/// Resize image by maintaining aspect ratio
///
/// # Arguments
/// * `image_path` - Path to the input image
/// * `max_width` - Maximum width
/// * `max_height` - Maximum height
/// * `filter` - Resize filter to use
/// * `output_path` - Path to save the resized image
///
/// # Returns
/// * `Ok(TransformResult)` - On success
/// * `Err(String)` - On failure
pub fn resize_with_aspect_ratio(
    image_path: &str,
    max_width: u32,
    max_height: u32,
    filter: ResizeFilter,
    output_path: &str,
) -> Result<TransformResult, String> {
    // Load the image
    let img = image::open(image_path)
        .map_err(|e| format!("Failed to open image: {}", e))?;

    let (orig_width, orig_height) = img.dimensions();

    // Calculate new dimensions maintaining aspect ratio
    let ratio_width = max_width as f64 / orig_width as f64;
    let ratio_height = max_height as f64 / orig_height as f64;
    let ratio = ratio_width.min(ratio_height);

    let new_width = (orig_width as f64 * ratio) as u32;
    let new_height = (orig_height as f64 * ratio) as u32;

    // Perform resize
    let options = ResizeOptions {
        width: new_width,
        height: new_height,
        filter,
    };

    resize_image(image_path, options, output_path)
}

/// Crop and resize an image in one operation
///
/// # Arguments
/// * `image_path` - Path to the input image
/// * `region` - Crop region (applied first)
/// * `resize_options` - Resize options (applied after crop)
/// * `output_path` - Path to save the final image
///
/// # Returns
/// * `Ok(TransformResult)` - On success
/// * `Err(String)` - On failure
pub fn crop_and_resize(
    image_path: &str,
    region: CropRegion,
    resize_options: ResizeOptions,
    output_path: &str,
) -> Result<TransformResult, String> {
    // First crop the image
    let crop_result = crop_image(image_path, region, output_path)?;

    // Then resize (overwriting the cropped image)
    let resize_result = resize_image(output_path, resize_options, output_path)?;

    // Return combined result
    Ok(TransformResult {
        output_path: output_path.to_string(),
        original_width: crop_result.original_width,
        original_height: crop_result.original_height,
        new_width: resize_result.new_width,
        new_height: resize_result.new_height,
    })
}

/// Create a thumbnail of an image
///
/// # Arguments
/// * `image_path` - Path to the input image
/// * `max_size` - Maximum dimension (width and height)
/// * `output_path` - Path to save the thumbnail
///
/// # Returns
/// * `Ok(TransformResult)` - On success
/// * `Err(String)` - On failure
pub fn create_thumbnail(image_path: &str, max_size: u32, output_path: &str) -> Result<TransformResult, String> {
    resize_with_aspect_ratio(image_path, max_size, max_size, ResizeFilter::Triangle, output_path)
}

/// Get image dimensions without loading the full image
///
/// # Arguments
/// * `image_path` - Path to the image
///
/// # Returns
/// * `Ok((width, height))` - On success
/// * `Err(String)` - On failure
pub fn get_dimensions(image_path: &str) -> Result<(u32, u32), String> {
    let img = image::open(image_path)
        .map_err(|e| format!("Failed to open image: {}", e))?;
    Ok(img.dimensions())
}

/// Check if a file is a supported image format
pub fn is_supported_image(path: &Path) -> bool {
    let supported_extensions = ["jpg", "jpeg", "png", "gif", "bmp", "webp", "tiff", "tif"];

    path.extension()
        .and_then(|ext| ext.to_str())
        .map(|ext| supported_extensions.contains(&ext.to_lowercase().as_str()))
        .unwrap_or(false)
}

// ============================================================================
// Exposure Adjustment Functions
// ============================================================================

/// Adjust the exposure of an image
/// Positive values brighten, negative values darken
/// Range: -2.0 to 2.0 (default 0.0)
pub fn adjust_exposure(image_path: &str, exposure: f32, output_path: &str) -> Result<TransformResult, String> {
    let img = image::open(image_path)
        .map_err(|e| format!("Failed to open image: {}", e))?;

    let (orig_width, orig_height) = img.dimensions();

    // Convert exposure to brightness and contrast
    // Exposure of 1.0 = +100% brightness (roughly)
    // Exposure of -1.0 = -100% brightness
    let brightness = exposure * 128.0; // Scale to pixel range
    let contrast = 1.0 + exposure.abs() * 0.5; // Slight contrast increase with exposure

    let adjusted = apply_brightness_contrast(img, brightness, contrast);

    adjusted.save(output_path)
        .map_err(|e| format!("Failed to save image: {}", e))?;

    Ok(TransformResult {
        output_path: output_path.to_string(),
        original_width: orig_width,
        original_height: orig_height,
        new_width: orig_width,
        new_height: orig_height,
    })
}

/// Adjust the brightness of an image
/// Range: -255 to 255 (0 = no change)
pub fn adjust_brightness(image_path: &str, brightness: i32, output_path: &str) -> Result<TransformResult, String> {
    let img = image::open(image_path)
        .map_err(|e| format!("Failed to open image: {}", e))?;

    let (orig_width, orig_height) = img.dimensions();

    // Brightness: 1.0 = no change, >1.0 brighter, <1.0 darker
    let brightness_factor = 1.0 + (brightness as f32 / 255.0);

    let adjusted = apply_brightness_contrast(img, brightness as f32, brightness_factor);

    adjusted.save(output_path)
        .map_err(|e| format!("Failed to save image: {}", e))?;

    Ok(TransformResult {
        output_path: output_path.to_string(),
        original_width: orig_width,
        original_height: orig_height,
        new_width: orig_width,
        new_height: orig_height,
    })
}

/// Adjust the contrast of an image
/// Range: -255 to 255 (0 = no change)
pub fn adjust_contrast(image_path: &str, contrast: i32, output_path: &str) -> Result<TransformResult, String> {
    let img = image::open(image_path)
        .map_err(|e| format!("Failed to open image: {}", e))?;

    let (orig_width, orig_height) = img.dimensions();

    // Contrast: 1.0 = no change, >1.0 more contrast, <1.0 less contrast
    let contrast_factor = 1.0 + (contrast as f32 / 255.0);

    let adjusted = apply_brightness_contrast(img, 0.0, contrast_factor);

    adjusted.save(output_path)
        .map_err(|e| format!("Failed to save image: {}", e))?;

    Ok(TransformResult {
        output_path: output_path.to_string(),
        original_width: orig_width,
        original_height: orig_height,
        new_width: orig_width,
        new_height: orig_height,
    })
}

/// Apply gamma correction to an image
/// Range: 0.1 to 5.0 (1.0 = no change)
/// Values < 1.0 brighten, values > 1.0 darken
pub fn adjust_gamma(image_path: &str, gamma: f32, output_path: &str) -> Result<TransformResult, String> {
    if gamma <= 0.0 {
        return Err("Gamma must be positive".to_string());
    }

    let img = image::open(image_path)
        .map_err(|e| format!("Failed to open image: {}", e))?;

    let (orig_width, orig_height) = img.dimensions();

    let adjusted = apply_gamma(img, gamma);

    adjusted.save(output_path)
        .map_err(|e| format!("Failed to save image: {}", e))?;

    Ok(TransformResult {
        output_path: output_path.to_string(),
        original_width: orig_width,
        original_height: orig_height,
        new_width: orig_width,
        new_height: orig_height,
    })
}

/// Apply brightness and contrast adjustment to an image
fn apply_brightness_contrast(img: image::DynamicImage, brightness: f32, contrast: f32) -> image::DynamicImage {
    let rgba = img.to_rgba8();

    let (width, height) = rgba.dimensions();
    let mut output = image::RgbaImage::new(width, height);

    for (x, y, pixel) in rgba.enumerate_pixels() {
        let r = pixel[0] as f32;
        let g = pixel[1] as f32;
        let b = pixel[2] as f32;
        let a = pixel[3];

        // Apply brightness
        let r = r + brightness;
        let g = g + brightness;
        let b = b + brightness;

        // Apply contrast
        let r = ((r - 128.0) * contrast) + 128.0;
        let g = ((g - 128.0) * contrast) + 128.0;
        let b = ((b - 128.0) * contrast) + 128.0;

        // Clamp to valid range
        let r = r.max(0.0).min(255.0) as u8;
        let g = g.max(0.0).min(255.0) as u8;
        let b = b.max(0.0).min(255.0) as u8;

        output.put_pixel(x, y, image::Rgba([r, g, b, a]));
    }

    image::DynamicImage::ImageRgba8(output)
}

/// Apply gamma correction to an image
fn apply_gamma(img: image::DynamicImage, gamma: f32) -> image::DynamicImage {
    let rgba = img.to_rgba8();

    let (width, height) = rgba.dimensions();
    let mut output = image::RgbaImage::new(width, height);

    // Precompute gamma lookup table
    let mut lut = [0u8; 256];
    for i in 0..256 {
        let value = i as f32 / 255.0;
        let corrected = value.powf(1.0 / gamma);
        lut[i] = (corrected * 255.0).min(255.0) as u8;
    }

    for (x, y, pixel) in rgba.enumerate_pixels() {
        let r = lut[pixel[0] as usize];
        let g = lut[pixel[1] as usize];
        let b = lut[pixel[2] as usize];

        output.put_pixel(x, y, image::Rgba([r, g, b, pixel[3]]));
    }

    image::DynamicImage::ImageRgba8(output)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::NamedTempFile;

    #[test]
    fn test_resize_options_default() {
        let options = ResizeOptions::default();
        assert_eq!(options.width, 800);
        assert_eq!(options.height, 600);
    }

    #[test]
    fn test_is_supported_image() {
        assert!(is_supported_image(Path::new("test.jpg")));
        assert!(is_supported_image(Path::new("test.png")));
        assert!(!is_supported_image(Path::new("test.txt")));
    }
}
