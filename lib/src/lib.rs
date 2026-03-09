#![cfg_attr(not(feature = "std"), no_std)]

extern crate alloc;

use alloc::{string::String, vec, vec::Vec};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

pub const OP_CROP: u8 = 1;
pub const OP_BRIGHTNESS: u8 = 1 << 1;
pub const OP_INVERT: u8 = 1 << 2;
pub const OP_THRESHOLD: u8 = 1 << 3;
pub const OP_ROTATE90: u8 = 1 << 4;
pub const PROVENANCE_MODE_MOCK: u8 = 0;
pub const PROVENANCE_MODE_C2PA: u8 = 1;
pub const C2PA_STATE_UNKNOWN: u8 = 0;
pub const C2PA_STATE_VALID: u8 = 1;
pub const C2PA_STATE_TRUSTED: u8 = 2;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CropParams {
    pub x: u32,
    pub y: u32,
    pub width: u32,
    pub height: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EditWitness {
    pub original_pixels: Vec<u8>,
    pub original_width: u32,
    pub original_height: u32,
    pub crop: CropParams,
    pub brightness_delta: i16,
    pub invert: bool,
    pub threshold: Option<u8>,
    pub rotate_quarters: u8,
    pub provenance_mode: u8,
    pub provenance_manifest_hash: [u8; 32],
    pub provenance_asset_hash: [u8; 32],
    pub provenance_state: u8,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EditedImage {
    pub width: u32,
    pub height: u32,
    pub pixels: Vec<u8>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProofPublicValues {
    pub original_hash: [u8; 32],
    pub edited_hash: [u8; 32],
    pub op_mask: u8,
    pub provenance_mode: u8,
    pub provenance_manifest_hash: [u8; 32],
    pub provenance_asset_hash: [u8; 32],
    pub provenance_state: u8,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SignedMetadata {
    pub scheme: String,
    pub image_hash_hex: String,
    pub signer_pubkey_sec1_hex: String,
    pub signature_der_hex: String,
    pub issued_at_unix_secs: u64,
    pub provenance_hint: String,
}

pub fn sha256_bytes(bytes: &[u8]) -> [u8; 32] {
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    let digest = hasher.finalize();

    let mut out = [0u8; 32];
    out.copy_from_slice(&digest);
    out
}

pub fn apply_edits(w: &EditWitness) -> Result<EditedImage, &'static str> {
    if w.original_pixels.len() != (w.original_width as usize) * (w.original_height as usize) {
        return Err("pixel buffer size does not match width*height");
    }

    let cropped = crop_gray(
        &w.original_pixels,
        w.original_width,
        w.original_height,
        &w.crop,
    )?;

    let mut brightened = cropped;
    apply_brightness_in_place(&mut brightened, w.brightness_delta);
    if w.invert {
        apply_invert_in_place(&mut brightened);
    }
    if let Some(t) = w.threshold {
        apply_threshold_in_place(&mut brightened, t);
    }

    let rotations = w.rotate_quarters % 4;
    if rotations == 0 {
        return Ok(EditedImage {
            width: w.crop.width,
            height: w.crop.height,
            pixels: brightened,
        });
    }

    let (width, height, pixels) = rotate_gray(&brightened, w.crop.width, w.crop.height, rotations)?;

    Ok(EditedImage {
        width,
        height,
        pixels,
    })
}

fn crop_gray(
    pixels: &[u8],
    width: u32,
    height: u32,
    crop: &CropParams,
) -> Result<Vec<u8>, &'static str> {
    if crop.width == 0 || crop.height == 0 {
        return Err("crop dimensions must be non-zero");
    }
    if crop.x >= width || crop.y >= height {
        return Err("crop origin is out of bounds");
    }

    let right = crop
        .x
        .checked_add(crop.width)
        .ok_or("crop x + width overflow")?;
    let bottom = crop
        .y
        .checked_add(crop.height)
        .ok_or("crop y + height overflow")?;

    if right > width || bottom > height {
        return Err("crop rectangle exceeds image bounds");
    }

    let out_len = (crop.width as usize) * (crop.height as usize);
    let mut out = Vec::with_capacity(out_len);

    for row in crop.y..bottom {
        let row_start = (row as usize) * (width as usize);
        let from = row_start + (crop.x as usize);
        let to = from + (crop.width as usize);
        out.extend_from_slice(&pixels[from..to]);
    }

    Ok(out)
}

fn apply_brightness_in_place(pixels: &mut [u8], delta: i16) {
    for p in pixels.iter_mut() {
        let v = i16::from(*p) + delta;
        *p = v.clamp(0, 255) as u8;
    }
}

fn apply_invert_in_place(pixels: &mut [u8]) {
    for p in pixels.iter_mut() {
        *p = 255u8.wrapping_sub(*p);
    }
}

fn apply_threshold_in_place(pixels: &mut [u8], threshold: u8) {
    for p in pixels.iter_mut() {
        *p = if *p >= threshold { 255 } else { 0 };
    }
}

fn rotate_gray(
    pixels: &[u8],
    width: u32,
    height: u32,
    rotate_quarters: u8,
) -> Result<(u32, u32, Vec<u8>), &'static str> {
    if pixels.len() != (width as usize) * (height as usize) {
        return Err("rotate input pixels size mismatch");
    }

    match rotate_quarters {
        1 => {
            let mut out = vec![0u8; pixels.len()];
            // 90 degrees clockwise.
            for y in 0..height {
                for x in 0..width {
                    let src = (y as usize) * (width as usize) + (x as usize);
                    let nx = height - 1 - y;
                    let ny = x;
                    let dst = (ny as usize) * (height as usize) + (nx as usize);
                    out[dst] = pixels[src];
                }
            }
            Ok((height, width, out))
        }
        2 => {
            let mut out = vec![0u8; pixels.len()];
            for y in 0..height {
                for x in 0..width {
                    let src = (y as usize) * (width as usize) + (x as usize);
                    let nx = width - 1 - x;
                    let ny = height - 1 - y;
                    let dst = (ny as usize) * (width as usize) + (nx as usize);
                    out[dst] = pixels[src];
                }
            }
            Ok((width, height, out))
        }
        3 => {
            let mut out = vec![0u8; pixels.len()];
            // 270 clockwise = 90 counter-clockwise.
            for y in 0..height {
                for x in 0..width {
                    let src = (y as usize) * (width as usize) + (x as usize);
                    let nx = y;
                    let ny = width - 1 - x;
                    let dst = (ny as usize) * (height as usize) + (nx as usize);
                    out[dst] = pixels[src];
                }
            }
            Ok((height, width, out))
        }
        _ => Err("rotate_quarters must be in [0, 3]"),
    }
}

pub fn op_mask_from_witness(w: &EditWitness) -> u8 {
    let mut mask = OP_CROP | OP_BRIGHTNESS;
    if w.invert {
        mask |= OP_INVERT;
    }
    if w.threshold.is_some() {
        mask |= OP_THRESHOLD;
    }
    if w.rotate_quarters % 4 != 0 {
        mask |= OP_ROTATE90;
    }
    mask
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn crop_and_brightness_are_deterministic() {
        let witness = EditWitness {
            original_pixels: vec![10, 20, 30, 40, 50, 60, 70, 80, 90],
            original_width: 3,
            original_height: 3,
            crop: CropParams {
                x: 1,
                y: 1,
                width: 2,
                height: 2,
            },
            brightness_delta: 10,
            invert: true,
            threshold: Some(80),
            rotate_quarters: 1,
            provenance_mode: PROVENANCE_MODE_MOCK,
            provenance_manifest_hash: [0u8; 32],
            provenance_asset_hash: [0u8; 32],
            provenance_state: C2PA_STATE_UNKNOWN,
        };

        let edited = apply_edits(&witness).unwrap();
        assert_eq!(edited.width, 2);
        assert_eq!(edited.height, 2);
        assert_eq!(edited.pixels, vec![255, 255, 255, 255]);
    }
}
