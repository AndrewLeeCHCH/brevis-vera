use axum::{
    extract::{DefaultBodyLimit, Multipart},
    http::StatusCode,
    response::Json,
    routing::{get, post},
    Router,
};
use serde::{Deserialize, Serialize};
use std::net::SocketAddr;
use std::path::{Path, PathBuf};
use tower_http::cors::{Any, CorsLayer};
use tracing::{error, info};
use uuid::Uuid;

// ============================================================================
// Data Types
// ============================================================================

/// Operation from the frontend JSON
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum OperationType {
    Crop,
    Resize,
    ResizeAspect,
    Brightness,
    Contrast,
    Exposure,
    Gamma,
    Thumbnail,
    CropResize,
}

/// Single operation from frontend
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct Operation {
    #[serde(rename = "type")]
    pub op_type: OperationType,
    pub params: String,
}

/// Proof request JSON from frontend
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ProofRequest {
    pub timestamp: String,
    #[serde(rename = "original_image")]
    pub original_image: String,
    #[serde(rename = "output_image")]
    pub output_image: String,
    pub operations: Vec<Operation>,
    #[serde(rename = "num_operations")]
    pub num_operations: u32,
    #[serde(rename = "original_image_hash")]
    pub original_image_hash: String,
    #[serde(rename = "final_image_hash")]
    pub final_image_hash: String,
}

/// Proof response to frontend
#[derive(Debug, Clone, Serialize)]
pub struct ProofResponse {
    pub success: bool,
    pub message: String,
    pub proof_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub final_image_hash: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub num_operations: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub storage_path: Option<String>,
}

/// Preview response with processed image
#[derive(Debug, Clone, Serialize)]
pub struct PreviewResponse {
    pub success: bool,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub image: Option<String>, // Base64 encoded
    #[serde(skip_serializing_if = "Option::is_none")]
    pub image_hash: Option<String>,
}

// ============================================================================
// Storage Functions
// ============================================================================

/// Get the proofs storage directory path
fn get_proofs_dir() -> PathBuf {
    let exe_path = std::env::current_exe().unwrap_or_default();
    let server_dir = exe_path.parent().unwrap_or(Path::new("."));
    let proofs_dir = server_dir.join("proofs");
    std::fs::create_dir_all(&proofs_dir).ok();
    proofs_dir
}

/// CLI-compatible operation format
#[derive(serde::Serialize)]
#[serde(tag = "type")]
enum CliOperation {
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

/// Convert server operation to CLI operation
fn convert_operation(op: &Operation) -> Option<CliOperation> {
    let params: Vec<&str> = op.params.split(',').collect();

    // Skip the first param if it matches the operation type name (frontend sends "type,value")
    let skip_first = params.first()
        .map(|p| p.eq_ignore_ascii_case(&format!("{:?}", op.op_type)))
        .unwrap_or(false);
    let values: Vec<&str> = if skip_first && params.len() > 1 {
        params[1..].to_vec()
    } else {
        params.clone()
    };

    match op.op_type {
        OperationType::Crop if values.len() >= 4 => Some(CliOperation::Crop {
            x: values[0].parse().ok()?,
            y: values[1].parse().ok()?,
            width: values[2].parse().ok()?,
            height: values[3].parse().ok()?,
        }),
        OperationType::Resize if values.len() >= 2 => Some(CliOperation::Resize {
            width: values[0].parse().unwrap_or(800),
            height: values[1].parse().unwrap_or(600),
        }),
        OperationType::ResizeAspect if values.len() >= 2 => Some(CliOperation::ResizeAspect {
            max_width: values[0].parse().unwrap_or(800),
            max_height: values[1].parse().unwrap_or(600),
        }),
        OperationType::Brightness if !values.is_empty() => Some(CliOperation::Brightness {
            value: values[0].parse().ok()?,
        }),
        OperationType::Contrast if !values.is_empty() => Some(CliOperation::Contrast {
            value: values[0].parse().ok()?,
        }),
        OperationType::Exposure if !values.is_empty() => Some(CliOperation::Exposure {
            value: values[0].parse().ok()?,
        }),
        OperationType::Gamma if !values.is_empty() => Some(CliOperation::Gamma {
            value: values[0].parse().ok()?,
        }),
        OperationType::Thumbnail if !values.is_empty() => Some(CliOperation::Thumbnail {
            max_size: values[0].parse().ok()?,
        }),
        OperationType::CropResize if values.len() >= 6 => Some(CliOperation::CropResize {
            x: values[0].parse().ok()?,
            y: values[1].parse().ok()?,
            width: values[2].parse().ok()?,
            height: values[3].parse().ok()?,
            target_width: values[4].parse().ok()?,
            target_height: values[5].parse().ok()?,
        }),
        _ => None,
    }
}

/// CLI-compatible JSON input format
#[derive(serde::Serialize)]
struct CliJsonInput {
    image: String,
    operations: Vec<CliOperation>,
    output: String,
}

/// Save proof data persistently for later CLI processing
fn save_proof_data(
    proof_id: &str,
    image_data: &[u8],
    _image_filename: &str,
    proof_request: &ProofRequest,
) -> Result<PathBuf, String> {
    let proofs_dir = get_proofs_dir();
    let proof_dir = proofs_dir.join(proof_id);

    std::fs::create_dir_all(&proof_dir)
        .map_err(|e| format!("Failed to create proof directory: {}", e))?;

    // Save the original image
    let image_path = proof_dir.join("input.jpg");
    std::fs::write(&image_path, image_data)
        .map_err(|e| format!("Failed to save image: {}", e))?;

    // Convert operations to CLI format
    let cli_operations: Vec<CliOperation> = proof_request.operations
        .iter()
        .filter_map(convert_operation)
        .collect();

    // Create CLI-compatible JSON
    let cli_input = CliJsonInput {
        image: "input.jpg".to_string(),
        operations: cli_operations,
        output: "output.jpg".to_string(),
    };

    // Save the CLI JSON input (for CLI to use)
    let json_path = proof_dir.join("cli_input.json");
    let json_content = serde_json::to_string_pretty(&cli_input)
        .map_err(|e| format!("Failed to serialize CLI input: {}", e))?;
    std::fs::write(&json_path, json_content)
        .map_err(|e| format!("Failed to save CLI input: {}", e))?;

    // Also save the original proof request for reference
    let orig_json_path = proof_dir.join("proof_request.json");
    let orig_json_content = serde_json::to_string_pretty(proof_request)
        .map_err(|e| format!("Failed to serialize proof request: {}", e))?;
    std::fs::write(&orig_json_path, orig_json_content)
        .map_err(|e| format!("Failed to save proof request: {}", e))?;

    info!("Proof data saved to: {:?}", proof_dir);
    Ok(proof_dir)
}

// ============================================================================
// API Handlers
// ============================================================================

/// Health check endpoint
async fn health_check() -> Json<serde_json::Value> {
    Json(serde_json::json!({
        "status": "healthy",
        "service": "Image Proof Validator"
    }))
}

/// Preview endpoint - process image and return result
async fn preview_image(
    mut multipart: Multipart,
) -> Result<Json<PreviewResponse>, (StatusCode, Json<PreviewResponse>)> {
    info!("Received preview request");

    let mut image_data: Option<Vec<u8>> = None;
    let mut proof_json: Option<ProofRequest> = None;

    // Parse multipart form data
    loop {
        match multipart.next_field().await {
            Ok(Some(field)) => {
                let field_name = field.name().unwrap_or("").to_string();

                match field_name.as_str() {
                    "image" => {
                        match field.bytes().await {
                            Ok(bytes) => {
                                info!("Preview image bytes received: {} bytes", bytes.len());
                                image_data = Some(bytes.to_vec());
                            }
                            Err(e) => {
                                error!("Failed to read preview image data: {:?}", e);
                                return Err((
                                    StatusCode::BAD_REQUEST,
                                    Json(PreviewResponse {
                                        success: false,
                                        message: format!("Failed to read image data: {:?}", e),
                                        image: None,
                                        image_hash: None,
                                    }),
                                ));
                            }
                        }
                    }
                    "proof_json" => {
                        match field.text().await {
                            Ok(json_str) => {
                                match serde_json::from_str::<ProofRequest>(&json_str) {
                                    Ok(parsed) => proof_json = Some(parsed),
                                    Err(e) => {
                                        error!("Failed to parse preview proof JSON: {}", e);
                                        return Err((
                                            StatusCode::BAD_REQUEST,
                                            Json(PreviewResponse {
                                                success: false,
                                                message: format!("Failed to parse proof JSON: {}", e),
                                                image: None,
                                                image_hash: None,
                                            }),
                                        ));
                                    }
                                }
                            }
                            Err(e) => {
                                error!("Failed to read preview proof JSON: {}", e);
                                return Err((
                                    StatusCode::BAD_REQUEST,
                                    Json(PreviewResponse {
                                        success: false,
                                        message: format!("Failed to read proof JSON: {}", e),
                                        image: None,
                                        image_hash: None,
                                    }),
                                ));
                            }
                        }
                    }
                    _ => {}
                }
            }
            Ok(None) => break,
            Err(e) => {
                error!("Failed to parse preview multipart: {}", e);
                return Err((
                    StatusCode::BAD_REQUEST,
                    Json(PreviewResponse {
                        success: false,
                        message: format!("Failed to parse multipart: {}", e),
                        image: None,
                        image_hash: None,
                    }),
                ));
            }
        }
    }

    let image_data = image_data.ok_or_else(|| {
        (
            StatusCode::BAD_REQUEST,
            Json(PreviewResponse {
                success: false,
                message: "Missing image file".to_string(),
                image: None,
                image_hash: None,
            }),
        )
    })?;

    let proof_request = proof_json.ok_or_else(|| {
        (
            StatusCode::BAD_REQUEST,
            Json(PreviewResponse {
                success: false,
                message: "Missing proof JSON".to_string(),
                image: None,
                image_hash: None,
            }),
        )
    })?;

    // Create temp directory
    let temp_dir = tempfile::TempDir::new().map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(PreviewResponse {
                success: false,
                message: format!("Failed to create temp directory: {}", e),
                image: None,
                image_hash: None,
            }),
        )
    })?;

    let temp_path = temp_dir.path();

    // Save the image
    let input_path = temp_path.join("input.jpg");
    std::fs::write(&input_path, &image_data).map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(PreviewResponse {
                success: false,
                message: format!("Failed to write input image: {}", e),
                image: None,
                image_hash: None,
            }),
        )
    })?;

    // Process image operations
    match process_image_operations(&input_path, &proof_request.operations, &temp_path) {
        Ok((image_hash, _)) => {
            // Read the processed image
            let output_path = temp_path.join("output.jpg");
            let processed_image_data = std::fs::read(&output_path).map_err(|e| {
                (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    Json(PreviewResponse {
                        success: false,
                        message: format!("Failed to read processed image: {}", e),
                        image: None,
                        image_hash: None,
                    }),
                )
            })?;

            // Convert to base64
            use base64::Engine;
            let base64_image = base64::engine::general_purpose::STANDARD.encode(&processed_image_data);

            info!("Preview generated successfully. Hash: {}", &image_hash[..16]);

            Ok(Json(PreviewResponse {
                success: true,
                message: "Preview generated".to_string(),
                image: Some(base64_image),
                image_hash: Some(image_hash),
            }))
        }
        Err(e) => {
            error!("Failed to process preview image: {}", e);
            Err((
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(PreviewResponse {
                    success: false,
                    message: format!("Failed to process image: {}", e),
                    image: None,
                    image_hash: None,
                }),
            ))
        }
    }
}

/// Generate proof endpoint
async fn generate_proof(
    mut multipart: Multipart,
) -> Result<Json<ProofResponse>, (StatusCode, Json<ProofResponse>)> {
    info!("Received proof generation request");

    let mut image_data: Option<Vec<u8>> = None;
    let mut proof_json: Option<ProofRequest> = None;
    let mut image_filename: Option<String> = None;

    // Parse multipart form data
    loop {
        match multipart.next_field().await {
            Ok(Some(field)) => {
                let field_name = field.name().unwrap_or("").to_string();

                match field_name.as_str() {
                    "image" => {
                        // Get filename first before consuming the field
                        image_filename = field.file_name().map(|s| s.to_string());
                        info!("Received image: {:?}", image_filename);
                        info!("Image field content_type: {:?}", field.content_type());

                        match field.bytes().await {
                            Ok(bytes) => {
                                info!("Image bytes received: {} bytes", bytes.len());
                                image_data = Some(bytes.to_vec());
                            }
                            Err(e) => {
                                error!("Failed to read image data: {:?}", e);
                                error!("Full multipart error: {:?}", multipart);
                                return Err((
                                    StatusCode::BAD_REQUEST,
                                    Json(ProofResponse {
                                        success: false,
                                        message: format!("Failed to read image data: {:?}", e),
                                        proof_id: String::new(),
                                        final_image_hash: None,
                                        num_operations: None,
                                        storage_path: None,
                                    }),
                                ));
                            }
                        }
                    }
                    "proof_json" => {
                        info!("Processing proof_json field");
                        match field.text().await {
                            Ok(json_str) => {
                                info!("Proof JSON text: {}", &json_str[..json_str.len().min(100)]);
                                match serde_json::from_str::<ProofRequest>(&json_str) {
                                    Ok(parsed) => {
                                        proof_json = Some(parsed);
                                        info!("Received proof JSON with {} operations", proof_json.as_ref().map(|j| j.operations.len()).unwrap_or(0));
                                    }
                                    Err(e) => {
                                        error!("Failed to parse proof JSON: {}", e);
                                        return Err((
                                            StatusCode::BAD_REQUEST,
                                            Json(ProofResponse {
                                                success: false,
                                                message: format!("Failed to parse proof JSON: {}", e),
                                                proof_id: String::new(),
                                                final_image_hash: None,
                                                num_operations: None,
                                                storage_path: None,
                                            }),
                                        ));
                                    }
                                }
                            }
                            Err(e) => {
                                error!("Failed to read proof JSON: {}", e);
                                return Err((
                                    StatusCode::BAD_REQUEST,
                                    Json(ProofResponse {
                                        success: false,
                                        message: format!("Failed to read proof JSON: {}", e),
                                        proof_id: String::new(),
                                        final_image_hash: None,
                                        num_operations: None,
                                        storage_path: None,
                                    }),
                                ));
                            }
                        }
                    }
                    _ => {
                        info!("Ignoring unknown field: {}", field_name);
                    }
                }
            }
            Ok(None) => break,
            Err(e) => {
                error!("Failed to parse multipart: {}", e);
                return Err((
                    StatusCode::BAD_REQUEST,
                    Json(ProofResponse {
                        success: false,
                        message: format!("Failed to parse multipart: {}", e),
                        proof_id: String::new(),
                        final_image_hash: None,
                        num_operations: None,
                        storage_path: None,
                    }),
                ));
            }
        }
    }

    // Validate we have both image and proof JSON
    let image_data = image_data.ok_or_else(|| {
        (
            StatusCode::BAD_REQUEST,
            Json(ProofResponse {
                success: false,
                message: "Missing image file".to_string(),
                proof_id: String::new(),
                final_image_hash: None,
                num_operations: None,
                storage_path: None,
            }),
        )
    })?;

    let proof_request = proof_json.ok_or_else(|| {
        (
            StatusCode::BAD_REQUEST,
            Json(ProofResponse {
                success: false,
                message: "Missing proof JSON".to_string(),
                proof_id: String::new(),
                final_image_hash: None,
                num_operations: None,
                storage_path: None,
            }),
        )
    })?;

    // Generate unique ID for this proof
    let proof_id = Uuid::new_v4().to_string();

    // Create temporary directory for processing
    let temp_dir = tempfile::TempDir::new().map_err(|e| {
        error!("Failed to create temp directory: {}", e);
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ProofResponse {
                success: false,
                message: format!("Failed to create temp directory: {}", e),
                proof_id: proof_id.clone(),
                final_image_hash: None,
                num_operations: None,
                storage_path: None,
            }),
        )
    })?;

    let temp_path = temp_dir.path();

    // Save the original image
    let input_filename = image_filename.unwrap_or_else(|| "input.jpg".to_string());
    let input_path = temp_path.join(&input_filename);
    std::fs::write(&input_path, &image_data).map_err(|e| {
        error!("Failed to write input image: {}", e);
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ProofResponse {
                success: false,
                message: format!("Failed to write input image: {}", e),
                proof_id: proof_id.clone(),
                final_image_hash: None,
                num_operations: None,
                storage_path: None,
            }),
        )
    })?;

    info!("Input image saved to: {:?}", input_path);

    // Save proof data persistently for later CLI processing
    let storage_path = save_proof_data(
        &proof_id,
        &image_data,
        &input_filename,
        &proof_request,
    ).map_err(|e| {
        error!("Failed to save proof data: {}", e);
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ProofResponse {
                success: false,
                message: format!("Failed to save proof data: {}", e),
                proof_id: proof_id.clone(),
                final_image_hash: None,
                num_operations: None,
                storage_path: None,
            }),
        )
    })?;

    info!("Proof data saved to: {:?}", storage_path);

    // Process image operations locally to compute final image hash
    match process_image_operations(&input_path, &proof_request.operations, &temp_path) {
        Ok((local_hash, num_ops)) => {
            info!(
                "Image processed locally. Hash: {}, Ops: {}",
                &local_hash[..16],
                num_ops
            );

            // Compute hash of the original uploaded image
            let original_hash_hex = compute_file_hash(&input_path).map_err(|e| {
                error!("Failed to compute original image hash: {}", e);
                (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    Json(ProofResponse {
                        success: false,
                        message: format!("Failed to compute original image hash: {}", e),
                        proof_id: proof_id.clone(),
                        final_image_hash: None,
                        num_operations: None,
                        storage_path: None,
                    }),
                )
            })?;
            info!("Original image hash: {}", &original_hash_hex[..16]);

            // Validate the submitted proof JSON
            // Check 0: Original image hash must match
            let json_original_hash_lower = proof_request.original_image_hash.to_lowercase();
            if original_hash_hex.to_lowercase() != json_original_hash_lower {
                error!(
                    "Validation failed: original_image_hash mismatch. JSON: {}, Actual: {}",
                    json_original_hash_lower, original_hash_hex
                );
                return Err((
                    StatusCode::BAD_REQUEST,
                    Json(ProofResponse {
                        success: false,
                        message: format!(
                            "Validation failed: original_image_hash mismatch. JSON: {}, Actual: {}",
                            proof_request.original_image_hash, original_hash_hex
                        ),
                        proof_id,
                        final_image_hash: None,
                        num_operations: None,
                        storage_path: None,
                    }),
                ));
            }

            // Check 1: Number of operations must match
            if num_ops != proof_request.num_operations {
                error!(
                    "Validation failed: num_operations mismatch. JSON: {}, Actual: {}",
                    proof_request.num_operations, num_ops
                );
                return Err((
                    StatusCode::BAD_REQUEST,
                    Json(ProofResponse {
                        success: false,
                        message: format!(
                            "Validation failed: num_operations mismatch. JSON: {}, Actual: {}",
                            proof_request.num_operations, num_ops
                        ),
                        proof_id,
                        final_image_hash: None,
                        num_operations: None,
                        storage_path: None,
                    }),
                ));
            }

            // Check 2: Final image hash must match
            let json_final_hash_lower = proof_request.final_image_hash.to_lowercase();
            if local_hash.to_lowercase() != json_final_hash_lower {
                error!(
                    "Validation failed: final_image_hash mismatch. JSON: {}, Actual: {}",
                    json_final_hash_lower, local_hash
                );
                return Err((
                    StatusCode::BAD_REQUEST,
                    Json(ProofResponse {
                        success: false,
                        message: format!(
                            "Validation failed: final_image_hash mismatch. JSON: {}, Actual: {}",
                            proof_request.final_image_hash, local_hash
                        ),
                        proof_id,
                        final_image_hash: None,
                        num_operations: None,
                        storage_path: None,
                    }),
                ));
            }

            info!("JSON validation passed! Operations and hash match.");

            // Return success - validation passed
            Ok(Json(ProofResponse {
                success: true,
                message: "Validation passed: image operations and hash verified successfully".to_string(),
                proof_id,
                final_image_hash: Some(local_hash),
                num_operations: Some(num_ops),
                storage_path: Some(storage_path.to_string_lossy().to_string()),
            }))
        }
        Err(e) => {
            error!("Failed to process image: {}", e);
            Err((
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ProofResponse {
                    success: false,
                    message: format!("Failed to process image: {}", e),
                    proof_id,
                    final_image_hash: None,
                    num_operations: None,
                    storage_path: None,
                }),
            ))
        }
    }
}

/// Process image operations locally
fn process_image_operations(
    input_path: &Path,
    operations: &[Operation],
    output_dir: &Path,
) -> Result<(String, u32), String> {
    use image_editor::{CropRegion, ResizeOptions, ResizeFilter};

    let mut current_path: PathBuf = input_path.to_path_buf();
    let output_path = output_dir.join("output.jpg");

    for (i, op) in operations.iter().enumerate() {
        let next_path: PathBuf = if i == operations.len() - 1 {
            output_path.clone()
        } else {
            output_dir.join(format!("temp_{}.jpg", i))
        };

        // Parse operation type and params
        let params: Vec<&str> = op.params.split(',').collect();

        match op.op_type {
            OperationType::Crop => {
                let x: u32 = params.get(1).and_then(|s| s.parse().ok()).unwrap_or(0);
                let y: u32 = params.get(2).and_then(|s| s.parse().ok()).unwrap_or(0);
                let width: u32 = params.get(3).and_then(|s| s.parse().ok()).unwrap_or(800);
                let height: u32 = params.get(4).and_then(|s| s.parse().ok()).unwrap_or(600);

                let region = CropRegion { x, y, width, height };
                image_editor::crop_image(
                    current_path.to_str().unwrap(),
                    region,
                    next_path.to_str().unwrap(),
                )
                .map_err(|e| e.to_string())?;
            }
            OperationType::Resize => {
                let width: u32 = params.get(1).and_then(|s| s.parse().ok()).unwrap_or(800);
                let height: u32 = params.get(2).and_then(|s| s.parse().ok()).unwrap_or(600);

                let options = ResizeOptions {
                    width,
                    height,
                    filter: ResizeFilter::Triangle,
                };
                image_editor::resize_image(
                    current_path.to_str().unwrap(),
                    options,
                    next_path.to_str().unwrap(),
                )
                .map_err(|e| e.to_string())?;
            }
            OperationType::ResizeAspect => {
                let max_width: u32 = params.get(0).and_then(|s| s.parse().ok()).unwrap_or(800);
                let max_height: u32 = params.get(1).and_then(|s| s.parse().ok()).unwrap_or(600);

                image_editor::resize_with_aspect_ratio(
                    current_path.to_str().unwrap(),
                    max_width,
                    max_height,
                    ResizeFilter::Triangle,
                    next_path.to_str().unwrap(),
                )
                .map_err(|e| e.to_string())?;
            }
            OperationType::Brightness => {
                let value: i32 = params.get(0).and_then(|s| s.parse().ok()).unwrap_or(0);

                image_editor::adjust_brightness(
                    current_path.to_str().unwrap(),
                    value,
                    next_path.to_str().unwrap(),
                )
                .map_err(|e| e.to_string())?;
            }
            OperationType::Contrast => {
                let value: i32 = params.get(1).and_then(|s| s.parse().ok()).unwrap_or(0);

                image_editor::adjust_contrast(
                    current_path.to_str().unwrap(),
                    value,
                    next_path.to_str().unwrap(),
                )
                .map_err(|e| e.to_string())?;
            }
            OperationType::Exposure => {
                let value: f32 = params.get(1).and_then(|s| s.parse().ok()).unwrap_or(0.0);

                image_editor::adjust_exposure(
                    current_path.to_str().unwrap(),
                    value,
                    next_path.to_str().unwrap(),
                )
                .map_err(|e| e.to_string())?;
            }
            OperationType::Gamma => {
                let value: f32 = params.get(1).and_then(|s| s.parse().ok()).unwrap_or(1.0);

                image_editor::adjust_gamma(
                    current_path.to_str().unwrap(),
                    value,
                    next_path.to_str().unwrap(),
                )
                .map_err(|e| e.to_string())?;
            }
            OperationType::Thumbnail => {
                let max_size: u32 = params.get(1).and_then(|s| s.parse().ok()).unwrap_or(200);

                image_editor::create_thumbnail(
                    current_path.to_str().unwrap(),
                    max_size,
                    next_path.to_str().unwrap(),
                )
                .map_err(|e| e.to_string())?;
            }
            OperationType::CropResize => {
                let x: u32 = params.get(1).and_then(|s| s.parse().ok()).unwrap_or(0);
                let y: u32 = params.get(2).and_then(|s| s.parse().ok()).unwrap_or(0);
                let width: u32 = params.get(3).and_then(|s| s.parse().ok()).unwrap_or(800);
                let height: u32 = params.get(4).and_then(|s| s.parse().ok()).unwrap_or(600);
                let target_width: u32 = params.get(5).and_then(|s| s.parse().ok()).unwrap_or(400);
                let target_height: u32 = params.get(6).and_then(|s| s.parse().ok()).unwrap_or(300);

                let region = CropRegion { x, y, width, height };
                let options = ResizeOptions {
                    width: target_width,
                    height: target_height,
                    filter: ResizeFilter::Triangle,
                };
                image_editor::crop_and_resize(
                    current_path.to_str().unwrap(),
                    region,
                    options,
                    next_path.to_str().unwrap(),
                )
                .map_err(|e| e.to_string())?;
            }
        }

        current_path = next_path;
    }

    // Compute final image hash
    let hash = compute_file_hash(&output_path)?;

    Ok((hash, operations.len() as u32))
}

/// Compute SHA256 hash of a file and return as hex string
fn compute_file_hash(path: &Path) -> Result<String, String> {
    use sha2::{Digest, Sha256};

    let data = std::fs::read(path).map_err(|e| format!("Failed to read file: {}", e))?;
    let mut hasher = Sha256::new();
    hasher.update(&data);
    let result = hasher.finalize();
    let hash_hex = hex::encode(result);
    Ok(hash_hex)
}

// ============================================================================
// Main
// ============================================================================

#[tokio::main]
async fn main() {
    // Initialize logging
    tracing_subscriber::fmt()
        .with_target(false)
        .with_env_filter(
            tracing_subscriber::EnvFilter::from_default_env()
                .add_directive(tracing::Level::INFO.into()),
        )
        .init();

    info!("Starting Image Proof Validator Server");

    // Configure CORS
    let cors = CorsLayer::new()
        .allow_origin(Any)
        .allow_methods(Any)
        .allow_headers(Any);

    // Build router
    let app = Router::new()
        .route("/", get(health_check))
        .route(
            "/api/proof",
            post(generate_proof).layer(DefaultBodyLimit::disable()),
        )
        .route(
            "/api/preview",
            post(preview_image).layer(DefaultBodyLimit::disable()),
        )
        .layer(cors);

    // Start server
    let addr = SocketAddr::from(([0, 0, 0, 0], 3000));
    info!("Server listening on http://{}", addr);

    let listener = tokio::net::TcpListener::bind(addr).await.unwrap();
    axum::serve(listener, app).await.unwrap();
}
