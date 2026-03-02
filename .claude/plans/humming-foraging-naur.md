# Plan: Add Image Crop to Image Processor

## Context

Currently, the image-processor proves that an image has a valid C2PA signature. The user wants to extend this to also prove image editing operations (starting with crop) after the C2PA signature is verified.

## Current Architecture

- **app/main.rs**: zkVM app that verifies C2PA signature
- **prover/main.rs**: Extracts C2PA data from image, runs zkVM, generates proof
- **lib/src/lib.rs**: C2PA extraction and verification utilities

## Goal

Add image crop functionality that proves:
1. The original image has a valid C2PA signature (existing)
2. The image was cropped correctly with given parameters (new)

## Implementation Approach

### Option: Prover does crop, zkVM verifies crop parameters

The prover will:
1. Extract C2PA from original image (existing)
2. Crop the image using image-editor crate
3. Pass crop parameters to zkVM (x, y, width, height)
4. Compute hash of cropped image
5. zkVM verifies crop parameters are valid and commits to result

### Changes Required

#### 1. Update PublicValuesStruct (lib/src/lib.rs)
```rust
pub struct PublicValuesStruct {
    bool is_valid,           // C2PA signature valid
    bool has_c2pa_manifest,  // Has C2PA manifest
    bool crop_valid,         // Crop parameters valid (new)
    u32 crop_x,              // Crop x coordinate (new)
    u32 crop_y,              // Crop y coordinate (new)
    u32 crop_width,          // Crop width (new)
    u32 crop_height,         // Crop height (new)
}
```

#### 2. Update app/main.rs (zkVM)
- Add crop parameter input handling (x, y, width, height)
- Validate crop parameters (bounds checking)
- Commit crop result to public values

#### 3. Update prover/main.rs
- Add crop parameter CLI argument
- Use image-editor crate to crop image
- Write crop parameters to zkVM stdin
- Save cropped image for output

#### 4. Update verifier/main.rs
- Display crop result in verification output

### Critical Files to Modify

1. `/Users/jinyaoli/Development/brevis-network/brevis-vera/image-processor/lib/src/lib.rs` - PublicValuesStruct
2. `/Users/jinyaoli/Development/brevis-network/brevis-vera/image-processor/app/src/main.rs` - zkVM app
3. `/Users/jinyaoli/Development/brevis-network/brevis-vera/image-processor/prover/src/main.rs` - prover
4. `/Users/jinyaoli/Development/brevis-network/brevis-vera/image-processor/verifier/src/main.rs` - verifier

### Reuse Existing Code

- Use `image-editor` crate for crop operation (already exists at `/Users/jinyaoli/Development/brevis-network/brevis-vera/image-editor/src/lib.rs`)
- Use existing C2PA extraction logic

## Verification

1. Build the prover: `cargo build -p c2pa-prover`
2. Run with crop: `cargo run -p c2pa-prover -- ../DSC00050.JPG --crop "100,100,800,600"`
3. Run verifier: `cargo run -p c2pa-verifier`
4. Verify output shows both C2PA valid and crop result
