use clap::Parser;
use image::GenericImageView;
use image_editor::{CropRegion, ResizeFilter, ResizeOptions};
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::fs;
use std::path::PathBuf;
use walkdir::WalkDir;

#[derive(Parser, Debug)]
#[command(author, version, about, long_about = None)]
struct Args {
    /// Path to the folder containing images
    #[arg(default_value = ".")]
    path: PathBuf,

    /// Scan subdirectories recursively
    #[arg(short, long)]
    recursive: bool,

    /// Output in JSON format
    #[arg(short, long)]
    json: bool,

    /// Sort by file size (largest first)
    #[arg(short, long)]
    sort: bool,

    /// Verify C2PA signatures
    #[arg(short, long)]
    verify_c2pa: bool,

    /// Crop the image (format: x,y,width,height)
    #[arg(long, value_name = "X,Y,WIDTH,HEIGHT")]
    crop: Option<String>,

    /// Resize the image (format: width,height)
    #[arg(long, value_name = "WIDTH,HEIGHT")]
    resize: Option<String>,

    /// Resize filter (nearest, triangle, catmullrom, gaussian, lanczos3)
    #[arg(long, default_value = "triangle")]
    resize_filter: String,

    /// Adjust exposure (-2.0 to 2.0)
    #[arg(long)]
    exposure: Option<f32>,

    /// Adjust brightness (-255 to 255)
    #[arg(long)]
    brightness: Option<i32>,

    /// Adjust contrast (-255 to 255)
    #[arg(long)]
    contrast: Option<i32>,

    /// Adjust gamma (0.1 to 5.0)
    #[arg(long)]
    gamma: Option<f32>,

    /// Output path for edited image
    #[arg(long, short)]
    output: Option<PathBuf>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
struct C2PAInfo {
    has_c2pa: bool,
    has_manifest: bool,
    manifest_count: usize,
    claim_label: Option<String>,
    signature_present: bool,
    error: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
struct ImageInfo {
    name: String,
    path: String,
    size_bytes: u64,
    size_formatted: String,
    width: u32,
    height: u32,
    format: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    c2pa: Option<C2PAInfo>,
}

fn format_size(bytes: u64) -> String {
    const KB: u64 = 1024;
    const MB: u64 = KB * 1024;
    const GB: u64 = MB * 1024;

    if bytes >= GB {
        format!("{:.2} GB", bytes as f64 / GB as f64)
    } else if bytes >= MB {
        format!("{:.2} MB", bytes as f64 / MB as f64)
    } else if bytes >= KB {
        format!("{:.2} KB", bytes as f64 / KB as f64)
    } else {
        format!("{} B", bytes)
    }
}

fn is_image_file(path: &std::path::Path) -> bool {
    let image_extensions: HashSet<&str> = [
        "jpg", "jpeg", "png", "gif", "bmp", "webp", "tiff", "tif", "ico", "svg",
    ]
    .iter()
    .cloned()
    .collect();

    path.extension()
        .and_then(|ext| ext.to_str())
        .map(|ext| image_extensions.contains(ext.to_lowercase().as_str()))
        .unwrap_or(false)
}

fn read_be_u32(data: &[u8], offset: usize) -> Option<u32> {
    if data.len() < offset + 4 {
        return None;
    }
    Some(u32::from_be_bytes([data[offset], data[offset + 1], data[offset + 2], data[offset + 3]]))
}

/// Parse JUMBF boxes to detect C2PA metadata
/// Returns (has_c2pa_manifest, signature_present, manifest_count)
fn parse_jumbf_boxes(data: &[u8]) -> (bool, bool, usize) {
    let mut has_c2pa = false;
    let mut has_signature = false;
    let mut manifest_count = 0;

    // Search for "jumb" signature anywhere in the file
    let jumb_signature = b"jumb";
    let c2pa_type_signatures: &[&[u8]] = &[b"jumdc2pa", b"ijumb", b"c2pa."];

    let mut pos = 0;
    while pos < data.len() - 8 {
        // Look for "jumb" signature
        let end = (pos + 4).min(data.len());
        if end >= pos + 4 && &data[pos..end] == jumb_signature {
            // Get the box size
            let box_size = read_be_u32(data, pos + 4).unwrap_or(0) as usize;

            // Check for C2PA content types (starts at pos + 8)
            if pos + 8 < data.len() {
                let search_end = (pos + 16).min(data.len());
                let type_slice = &data[pos + 8..search_end];

                // Check if this is a C2PA-related JUMBF box
                let is_c2pa_box = c2pa_type_signatures.iter().any(|sig| {
                    type_slice.len() >= sig.len() && &type_slice[..sig.len()] == *sig
                });

                if is_c2pa_box {
                    has_c2pa = true;
                    manifest_count += 1;
                }

                if box_size > 8 {
                    pos += box_size;
                    continue;
                }
            }
        }
        pos += 1;
    }

    // Search for signature box types anywhere in the file
    if data.windows(8).any(|w| &w[4..8] == b"SSIG") {
        has_signature = true;
    }
    if data.windows(8).any(|w| &w[4..8] == b"sig.") {
        has_signature = true;
    }

    // Also search for specific C2PA strings as fallback
    let c2pa_strings = ["c2pa", "actions", "assertions", "claim"];
    for search_str in c2pa_strings {
        if let Some(found) = data.windows(search_str.len()).position(|w| w == search_str.as_bytes()) {
            // Only count if it's likely in a JUMBF context (has null bytes or "jumb" nearby)
            let context_start = found.saturating_sub(20);
            let context_end = found.min(data.len());
            if context_start < context_end {
                let context = &data[context_start..context_end];
                let has_jumb = context.windows(4).any(|w| w == b"jumb");
                let has_null = context.iter().any(|&b| b == 0);
                if has_jumb || has_null {
                    has_c2pa = true;
                }
            }
        }
    }

    (has_c2pa, has_signature, manifest_count)
}

fn verify_c2pa(path: &std::path::Path) -> C2PAInfo {
    // Read entire file for C2PA parsing
    let data = match std::fs::read(path) {
        Ok(d) => d,
        Err(e) => {
            return C2PAInfo {
                has_c2pa: false,
                has_manifest: false,
                manifest_count: 0,
                claim_label: None,
                signature_present: false,
                error: Some(format!("Failed to read file: {}", e)),
            };
        }
    };

    // Parse JUMBF boxes
    let (has_c2pa, has_signature, manifest_count) = parse_jumbf_boxes(&data);

    C2PAInfo {
        has_c2pa,
        has_manifest: has_c2pa,
        manifest_count,
        claim_label: if has_c2pa { Some("c2pa".to_string()) } else { None },
        signature_present: has_signature,
        error: None,
    }
}

fn get_image_info(path: &std::path::Path, do_verify_c2pa: bool) -> Option<ImageInfo> {
    let metadata = fs::metadata(path).ok()?;
    let size_bytes = metadata.len();

    let img = image::open(path).ok()?;
    let (width, height) = img.dimensions();

    let format = path
        .extension()
        .and_then(|ext| ext.to_str())
        .map(|s| s.to_uppercase())
        .unwrap_or_else(|| "Unknown".to_string());

    let c2pa_info = if do_verify_c2pa && (format == "JPG" || format == "JPEG") {
        Some(verify_c2pa(path))
    } else {
        None
    };

    Some(ImageInfo {
        name: path.file_name()?.to_string_lossy().to_string(),
        path: path.to_string_lossy().to_string(),
        size_bytes,
        size_formatted: format_size(size_bytes),
        width,
        height,
        format,
        c2pa: c2pa_info,
    })
}

fn scan_directory(path: &PathBuf, recursive: bool, do_verify_c2pa: bool) -> Vec<ImageInfo> {
    let mut images: Vec<ImageInfo> = Vec::new();

    if recursive {
        for entry in WalkDir::new(path).into_iter().filter_map(|e| e.ok()) {
            if entry.file_type().is_file() && is_image_file(entry.path()) {
                if let Some(info) = get_image_info(&entry.path(), do_verify_c2pa) {
                    images.push(info);
                }
            }
        }
    } else {
        if let Ok(entries) = fs::read_dir(path) {
            for entry in entries.filter_map(|e| e.ok()) {
                let path = entry.path();
                if path.is_file() && is_image_file(&path) {
                    if let Some(info) = get_image_info(&path, do_verify_c2pa) {
                        images.push(info);
                    }
                }
            }
        }
    }

    images
}

fn print_table(images: &[ImageInfo], do_verify_c2pa: bool) {
    if do_verify_c2pa {
        println!("{:<40} {:>12} {:>15} {:>10} {:>12} {:>10}", "Name", "Size", "Dimensions", "Format", "C2PA", "Signed");
        println!("{}", "-".repeat(105));
    } else {
        println!("{:<40} {:>12} {:>15} {:>10}", "Name", "Size", "Dimensions", "Format");
        println!("{}", "-".repeat(80));
    }

    for img in images {
        let (c2pa_status, signed_status) = img.c2pa.as_ref().map(|c| {
            let c2pa = if c.has_c2pa { "Present" } else { "None" };
            let signed = if c.signature_present { "Yes" } else { "No" };
            (c2pa, signed)
        }).unwrap_or(("-", "-"));

        if do_verify_c2pa {
            println!(
                "{:<40} {:>12} {:>15} {:>10} {:>12} {:>10}",
                if img.name.len() > 38 {
                    format!("...{}", &img.name[img.name.len() - 37..])
                } else {
                    img.name.clone()
                },
                img.size_formatted,
                format!("{}x{}", img.width, img.height),
                img.format,
                c2pa_status,
                signed_status
            );
        } else {
            println!(
                "{:<40} {:>12} {:>15} {:>10}",
                if img.name.len() > 38 {
                    format!("...{}", &img.name[img.name.len() - 37..])
                } else {
                    img.name.clone()
                },
                img.size_formatted,
                format!("{}x{}", img.width, img.height),
                img.format
            );
        }
    }

    println!("\nTotal: {} image(s)", images.len());
}

fn print_json(images: &[ImageInfo]) {
    println!("{}", serde_json::to_string_pretty(images).unwrap_or_else(|_| "[]".to_string()));
}

fn parse_resize_filter(filter: &str) -> ResizeFilter {
    match filter.to_lowercase().as_str() {
        "nearest" => ResizeFilter::Nearest,
        "triangle" => ResizeFilter::Triangle,
        "catmullrom" => ResizeFilter::CatmullRom,
        "gaussian" => ResizeFilter::Gaussian,
        "lanczos3" => ResizeFilter::Lanczos3,
        _ => ResizeFilter::Triangle,
    }
}

fn edit_image(path: &PathBuf, args: &Args) -> Result<(), String> {
    let input_path = path.to_string_lossy().to_string();

    // Determine output path
    let output_path = if let Some(ref output) = args.output {
        output.to_string_lossy().to_string()
    } else {
        // Default: add _edited before extension
        let stem = path.file_stem().unwrap_or_default().to_string_lossy();
        let ext = path.extension().unwrap_or_default().to_string_lossy();
        let parent = path.parent().unwrap_or(std::path::Path::new("."));
        format!("{}/{}_edited.{}", parent.display(), stem, ext)
    };

    // Handle crop
    if let Some(crop_str) = &args.crop {
        let parts: Vec<&str> = crop_str.split(',').collect();
        if parts.len() != 4 {
            return Err("Crop format must be: x,y,width,height".to_string());
        }

        let x: u32 = parts[0].parse().map_err(|_| "Invalid x value")?;
        let y: u32 = parts[1].parse().map_err(|_| "Invalid y value")?;
        let w: u32 = parts[2].parse().map_err(|_| "Invalid width value")?;
        let h: u32 = parts[3].parse().map_err(|_| "Invalid height value")?;

        let region = CropRegion { x, y, width: w, height: h };

        // Handle resize after crop
        if let Some(resize_str) = &args.resize {
            let parts: Vec<&str> = resize_str.split(',').collect();
            if parts.len() != 2 {
                return Err("Resize format must be: width,height".to_string());
            }

            let width: u32 = parts[0].parse().map_err(|_| "Invalid width")?;
            let height: u32 = parts[1].parse().map_err(|_| "Invalid height")?;
            let filter = parse_resize_filter(&args.resize_filter);

            let resize_options = ResizeOptions { width, height, filter };

            let result = image_editor::crop_and_resize(&input_path, region, resize_options, &output_path)?;
            println!("Cropped and resized: {}x{} -> {}x{}",
                result.original_width, result.original_height,
                result.new_width, result.new_height);
        } else {
            let result = image_editor::crop_image(&input_path, region, &output_path)?;
            println!("Cropped: {}x{} -> {}x{}",
                result.original_width, result.original_height,
                result.new_width, result.new_height);
        }
    }
    // Handle resize only (no crop)
    else if let Some(resize_str) = &args.resize {
        let parts: Vec<&str> = resize_str.split(',').collect();
        if parts.len() != 2 {
            return Err("Resize format must be: width,height".to_string());
        }

        let width: u32 = parts[0].parse().map_err(|_| "Invalid width")?;
        let height: u32 = parts[1].parse().map_err(|_| "Invalid height")?;
        let filter = parse_resize_filter(&args.resize_filter);

        let options = ResizeOptions { width, height, filter };
        let result = image_editor::resize_image(&input_path, options, &output_path)?;
        println!("Resized: {}x{} -> {}x{}",
            result.original_width, result.original_height,
            result.new_width, result.new_height);
    }
    // Handle exposure adjustments
    else if args.exposure.is_some() || args.brightness.is_some() || args.contrast.is_some() || args.gamma.is_some() {
        // Apply adjustments to input and save to output
        // Exposure
        if let Some(exp) = args.exposure {
            let result = image_editor::adjust_exposure(&input_path, exp, &output_path)?;
            println!("Exposure adjusted: {}", exp);
        }

        // Brightness
        if let Some(bright) = args.brightness {
            let result = image_editor::adjust_brightness(&input_path, bright, &output_path)?;
            println!("Brightness adjusted: {}", bright);
        }

        // Contrast
        if let Some(cont) = args.contrast {
            let result = image_editor::adjust_contrast(&input_path, cont, &output_path)?;
            println!("Contrast adjusted: {}", cont);
        }

        // Gamma
        if let Some(g) = args.gamma {
            let result = image_editor::adjust_gamma(&input_path, g, &output_path)?;
            println!("Gamma adjusted: {}", g);
        }
    }

    println!("Output saved to: {}", output_path);
    Ok(())
}

fn main() {
    let args = Args::parse();

    // Check for edit operations first
    let has_edit = args.crop.is_some() || args.resize.is_some()
        || args.exposure.is_some() || args.brightness.is_some()
        || args.contrast.is_some() || args.gamma.is_some();

    if !args.path.exists() {
        eprintln!("Error: Path '{}' does not exist", args.path.display());
        std::process::exit(1);
    }

    // Handle single file with edit operations
    if args.path.is_file() && is_image_file(&args.path) && has_edit {
        match edit_image(&args.path, &args) {
            Ok(()) => {}
            Err(e) => {
                eprintln!("Error: {}", e);
                std::process::exit(1);
            }
        }
        return;
    }

    if !args.path.is_dir() {
        if args.path.is_file() && is_image_file(&args.path) {
            // Single file mode - just show info
            if let Some(info) = get_image_info(&args.path, args.verify_c2pa) {
                if args.json {
                    print_json(&[info]);
                } else {
                    print_table(&[info], args.verify_c2pa);
                }
            } else {
                eprintln!("Error: Could not read image info from '{}'", args.path.display());
                std::process::exit(1);
            }
            return;
        } else {
            eprintln!("Error: Path '{}' is not a directory", args.path.display());
            std::process::exit(1);
        }
    }

    let mut images = scan_directory(&args.path, args.recursive, args.verify_c2pa);

    if images.is_empty() {
        println!("No image files found in '{}'", args.path.display());
        return;
    }

    if args.sort {
        images.sort_by(|a, b| b.size_bytes.cmp(&a.size_bytes));
    }

    if args.json {
        print_json(&images);
    } else {
        print_table(&images, args.verify_c2pa);
    }
}
