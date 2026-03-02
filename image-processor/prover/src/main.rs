use alloy_sol_types::SolType;
use c2pa_lib::{extract_c2pa_from_jpeg, load_elf, verify_c2pa_claim, PublicValuesStruct};
use image_editor::{CropRegion, ResizeOptions};
use pico_sdk::{client::DefaultProverClient, init_logger};
use sha2::{Digest, Sha256};
use std::env;
use std::path::Path;

/// Operation type enum
#[derive(Debug, Clone)]
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

    /// Serialize params to bytes (for private input)
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
            Operation::Brightness { value } => {
                value.to_le_bytes().to_vec()
            }
            Operation::Contrast { value } => {
                value.to_le_bytes().to_vec()
            }
            Operation::Exposure { value } => {
                value.to_le_bytes().to_vec()
            }
            Operation::Gamma { value } => {
                value.to_le_bytes().to_vec()
            }
            Operation::Thumbnail { max_size } => {
                max_size.to_le_bytes().to_vec()
            }
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
}

/// Apply an operation to an image and return the output path
fn apply_operation(input_path: &str, operation: &Operation, output_path: &str) -> Result<String, String> {
    match operation {
        Operation::Crop { x, y, width, height } => {
            let region = CropRegion { x: *x, y: *y, width: *width, height: *height };
            let result = image_editor::crop_image(input_path, region, output_path)?;
            println!("  Crop: {}x{} -> {}x{}",
                result.original_width, result.original_height, result.new_width, result.new_height);
        }
        Operation::Resize { width, height } => {
            let options = ResizeOptions {
                width: *width,
                height: *height,
                filter: image_editor::ResizeFilter::Triangle,
            };
            let result = image_editor::resize_image(input_path, options, output_path)?;
            println!("  Resize: {}x{} -> {}x{}",
                result.original_width, result.original_height, result.new_width, result.new_height);
        }
        Operation::ResizeAspect { max_width, max_height } => {
            let result = image_editor::resize_with_aspect_ratio(
                input_path, *max_width, *max_height,
                image_editor::ResizeFilter::Triangle, output_path)?;
            println!("  ResizeAspect: {}x{} -> {}x{}",
                result.original_width, result.original_height, result.new_width, result.new_height);
        }
        Operation::Brightness { value } => {
            let result = image_editor::adjust_brightness(input_path, *value, output_path)?;
            println!("  Brightness: {} -> {}x{}",
                value, result.new_width, result.new_height);
        }
        Operation::Contrast { value } => {
            let result = image_editor::adjust_contrast(input_path, *value, output_path)?;
            println!("  Contrast: {} -> {}x{}",
                value, result.new_width, result.new_height);
        }
        Operation::Exposure { value } => {
            let result = image_editor::adjust_exposure(input_path, *value, output_path)?;
            println!("  Exposure: {} -> {}x{}",
                value, result.new_width, result.new_height);
        }
        Operation::Gamma { value } => {
            let result = image_editor::adjust_gamma(input_path, *value, output_path)?;
            println!("  Gamma: {} -> {}x{}",
                value, result.new_width, result.new_height);
        }
        Operation::Thumbnail { max_size } => {
            let result = image_editor::create_thumbnail(input_path, *max_size, output_path)?;
            println!("  Thumbnail: {}x{} -> {}x{}",
                result.original_width, result.original_height, result.new_width, result.new_height);
        }
        Operation::CropResize { x, y, width, height, target_width, target_height } => {
            let region = CropRegion { x: *x, y: *y, width: *width, height: *height };
            let options = ResizeOptions {
                width: *target_width,
                height: *target_height,
                filter: image_editor::ResizeFilter::Triangle,
            };
            let result = image_editor::crop_and_resize(input_path, region, options, output_path)?;
            println!("  CropResize: {}x{} -> {}x{}",
                result.original_width, result.original_height, result.new_width, result.new_height);
        }
    }
    Ok(output_path.to_string())
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

/// Compute semantic hash (same as app-lib compute_image_edit_hash)
/// This must match the zkVM's hash computation
fn compute_semantic_hash(original_hash: &[u8; 32], operations: &[Operation]) -> [u8; 32] {
    let mut hash = *original_hash;

    for op in operations {
        // Mix the operation type
        hash[0] = hash[0].wrapping_add(op.to_type_id());

        // Mix the params
        let params = op.to_params_bytes();
        for (i, &param) in params.iter().enumerate() {
            if i < 32 {
                hash[i] = hash[i].wrapping_add(param);
            }
        }

        // Rotate and mix to make it order-dependent
        let rot = (hash[0] as usize) % 32;
        let mut rotated = [0u8; 32];
        for i in 0..32 {
            rotated[i] = hash[(i + rot) % 32];
        }
        hash = rotated;
    }

    hash
}

fn main() {
    // Initialize logger
    init_logger();

    // Get arguments from command line
    let args: Vec<String> = env::args().collect();

    // Parse arguments
    let mut image_path = String::new();
    let mut operations: Vec<Operation> = Vec::new();
    let mut output_path = String::new();

    let mut i = 1;
    while i < args.len() {
        match args[i].as_str() {
            "--op" => {
                // Parse operation: --op type,param1,param2,...
                if i + 1 < args.len() {
                    let op_str = &args[i + 1];
                    let parts: Vec<&str> = op_str.split(',').collect();
                    if parts.is_empty() {
                        eprintln!("Invalid operation format");
                        i += 1;
                        continue;
                    }

                    let op = match parts[0] {
                        "crop" if parts.len() == 5 => Operation::Crop {
                            x: parts[1].parse().unwrap_or(0),
                            y: parts[2].parse().unwrap_or(0),
                            width: parts[3].parse().unwrap_or(0),
                            height: parts[4].parse().unwrap_or(0),
                        },
                        "resize" if parts.len() == 3 => Operation::Resize {
                            width: parts[1].parse().unwrap_or(800),
                            height: parts[2].parse().unwrap_or(600),
                        },
                        "resize_aspect" if parts.len() == 3 => Operation::ResizeAspect {
                            max_width: parts[1].parse().unwrap_or(800),
                            max_height: parts[2].parse().unwrap_or(600),
                        },
                        "brightness" if parts.len() == 2 => Operation::Brightness {
                            value: parts[1].parse().unwrap_or(0),
                        },
                        "contrast" if parts.len() == 2 => Operation::Contrast {
                            value: parts[1].parse().unwrap_or(0),
                        },
                        "exposure" if parts.len() == 2 => Operation::Exposure {
                            value: parts[1].parse().unwrap_or(0.0),
                        },
                        "gamma" if parts.len() == 2 => Operation::Gamma {
                            value: parts[1].parse().unwrap_or(1.0),
                        },
                        "thumbnail" if parts.len() == 2 => Operation::Thumbnail {
                            max_size: parts[1].parse().unwrap_or(200),
                        },
                        "crop_resize" if parts.len() == 7 => Operation::CropResize {
                            x: parts[1].parse().unwrap_or(0),
                            y: parts[2].parse().unwrap_or(0),
                            width: parts[3].parse().unwrap_or(0),
                            height: parts[4].parse().unwrap_or(0),
                            target_width: parts[5].parse().unwrap_or(800),
                            target_height: parts[6].parse().unwrap_or(600),
                        },
                        _ => {
                            eprintln!("Unknown operation: {} or invalid params", parts[0]);
                            i += 1;
                            continue;
                        }
                    };
                    operations.push(op);
                    i += 2;
                } else {
                    i += 1;
                }
            }
            "--output" | "-o" => {
                if i + 1 < args.len() {
                    output_path = args[i + 1].clone();
                    i += 2;
                } else {
                    i += 1;
                }
            }
            _ => {
                if !args[i].starts_with('-') && image_path.is_empty() {
                    image_path = args[i].clone();
                }
                i += 1;
            }
        }
    }

    if image_path.is_empty() {
        println!("Usage: cargo run --release -- <image_path> [options]");
        println!("Options:");
        println!("  --op <op>        : Add an operation (can be specified multiple times)");
        println!("  --output <path>  : Output path for the final image");
        println!("");
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
        println!("");
        println!("Example:");
        println!("  cargo run --release -- DSC00050.JPG --op crop,100,100,800,600 --op resize,400,300 -o output.jpg");
        image_path = "DSC00050.JPG".to_string();
    }

    // Default output path
    if output_path.is_empty() {
        output_path = format!("{}_edited.jpg", Path::new(&image_path).file_stem().unwrap_or_default().to_string_lossy());
    }

    println!("Input image: {}", image_path);
    println!("Output image: {}", output_path);
    println!("Number of operations: {}", operations.len());
    for (i, op) in operations.iter().enumerate() {
        println!("  Operation {}: {:?}", i + 1, op);
    }

    // =========================================================================
    // Step 1: Extract C2PA from original image
    // =========================================================================
    let elf = load_elf("image-processor/app/elf/riscv32im-pico-zkvm-elf");
    println!("ELF length: {}", elf.len());

    println!("\n[1/3] Extracting C2PA data from original image...");
    let c2pa_result = extract_c2pa_from_jpeg(&image_path);
    println!("Has C2PA manifest: {}", c2pa_result.has_manifest);

    let has_manifest = c2pa_result.has_manifest;
    let metadata = c2pa_result.metadata;

    // =========================================================================
    // Step 2: Apply image operations (in prover)
    // =========================================================================
    println!("\n[2/3] Applying image operations...");

    // Apply operations sequentially
    for (i, op) in operations.iter().enumerate() {
        let input_path = if i == 0 {
            image_path.clone()
        } else {
            format!("{}_temp_{}.jpg", Path::new(&image_path).file_stem().unwrap_or_default().to_string_lossy(), i)
        };

        let output_path_i = if i == operations.len() - 1 {
            output_path.clone()
        } else {
            format!("{}_temp_{}.jpg", Path::new(&image_path).file_stem().unwrap_or_default().to_string_lossy(), i + 1)
        };

        apply_operation(&input_path, op, &output_path_i).expect("Failed to apply operation");
    }

    // Get original image hash from C2PA metadata
    let original_image_hash = if has_manifest {
        metadata.as_ref().unwrap().claim_hash
    } else {
        [0u8; 32]
    };

    // Compute file hash of the final processed image
    // This is the actual SHA256 hash of the output image
    let final_image_hash = compute_file_hash(&output_path).expect("Failed to compute file hash");
    println!("Final image hash (file): {:02x?}", &final_image_hash[..8]);

    // Clean up temp files
    for i in 1..operations.len() {
        let temp = format!("{}_temp_{}.jpg", Path::new(&image_path).file_stem().unwrap_or_default().to_string_lossy(), i);
        let _ = std::fs::remove_file(&temp);
    }

    // =========================================================================
    // Step 3: Generate ZK proof
    // =========================================================================
    println!("\n[3/3] Generating ZK proof...");

    if has_manifest {
        let m = metadata.unwrap();
        println!("Certificate chain present: {}", m.certificate_chain.is_some());

        // Verify the complete C2PA claim including certificate chain
        let claim_valid = verify_c2pa_claim(&m);
        println!("C2PA claim verification (including cert chain): {}", claim_valid);

        // Initialize prover
        let client = DefaultProverClient::new(&elf);
        let mut stdin = client.new_stdin_builder();

        // Write has_manifest flag
        stdin.write(&(1u8));

        // Write cert_chain_verified flag
        let cert_verified = if m.certificate_chain.is_some() { 1u8 } else { 0u8 };
        stdin.write(&cert_verified);

        // Write issuer (32 bytes)
        stdin.write_slice(&m.issuer);

        // Write timestamp
        stdin.write(&m.timestamp);

        // Write signature (64 bytes)
        stdin.write_slice(&m.signature);

        // Write claim hash (32 bytes) - this is the original image hash from C2PA
        stdin.write_slice(&m.claim_hash);

        // Write number of operations
        let num_ops = operations.len() as u32;
        stdin.write(&num_ops);

        // Write each operation (type + params)
        for op in &operations {
            stdin.write(&(op.to_type_id()));
            stdin.write_slice(&op.to_params_bytes());
        }

        // Write final image hash (computed from file)
        let final_image_hash = compute_file_hash(&output_path).expect("Failed to compute file hash");
        println!("Final image hash (file): {:02x?}", &final_image_hash[..8]);
        stdin.write_slice(&final_image_hash);

        // Generate proof
        let proof = client.prove_fast(stdin).expect("Failed to generate proof");

        // Get public values
        let public_buffer = proof.pv_stream.clone().unwrap();
        let public_values = PublicValuesStruct::abi_decode(&public_buffer, false).unwrap();

        println!("\n=== Proof Generated! ===");
        println!("Final image hash: {:02x?}", &public_values.final_image_hash[..8]);
        println!("Number of operations: {}", public_values.num_operations);

        // Save proof data
        save_proof(&proof, &public_buffer);
    } else {
        println!("No C2PA manifest - generating proof for INVALID case");

        let client = DefaultProverClient::new(&elf);
        let mut stdin = client.new_stdin_builder();

        // Write has_manifest = 0 (false)
        stdin.write(&(0u8));

        // Write empty claim hash for invalid case
        stdin.write_slice(&[0u8; 32]);

        // Write number of operations (0 for invalid case)
        stdin.write(&(0u32));

        // Write empty final image hash for invalid case
        stdin.write_slice(&[0u8; 32]);

        // Generate proof
        let proof = client.prove_fast(stdin).expect("Failed to generate proof");

        // Get public values
        let public_buffer = proof.pv_stream.clone().unwrap();
        let public_values = PublicValuesStruct::abi_decode(&public_buffer, false).unwrap();

        println!("\n=== Proof Generated! ===");
        println!("Final image hash: {:02x?}", &public_values.final_image_hash[..8]);
        println!("Number of operations: {}", public_values.num_operations);

        // Save proof data
        save_proof(&proof, &public_buffer);
    }

    println!("\n=== C2PA Image Edit ZK Proof Generated! ===");
    println!("Output saved to: {}", output_path);
}

fn save_proof<P>(proof: &P, public_buffer: &Vec<u8>)
where
    P: serde::Serialize,
{
    let output_dir = Path::new("proof_data");
    std::fs::create_dir_all(output_dir).unwrap();

    // Save proof
    let proof_bytes = bincode::serialize(proof).expect("Failed to serialize proof");
    std::fs::write(output_dir.join("c2pa_proof.bin"), &proof_bytes).expect("Failed to write proof");

    // Save public values
    std::fs::write(output_dir.join("c2pa_public_values.bin"), public_buffer)
        .expect("Failed to write public values");
}
