use alloy_sol_types::SolType;
use c2pa_lib::{extract_c2pa_from_jpeg, load_elf, verify_c2pa_claim, PublicValuesStruct};
use clap::Parser;
use image::GenericImageView;
use image_editor::{CropRegion, ResizeOptions};
use pico_sdk::{client::DefaultProverClient, init_logger};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::path::Path;

// CLI tool for generating ZK proofs for image attestation
#[derive(Parser, Debug)]
#[command(name = "zk-proof-cli")]
#[command(about = "Generate ZK proofs for C2PA image attestation", long_about = None)]
struct Args {
    // Proof ID from server (loads from proofs/<proof_id>/ folder)
    #[arg(long)]
    proof_id: Option<String>,

    // Input image path
    #[arg(short, long)]
    image: Option<String>,

    // Operations to apply (can be specified multiple times)
    #[arg(short, long)]
    op: Vec<String>,

    // Output path for processed image
    #[arg(short, long, default_value = "output.jpg")]
    output: String,

    // JSON input file (alternative to --image and --op)
    #[arg(long)]
    json_input: Option<String>,

    // Output JSON file for proof data (prints to stdout if not specified)
    #[arg(long)]
    json_output: Option<String>,

    // ELF file path (defaults to embedded path)
    #[arg(long)]
    elf_path: Option<String>,

    // Verbose output
    #[arg(short, long, default_value = "false")]
    verbose: bool,
}

/// Operation type
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
enum Operation {
    Crop { x: u32, y: u32, width: u32, height: u32 },
    Resize { width: u32, height: u32 },
    ResizeAspect { max_width: u32, max_height: u32 },
    Brightness { value: i32 },
    Contrast { value: i32 },
    Exposure { value: f32 },
    Gamma { value: f32 },
    Thumbnail { max_size: u32 },
    CropResize { x: u32, y: u32, width: u32, height: u32, target_width: u32, target_height: u32 },
}

impl Operation {
    fn to_type_id(&self) -> u8 {
        match self {
            Operation::Crop { .. } => 0,
            Operation::Resize { .. } => 1,
            Operation::ResizeAspect { .. } => 2,
            Operation::Brightness { .. } => 3,
            Operation::Contrast { .. } => 4,
            Operation::Exposure { .. } => 5,
            Operation::Gamma { .. } => 6,
            Operation::Thumbnail { .. } => 7,
            Operation::CropResize { .. } => 8,
        }
    }

    fn to_params_bytes(&self) -> Vec<u8> {
        match self {
            Operation::Crop { x, y, width, height } => {
                let mut bytes = vec![];
                bytes.extend_from_slice(&x.to_le_bytes());
                bytes.extend_from_slice(&y.to_le_bytes());
                bytes.extend_from_slice(&width.to_le_bytes());
                bytes.extend_from_slice(&height.to_le_bytes());
                bytes
            }
            Operation::Resize { width, height } => {
                let mut bytes = vec![];
                bytes.extend_from_slice(&width.to_le_bytes());
                bytes.extend_from_slice(&height.to_le_bytes());
                bytes
            }
            Operation::ResizeAspect { max_width, max_height } => {
                let mut bytes = vec![];
                bytes.extend_from_slice(&max_width.to_le_bytes());
                bytes.extend_from_slice(&max_height.to_le_bytes());
                bytes
            }
            Operation::Brightness { value } => value.to_le_bytes().to_vec(),
            Operation::Contrast { value } => value.to_le_bytes().to_vec(),
            Operation::Exposure { value } => value.to_le_bytes().to_vec(),
            Operation::Gamma { value } => value.to_le_bytes().to_vec(),
            Operation::Thumbnail { max_size } => max_size.to_le_bytes().to_vec(),
            Operation::CropResize { x, y, width, height, target_width, target_height } => {
                let mut bytes = vec![];
                bytes.extend_from_slice(&x.to_le_bytes());
                bytes.extend_from_slice(&y.to_le_bytes());
                bytes.extend_from_slice(&width.to_le_bytes());
                bytes.extend_from_slice(&height.to_le_bytes());
                bytes.extend_from_slice(&target_width.to_le_bytes());
                bytes.extend_from_slice(&target_height.to_le_bytes());
                bytes
            }
        }
    }

    fn from_str(s: &str) -> Option<Self> {
        let parts: Vec<&str> = s.split(',').collect();
        if parts.is_empty() {
            return None;
        }

        match parts[0] {
            "crop" if parts.len() == 5 => Some(Operation::Crop {
                x: parts[1].parse().ok()?,
                y: parts[2].parse().ok()?,
                width: parts[3].parse().ok()?,
                height: parts[4].parse().ok()?,
            }),
            "resize" if parts.len() == 3 => Some(Operation::Resize {
                width: parts[1].parse().unwrap_or(800),
                height: parts[2].parse().unwrap_or(600),
            }),
            "resize_aspect" if parts.len() == 3 => Some(Operation::ResizeAspect {
                max_width: parts[1].parse().unwrap_or(800),
                max_height: parts[2].parse().unwrap_or(600),
            }),
            "brightness" if parts.len() == 2 => Some(Operation::Brightness {
                value: parts[1].parse().ok()?,
            }),
            "contrast" if parts.len() == 2 => Some(Operation::Contrast {
                value: parts[1].parse().ok()?,
            }),
            "exposure" if parts.len() == 2 => Some(Operation::Exposure {
                value: parts[1].parse().ok()?,
            }),
            "gamma" if parts.len() == 2 => Some(Operation::Gamma {
                value: parts[1].parse().ok()?,
            }),
            "thumbnail" if parts.len() == 2 => Some(Operation::Thumbnail {
                max_size: parts[1].parse().ok()?,
            }),
            "crop_resize" if parts.len() == 7 => Some(Operation::CropResize {
                x: parts[1].parse().ok()?,
                y: parts[2].parse().ok()?,
                width: parts[3].parse().ok()?,
                height: parts[4].parse().ok()?,
                target_width: parts[5].parse().ok()?,
                target_height: parts[6].parse().ok()?,
            }),
            _ => None,
        }
    }
}

/// JSON input format (for server integration)
#[derive(Debug, Deserialize)]
struct JsonInput {
    /// Original image path
    image: String,
    /// Operations to apply
    operations: Vec<Operation>,
    /// Output path
    output: Option<String>,
}

/// JSON output format
#[derive(Debug, Serialize)]
struct JsonOutput {
    /// Whether C2PA manifest was present
    has_manifest: bool,
    /// Whether certificate chain was verified
    cert_chain_verified: bool,
    /// Final image hash (hex)
    final_image_hash: String,
    /// Number of operations
    num_operations: u32,
    /// Proof data (base64 encoded)
    proof: Option<String>,
    /// Public values (hex)
    public_values: Option<String>,
    /// Error message if any
    error: Option<String>,
}

/// Apply an operation to an image
fn apply_operation(input_path: &str, operation: &Operation, output_path: &str) -> Result<String, String> {
    match operation {
        Operation::Crop { x, y, width, height } => {
            let region = CropRegion { x: *x, y: *y, width: *width, height: *height };
            image_editor::crop_image(input_path, region, output_path)?;
            Ok(output_path.to_string())
        }
        Operation::Resize { width, height } => {
            let options = ResizeOptions {
                width: *width,
                height: *height,
                filter: image_editor::ResizeFilter::Triangle,
            };
            image_editor::resize_image(input_path, options, output_path)?;
            Ok(output_path.to_string())
        }
        Operation::ResizeAspect { max_width, max_height } => {
            image_editor::resize_with_aspect_ratio(
                input_path, *max_width, *max_height,
                image_editor::ResizeFilter::Triangle, output_path)?;
            Ok(output_path.to_string())
        }
        Operation::Brightness { value } => {
            image_editor::adjust_brightness(input_path, *value, output_path)?;
            Ok(output_path.to_string())
        }
        Operation::Contrast { value } => {
            image_editor::adjust_contrast(input_path, *value, output_path)?;
            Ok(output_path.to_string())
        }
        Operation::Exposure { value } => {
            image_editor::adjust_exposure(input_path, *value, output_path)?;
            Ok(output_path.to_string())
        }
        Operation::Gamma { value } => {
            image_editor::adjust_gamma(input_path, *value, output_path)?;
            Ok(output_path.to_string())
        }
        Operation::Thumbnail { max_size } => {
            image_editor::create_thumbnail(input_path, *max_size, output_path)?;
            Ok(output_path.to_string())
        }
        Operation::CropResize { x, y, width, height, target_width, target_height } => {
            let region = CropRegion { x: *x, y: *y, width: *width, height: *height };
            let options = ResizeOptions {
                width: *target_width,
                height: *target_height,
                filter: image_editor::ResizeFilter::Triangle,
            };
            image_editor::crop_and_resize(input_path, region, options, output_path)?;
            Ok(output_path.to_string())
        }
    }
}

/// Compute SHA256 hash of a file
fn compute_file_hash(path: &str) -> Result<[u8; 32], String> {
    let data = std::fs::read(path).map_err(|e| format!("Failed to read file: {}", e))?;
    let mut hasher = Sha256::new();
    hasher.update(&data);
    let result = hasher.finalize();
    let mut hash = [0u8; 32];
    hash.copy_from_slice(&result);
    Ok(hash)
}

/// Generate ZK proof
fn generate_proof(
    image_path: &str,
    operations: &[Operation],
    output_path: &str,
    elf_path: &str,
    verbose: bool,
) -> Result<JsonOutput, String> {
    // Load ELF
    if verbose {
        println!("Loading ELF from: {}", elf_path);
    }
    let elf = load_elf(elf_path);

    // Extract C2PA from original image
    if verbose {
        println!("Extracting C2PA data from: {}", image_path);
    }
    let c2pa_result = extract_c2pa_from_jpeg(image_path);
    let has_manifest = c2pa_result.has_manifest;
    let metadata = c2pa_result.metadata;

    if verbose {
        println!("Has C2PA manifest: {}", has_manifest);
    }

    // Apply image operations
    if verbose {
        println!("Applying {} operations...", operations.len());
    }

    for (i, op) in operations.iter().enumerate() {
        let input_path = if i == 0 {
            image_path.to_string()
        } else {
            format!("{}_temp_{}.jpg", Path::new(image_path).file_stem().unwrap_or_default().to_string_lossy(), i)
        };

        let output_path_i = if i == operations.len() - 1 {
            output_path.to_string()
        } else {
            format!("{}_temp_{}.jpg", Path::new(image_path).file_stem().unwrap_or_default().to_string_lossy(), i + 1)
        };

        apply_operation(&input_path, op, &output_path_i).map_err(|e| format!("Failed to apply operation {}: {}", i, e))?;
    }

    // Compute final image hash
    let final_image_hash = compute_file_hash(output_path)?;
    if verbose {
        println!("Final image hash: {:02x?}", &final_image_hash[..8]);
    }

    // Clean up temp files
    for i in 1..operations.len() {
        let temp = format!("{}_temp_{}.jpg", Path::new(image_path).file_stem().unwrap_or_default().to_string_lossy(), i);
        let _ = std::fs::remove_file(&temp);
    }

    // Generate ZK proof
    if verbose {
        println!("Generating ZK proof...");
    }

    let (proof_bytes, public_values_hex) = if has_manifest {
        let m = metadata.as_ref().unwrap();

        if verbose {
            println!("Certificate chain present: {}", m.certificate_chain.is_some());
        }

        // Verify C2PA claim
        let claim_valid = verify_c2pa_claim(&m);
        if verbose {
            println!("C2PA claim verification: {}", claim_valid);
        }

        // Load and downscale image for zkVM
        let original_img = image::open(image_path).map_err(|e| format!("Failed to open image: {}", e))?;
        let (img_width, img_height) = original_img.dimensions();

        const MAX_ZKVM_SIZE: u32 = 200;
        let (zkvm_width, zkvm_height) = if img_width > MAX_ZKVM_SIZE || img_height > MAX_ZKVM_SIZE {
            let ratio = (MAX_ZKVM_SIZE as f32 / img_width as f32).min(MAX_ZKVM_SIZE as f32 / img_height as f32);
            let new_width = (img_width as f32 * ratio) as u32;
            let new_height = (img_height as f32 * ratio) as u32;
            if verbose {
                println!("Downscaling to {}x{} for zkVM", new_width, new_height);
            }
            (new_width, new_height)
        } else {
            (img_width, img_height)
        };

        let zkvm_img = original_img.resize_exact(
            zkvm_width,
            zkvm_height,
            image::imageops::FilterType::Triangle,
        );
        let rgba_img = zkvm_img.to_rgba8();
        let pixel_data = rgba_img.into_raw();

        // Initialize prover
        let client = DefaultProverClient::new(&elf);
        let mut stdin = client.new_stdin_builder();

        // Write has_manifest flag
        stdin.write(&(1u8));

        // Write cert_chain_verified flag
        let cert_verified = if m.certificate_chain.is_some() { 1u8 } else { 0u8 };
        stdin.write(&cert_verified);

        // Write issuer
        stdin.write_slice(&m.issuer);

        // Write timestamp
        stdin.write(&m.timestamp);

        // Write signature
        stdin.write_slice(&m.signature);

        // Write claim hash
        stdin.write_slice(&m.claim_hash);

        // Write zkvm image dimensions
        stdin.write(&zkvm_width);
        stdin.write(&zkvm_height);

        // Write pixel data
        stdin.write_slice(&pixel_data);

        // Write number of operations
        let num_ops = operations.len() as u32;
        stdin.write(&num_ops);

        // Write operations
        for op in operations {
            stdin.write(&(op.to_type_id()));
            stdin.write_slice(&op.to_params_bytes());
        }

        // Write final image hash
        stdin.write_slice(&final_image_hash);

        // Generate proof
        let proof = client.prove_fast(stdin).map_err(|e| format!("Failed to generate proof: {}", e))?;

        // Get public values
        let public_buffer = proof.pv_stream.clone().unwrap();
        let public_values = PublicValuesStruct::abi_decode(&public_buffer, false).unwrap();

        if verbose {
            println!("Proof generated successfully");
            println!("Final image hash: {:02x?}", &public_values.final_image_hash[..8]);
            println!("Number of operations: {}", public_values.num_operations);
        }

        // Serialize proof
        let proof_bytes = bincode::serialize(&proof).map_err(|e| format!("Failed to serialize proof: {}", e))?;

        (Some(proof_bytes), hex::encode(&public_buffer))
    } else {
        // No manifest case - generate proof with invalid inputs
        let original_img = image::open(image_path).map_err(|e| format!("Failed to open image: {}", e))?;
        let (img_width, img_height) = original_img.dimensions();

        const MAX_ZKVM_SIZE: u32 = 200;
        let (zkvm_width, zkvm_height) = if img_width > MAX_ZKVM_SIZE || img_height > MAX_ZKVM_SIZE {
            let ratio = (MAX_ZKVM_SIZE as f32 / img_width as f32).min(MAX_ZKVM_SIZE as f32 / img_height as f32);
            let new_width = (img_width as f32 * ratio) as u32;
            let new_height = (img_height as f32 * ratio) as u32;
            (new_width, new_height)
        } else {
            (img_width, img_height)
        };

        let zkvm_img = original_img.resize_exact(
            zkvm_width,
            zkvm_height,
            image::imageops::FilterType::Triangle,
        );
        let rgba_img = zkvm_img.to_rgba8();
        let pixel_data = rgba_img.into_raw();

        let client = DefaultProverClient::new(&elf);
        let mut stdin = client.new_stdin_builder();

        // Write has_manifest = 0
        stdin.write(&(0u8));

        // Write issuer (zeros)
        stdin.write_slice(&[0u8; 32]);

        // Write timestamp (0)
        stdin.write(&(0u64));

        // Write signature (zeros)
        stdin.write_slice(&[0u8; 64]);

        // Write claim hash (zeros)
        stdin.write_slice(&[0u8; 32]);

        // Write dimensions
        stdin.write(&zkvm_width);
        stdin.write(&zkvm_height);

        // Write pixel data
        stdin.write_slice(&pixel_data);

        // Write number of operations (0)
        stdin.write(&(0u32));

        // Write empty final image hash
        stdin.write_slice(&[0u8; 32]);

        // Generate proof
        let proof = client.prove_fast(stdin).map_err(|e| format!("Failed to generate proof: {}", e))?;

        // Get public values
        let public_buffer = proof.pv_stream.clone().unwrap();

        // Serialize proof
        let proof_bytes = bincode::serialize(&proof).map_err(|e| format!("Failed to serialize proof: {}", e))?;

        (Some(proof_bytes), hex::encode(&public_buffer))
    };

    let cert_chain_verified = has_manifest && metadata.as_ref().map(|m| m.certificate_chain.is_some()).unwrap_or(false);

    Ok(JsonOutput {
        has_manifest,
        cert_chain_verified,
        final_image_hash: hex::encode(final_image_hash),
        num_operations: operations.len() as u32,
        proof: proof_bytes.map(|b| base64_encode(&b)),
        public_values: Some(public_values_hex),
        error: None,
    })
}

fn base64_encode(data: &[u8]) -> String {
    const CHARSET: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut result = String::new();

    for chunk in data.chunks(3) {
        let b0 = chunk[0] as usize;
        let b1 = chunk.get(1).copied().unwrap_or(0) as usize;
        let b2 = chunk.get(2).copied().unwrap_or(0) as usize;

        result.push(CHARSET[b0 >> 2] as char);
        result.push(CHARSET[((b0 & 0x03) << 4) | (b1 >> 4)] as char);

        if chunk.len() > 1 {
            result.push(CHARSET[((b1 & 0x0f) << 2) | (b2 >> 6)] as char);
        } else {
            result.push('=');
        }

        if chunk.len() > 2 {
            result.push(CHARSET[b2 & 0x3f] as char);
        } else {
            result.push('=');
        }
    }

    result
}

fn main() {
    // Initialize logger
    let _ = init_logger();

    let args = Args::parse();

    // Determine elf path
    let elf_path = args.elf_path.unwrap_or_else(|| "image-processor/app/elf/riscv32im-pico-zkvm-elf".to_string());

    // Parse input
    let (image_path, operations, output_path) = if let Some(proof_id) = args.proof_id {
        // Load from server's proofs directory
        let proofs_dir = Path::new("proofs");
        let proof_dir = proofs_dir.join(&proof_id);

        let json_path = proof_dir.join("cli_input.json");
        if !json_path.exists() {
            eprintln!("Error: Proof data not found at {:?}", proof_dir);
            eprintln!("Make sure the server has processed this proof ID: {}", proof_id);
            std::process::exit(1);
        }

        let json_content = std::fs::read_to_string(&json_path)
            .map_err(|e| format!("Failed to read CLI input: {}", e)).unwrap();
        let input: JsonInput = serde_json::from_str(&json_content)
            .map_err(|e| format!("Failed to parse CLI input: {}", e)).unwrap();

        // Use absolute path for the image
        let image_path = proof_dir.join(&input.image);

        (image_path.to_string_lossy().to_string(), input.operations, input.output.unwrap_or_else(|| "output.jpg".to_string()))
    } else if let Some(json_input) = args.json_input {
        // Read JSON input file
        let json_content = std::fs::read_to_string(&json_input)
            .map_err(|e| format!("Failed to read JSON input: {}", e)).unwrap();
        let input: JsonInput = serde_json::from_str(&json_content)
            .map_err(|e| format!("Failed to parse JSON input: {}", e)).unwrap();

        (
            input.image,
            input.operations,
            input.output.unwrap_or_else(|| "output.jpg".to_string()),
        )
    } else if let Some(image) = args.image {
        // Parse operations from command line
        let operations: Vec<Operation> = args.op.iter()
            .filter_map(|s| Operation::from_str(s))
            .collect();

        if operations.is_empty() {
            eprintln!("Error: No operations specified. Use --op flag or --json-input");
            std::process::exit(1);
        }

        (image, operations, args.output)
    } else {
        // Show usage
        println!("Usage: zk-proof-cli [OPTIONS]");
        println!();
        println!("Options:");
        println!("  --image <path>          Input image path");
        println!("  --op <operation>        Operation to apply (can be specified multiple times)");
        println!("  --output <path>        Output path (default: output.jpg)");
        println!("  --json-input <path>    JSON input file (alternative to --image and --op)");
        println!("  --json-output <path>   JSON output file (prints to stdout if not specified)");
        println!("  --elf-path <path>      ELF file path (default: image-processor/app/elf/riscv32im-pico-zkvm-elf)");
        println!("  --verbose              Verbose output");
        println!("  --proof-id <id>       Load from server proofs directory");
        println!();
        println!("Operations:");
        println!("  crop,x,y,width,height");
        println!("  resize,width,height");
        println!("  resize_aspect,max_width,max_height");
        println!("  brightness,value");
        println!("  contrast,value");
        println!("  exposure,value");
        println!("  gamma,value");
        println!("  thumbnail,max_size");
        println!("  crop_resize,x,y,width,height,target_width,target_height");
        println!();
        println!("Example (command line):");
        println!("  zk-proof-cli --image photo.jpg --op resize,800,600 --op brightness,10 -o output.jpg");
        println!();
        println!("Example (JSON input):");
        println!("  zk-proof-cli --json-input input.json --json-output output.json");
        println!();
        println!("Example (from server proof ID):");
        println!("  zk-proof-cli --proof-id <uuid> --verbose");
        println!();
        println!("JSON input format:");
        println!(r#"{{
  "image": "photo.jpg",
  "operations": [
    {{ "type": "Resize", "width": 800, "height": 600 }},
    {{ "type": "Brightness", "value": 10 }}
  ],
  "output": "output.jpg"
}}"#);
        std::process::exit(1);
    };

    // Generate proof
    let result = generate_proof(&image_path, &operations, &output_path, &elf_path, args.verbose);

    // Output result
    let json_output = match result {
        Ok(output) => serde_json::to_string_pretty(&output).unwrap(),
        Err(e) => {
            let error_output = JsonOutput {
                has_manifest: false,
                cert_chain_verified: false,
                final_image_hash: String::new(),
                num_operations: 0,
                proof: None,
                public_values: None,
                error: Some(e),
            };
            serde_json::to_string_pretty(&error_output).unwrap()
        }
    };

    if let Some(output_path) = args.json_output {
        std::fs::write(&output_path, &json_output).unwrap();
        println!("Proof data written to: {}", output_path);
    } else {
        println!("{}", json_output);
    }
}
