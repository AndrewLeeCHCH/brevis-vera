# AGENTS.md

Guidance for future coding agents working in this repository.

## Project Goal

This repo is a Brevis Vera prototype implementing:
1. Provenance verification (mock ECDSA and real C2PA check)
2. Editing pipeline (mandatory crop + brightness)
3. Pico zkVM proof generation
4. Independent verification CLI

## Repo Layout

- `lib/` (`brevis-vera-lib`)
- Shared data model and deterministic edit logic.
- Used by both host (`prover`) and guest (`app`).

- `app/` (`brevis-vera-app`)
- Pico guest program (`#![no_main]`).
- Reads `EditWitness`, applies edits, commits `ProofPublicValues`.

- `prover/` (`brevis-vera-prover`)
- Host CLI with subcommands:
- `mock-sign`
- `prepare-series`
- `edit-and-prove`
- `verify`
- `verify-c2pa`
- `perf`

- `DSC00050.JPG`
- Real Sony image with embedded C2PA manifest.

## Critical Invariants

1. `prover` and `app` must continue using the same shared structs from `brevis-vera-lib`.
2. `crop` must remain mandatory in attested operations.
3. Public committed values must stay minimal:
- `original_hash`
- `edited_hash`
- `op_mask`
4. Private edit details (crop coordinates, source dimensions) should not be committed publicly.

## Build and Run

Run from repo root:

```bash
cargo fmt
cargo build
cargo pico build --manifest-path app/Cargo.toml
```

Recommended demo runs (optimized):

```bash
cargo run --release -p brevis-vera-prover -- mock-sign \
  --input-image samples/input.png \
  --metadata-out artifacts/mock_metadata.json \
  --private-key-pem artifacts/mock_signer_key.pem

cargo run --release -p brevis-vera-prover -- edit-and-prove \
  --input-image samples/input.png \
  --metadata artifacts/mock_metadata.json \
  --crop-x 1 --crop-y 1 --crop-w 4 --crop-h 4 \
  --brightness-delta 20 \
  --edited-image-out artifacts/edited.png \
  --riscv-proof-out artifacts/riscv_proof.bin \
  --public-values-out artifacts/public_values.json

cargo run --release -p brevis-vera-prover -- verify \
  --edited-image artifacts/edited.png \
  --metadata artifacts/mock_metadata.json \
  --riscv-proof artifacts/riscv_proof.bin

cargo run --release -p brevis-vera-prover -- verify-c2pa --input-image DSC00050.JPG

cargo run --release -p brevis-vera-prover -- perf \
  --input-image samples/input.png \
  --metadata artifacts/mock_metadata.json \
  --crop-x 1 --crop-y 1 --crop-w 4 --crop-h 4 \
  --brightness-delta 20 \
  --iterations 3 --warmup 1 \
  --c2pa-image DSC00050.JPG \
  --json-out artifacts/perf_report.json
```

## Current Verification Model

- `verify-c2pa` validates embedded C2PA manifest/signature on a source asset.
- `edit-and-prove` currently uses mock signed metadata as the provenance gate.
- `verify` checks:
1. Pico RISC-V proof validity
2. Metadata signature validity
3. Hash linkage between proof public values and edited file

## Toolchain and Dependency Notes

1. `cargo pico build` is required whenever guest logic changes.
2. C2PA crate is pinned via workspace dependency (`c2pa` with `file_io` feature).
3. Proof generation can be compute-heavy; prefer `--release` for operational runs.

## If You Extend to Full Real-C2PA Flow

1. Add a mode so `edit-and-prove` derives provenance hash from actual C2PA-verified input directly.
2. Keep mock mode for fallback testing.
3. Preserve existing public-value schema unless explicitly migrating verification artifacts.
4. If introducing certificate trust-list enforcement, document trust anchor source and verification settings.

## Editing Guidelines for Agents

1. Do not duplicate edit logic in `app` and `prover`; keep it centralized in `lib`.
2. Keep CLI output explicit and machine-readable enough for demos.
3. If changing proof/public-value formats, update both producing and consuming paths in the same change.
4. Update `README.md` when command interfaces or artifact names change.
