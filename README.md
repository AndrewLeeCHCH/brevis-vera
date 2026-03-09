# Brevis Vera Prototype (Capture -> Edit -> Prove -> Verify)

This repository contains a working prototype for Brevis Vera's media authenticity attestation flow:

1. Capture/provenance: verify an ECDSA P-256 signature over the original image hash (mock C2PA-style metadata).
2. Editing: apply mandatory crop plus brightness adjustment.
   Additional supported in-circuit transforms: invert, threshold, rotate-90 (quarter turns).
3. ZK proof: generate a Pico zkVM proof that the edited output derives from the original via the approved edit pipeline.
4. Verification: third-party verifier checks signature linkage, proof validity, and edited output hash.

## Architecture

- `lib/` (`brevis-vera-lib`)
- Shared schema and deterministic edit logic.
- `EditWitness`: private witness consumed by Pico guest.
- `ProofPublicValues`: public outputs committed in proof (`original_hash`, `edited_hash`, `op_mask`).

- `app/` (`brevis-vera-app`)
- Pico guest program.
- Applies edits inside zkVM and commits only privacy-preserving public values.
- Keeps crop coordinates and original dimensions private.

- `prover/` (`brevis-vera-prover`)
- CLI for full flow:
- `mock-sign`: generate mock signed provenance metadata.
- `edit-and-prove`: verify metadata, edit image, generate Pico proof artifacts.
- `edit-and-prove-c2pa`: verify C2PA on host, bind C2PA fields into proof public values.
- `verify`: independent verification CLI (verifies Pico RISC-V proof directly).
- `verify-c2pa-proof`: verify a proof that is bound to a C2PA source image.
- `verify-c2pa`: verify real embedded C2PA signatures/manifests from an image.
- `perf`: stage-level performance benchmark for current pipeline.
- `prepare-series`: generate a size series of input images for scaling tests.

## Proof Generation And Verification Locations

- Proof generation happens in [prover/src/main.rs](/Users/jinyaoli/Documents/Playground/prover/src/main.rs):
- `run_edit_and_prove(...)` and `run_edit_and_prove_c2pa(...)`
- At `client.prove_fast(stdin_builder)` and then `riscv_proof.save_to_file(...)`

- Proof verification happens in [prover/src/main.rs](/Users/jinyaoli/Documents/Playground/prover/src/main.rs):
- `run_verify(...)` and `run_verify_c2pa_proof(...)`
- At `riscv.verify(&riscv_proof, riscv.vk())`

- Guest code being proven is in [app/src/main.rs](/Users/jinyaoli/Documents/Playground/app/src/main.rs):
- Reads `EditWitness`, applies edits, commits `ProofPublicValues`

## Why this satisfies the requirements

- **Capture & provenance**: verifies ECDSA P-256 signature over source hash before proof generation.
- **Editing layer**: supports two transforms: `crop` (mandatory) and `brightness`.
  Also supports: `invert`, `threshold`, `rotate_quarters`.
- **ZK core requirement**: Pico proof attests source-to-output derivation and transformation class (`op_mask`) without leaking edit details.
- **Verification layer**: CLI outputs clear authenticity verdict and rejects tampered metadata, proof, or edited image.

## Build

```bash
cargo fmt
cargo build
cargo pico build --manifest-path app/Cargo.toml
```

## End-to-end demo

Use any input image, for example `samples/input.jpg`.

```bash
# 1) Create signed mock provenance metadata (ECDSA P-256)
cargo run -p brevis-vera-prover -- mock-sign \
  --input-image samples/input.jpg \
  --metadata-out artifacts/mock_metadata.json \
  --private-key-pem artifacts/mock_signer_key.pem

# 2) Verify signature, apply edits, and generate Pico proof artifacts
cargo run --release -p brevis-vera-prover -- edit-and-prove \
  --input-image samples/input.jpg \
  --metadata artifacts/mock_metadata.json \
  --crop-x 10 --crop-y 10 --crop-w 200 --crop-h 200 \
  --brightness-delta 20 \
  --invert \
  --threshold 120 \
  --rotate-quarters 1 \
  --edited-image-out artifacts/edited.png \
  --riscv-proof-out artifacts/riscv_proof.bin \
  --public-values-out artifacts/public_values.json

# 3) Independent verifier check
cargo run --release -p brevis-vera-prover -- verify \
  --edited-image artifacts/edited.png \
  --metadata artifacts/mock_metadata.json \
  --riscv-proof artifacts/riscv_proof.bin
```

## Web App Demo

A local browser UI is available for demoing the prototype flow on top of the existing CLI.

```bash
cargo run -p brevis-vera-web
```

Open `http://127.0.0.1:8080` and use:

1. **Mock Flow: Sign -> Edit+Prove**  
Runs `mock-sign` and `edit-and-prove`, then saves outputs under `artifacts/web_runs/run_*/`.
2. **Verify Page**  
Open `/verify-page` to verify edited image + proof (+ public values view) using saved artifacts.
3. **C2PA Validation**  
Runs `verify-c2pa` for a source image (default `DSC00050.JPG`).
4. **Image Selection + Preview**  
Choose an image file in the browser, auto-upload to `artifacts/web_uploads/`, and preview before running commands.

Notes:

- Frontend and backend are decoupled in code:
- Frontend static files: `web/frontend/index.html`, `web/frontend/verify.html`, `web/frontend/assets/*`
- Backend API server: `web/src/main.rs` (`/api/*` endpoints)
- The backend executes local `cargo run --release -p brevis-vera-prover -- ...` commands.
- Generated files remain in `artifacts/` and are linked from result pages.
- Server logs are written to stdout/stderr of the process running `cargo run -p brevis-vera-web`.
- To persist logs: `cargo run -p brevis-vera-web > artifacts/web_server.log 2>&1`
- This app is for local prototype/demo use, not hardened production deployment.

Expected final output includes:

```text
AUTHENTICITY VERDICT: VALID
```

For a real C2PA image:

```bash
cargo run --release -p brevis-vera-prover -- verify-c2pa --input-image DSC00050.JPG

# Generate proof bound to C2PA source
cargo run --release -p brevis-vera-prover -- edit-and-prove-c2pa \
  --input-image DSC00050.JPG \
  --crop-x 100 --crop-y 100 --crop-w 512 --crop-h 512 \
  --brightness-delta 20 \
  --invert \
  --threshold 120 \
  --rotate-quarters 1 \
  --edited-image-out artifacts/c2pa_edited.png \
  --riscv-proof-out artifacts/c2pa_riscv_proof.bin \
  --public-values-out artifacts/c2pa_public_values.json

# Verify proof + C2PA linkage
cargo run --release -p brevis-vera-prover -- verify-c2pa-proof \
  --source-c2pa-image DSC00050.JPG \
  --edited-image artifacts/c2pa_edited.png \
  --riscv-proof artifacts/c2pa_riscv_proof.bin
```

## Current C2PA Boundary

- Current implementation verifies C2PA using `c2pa::Reader` on the host side.
- The guest does not perform full C2PA signature/certificate-chain verification.
- The guest enforces linkage by checking `sha256(original_pixels)` against `provenance_asset_hash`
  and committing C2PA-related public values (`provenance_mode`, `provenance_manifest_hash`, `provenance_asset_hash`, `provenance_state`).

## Proof Modes And Security Tradeoff

- Current CLI uses Pico `prove_fast` and verifies the **RISC-V proof** with `riscv.verify(...)`.
- This gives soundness for the guest computation statement (edit derivation and committed public values).

- Pico `prove` (full pipeline) additionally produces and checks recursion stages:
- `RISC-V -> convert -> combine -> compress -> embed`
- This is required if you need assurance for embed/on-chain style proof artifacts.

- Security implication of current mode:
- You are **not** validating `embed_proof`.
- You should not claim embed/on-chain-proof verification in this mode.
- For local/off-chain attestation this is acceptable; for full recursive/on-chain trust you should switch to full `prove` + embed verification.

## Performance test

```bash
cargo run --release -p brevis-vera-prover -- perf \
  --input-image samples/input.png \
  --metadata artifacts/mock_metadata.json \
  --crop-x 1 --crop-y 1 --crop-w 4 --crop-h 4 \
  --brightness-delta 20 \
  --iterations 3 \
  --warmup 1 \
  --c2pa-image DSC00050.JPG \
  --json-out artifacts/perf_report.json
```

## Emulation Performance Report (2026-03-09)

Run the reproducible emulation checks:

```bash
# CI-style fixture checks (includes emulate-cycles + proof/mask validation)
bash scripts/check-fixtures.sh

# Emulation scaling matrix
bash scripts/perf-matrix.sh commit
bash scripts/perf-matrix.sh nightly
```

Generated artifacts:

- `artifacts/emulation_result_2026-03-09.md`
- `artifacts/perf_matrix_commit.csv`
- `artifacts/perf_matrix_nightly.csv`

Latest measured results:

Commit tier (`artifacts/perf_matrix_commit.csv`)

| label | pixels | cycles | emu_ms | accessed_addrs |
|---|---:|---:|---:|---:|
| series_256x256 | 65536 | 5518251 | 499.31 | 53153 |
| series_512x512 | 262144 | 20964267 | 1884.55 | 151457 |
| series_1024x1024 | 1048576 | 82748332 | 7897.76 | 544673 |

Nightly tier (`artifacts/perf_matrix_nightly.csv`)

| label | pixels | cycles | emu_ms | accessed_addrs |
|---|---:|---:|---:|---:|
| series_256x256 | 65536 | 5518251 | 385.97 | 53153 |
| series_512x512 | 262144 | 20964267 | 1474.23 | 151457 |
| series_1024x1024 | 1048576 | 82748332 | 5689.39 | 544673 |
| series_2048x2048 | 4194304 | 332244154 | 22721.92 | 2903969 |
| series_4096x4096 | 16777216 | 1330226632 | 94277.41 | 12341153 |

## Prepare scaling inputs from DSC image

```bash
cargo run --release -p brevis-vera-prover -- prepare-series \
  --input-image DSC00050.JPG \
  --output-dir samples/series \
  --sizes 256,512,1024,2048,4096 \
  --ext jpg
```

## Transition to real C2PA input

When a real C2PA signed Sony image is available:

1. Replace `mock-sign` metadata with parsed C2PA claim hash/signature output.
2. Keep the `edit-and-prove` and `verify` linkage logic unchanged.
3. Add full X.509 chain validation in place of current single-signature check.

## Production Gaps (Current Implementation)

This repository is a functional prototype, not a production-ready verifier/prover stack yet.

1. Provenance mode split
- `edit-and-prove` still relies on mock-signed metadata.
- Real C2PA-bound proving is separate (`edit-and-prove-c2pa`) and not yet the single default pipeline.
- Security risk: operator or integrator can run the mock path in production by mistake and accept weaker provenance guarantees.

2. Trust and policy enforcement
- C2PA validation is performed on host side, but production policy controls are still minimal.
- Trust-anchor management, certificate revocation handling, and explicit acceptance/rejection policy configuration need hardening.
- Security risk: invalid, revoked, or untrusted signing chains may be accepted if policy defaults are too permissive.

3. Proof profile
- Current default flow uses `prove_fast` + RISC-V proof verification.
- Recursive stages (`convert/combine/compress/embed`) are not verified in the main path, so this should not be treated as embed/on-chain-grade assurance.
- Security risk: if downstream systems assume embed/on-chain assurance, they may over-trust artifacts that were only RISC-V-level verified.

4. Key management and signing security
- Demo flows use local artifact keys (`artifacts/mock_signer_key.pem`).
- Production requires HSM/KMS-backed key custody, rotation, audit trail, and strict secret handling.
- Security risk: key leakage or unauthorized signing can compromise provenance integrity.

5. Artifact and API stability
- Proof/public-value artifact formats are still evolving with prototype scripts.
- A versioned, backward-compatible artifact schema and stable external API/CLI contract are still needed.
- Security risk: parser/version mismatches can cause silent verification bypasses or incorrect acceptance/rejection decisions.

6. Operational hardening
- Missing production guardrails: structured observability, SLO-oriented benchmarking in CI, deterministic resource limits, and failure-recovery strategy.
- No formal threat model or security review is documented yet.
- Security risk: reduced ability to detect abuse, regressions, or denial-of-service conditions in time.

7. Privacy and compliance posture
- Public values are intentionally minimal, but data handling/retention policy, compliance controls, and deployment-level privacy review are not yet defined.
- Security risk: improper retention/access controls can expose sensitive media or metadata.
