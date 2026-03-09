#![no_main]

use brevis_vera_lib::{
    EditWitness, ProofPublicValues, apply_edits, op_mask_from_witness, sha256_bytes,
};
use pico_sdk::io::{commit, read_as};

pico_sdk::entrypoint!(main);

pub fn main() {
    let witness: EditWitness = read_as();

    let original_hash = sha256_bytes(&witness.original_pixels);
    assert_eq!(
        original_hash, witness.provenance_asset_hash,
        "provenance asset hash must match original pixels hash"
    );

    let edited = apply_edits(&witness).expect("invalid edit witness");
    let edited_hash = sha256_bytes(&edited.pixels);

    let public_values = ProofPublicValues {
        original_hash,
        edited_hash,
        op_mask: op_mask_from_witness(&witness),
        provenance_mode: witness.provenance_mode,
        provenance_manifest_hash: witness.provenance_manifest_hash,
        provenance_asset_hash: witness.provenance_asset_hash,
        provenance_state: witness.provenance_state,
    };

    commit(&public_values);
}
