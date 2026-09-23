# Brevis Vera Prototype

A prototype for **media authenticity attestation**: verify provenance, apply a constrained edit pipeline, generate a Pico zkVM proof, and let a third party verify the result independently.

```
Capture / provenance  →  Edit (crop + transforms)  →  Prove (Pico guest)  →  Verify (CLI or web)
```

This repository is a **research and demo prototype**, not a production verifier. See [Security](#security) before deploying or integrating it.

## What it proves

Given a source image and approved edit parameters, the system attests that:

1. The source image hash was signed (mock ECDSA) or came from a C2PA-valid asset (host-checked).
2. The edited output was produced by the deterministic edit pipeline in `brevis-vera-lib`.
3. Public outputs are minimal: `original_hash`, `edited_hash`, `op_mask`, plus provenance fields.

Private edit details (crop coordinates, source dimensions) stay in the witness and are not committed publicly.

## Repository layout

| Path | Role |
|------|------|
| [`lib/`](lib/) | Shared structs and deterministic edit logic (`EditWitness`, `ProofPublicValues`, `apply_edits`) |
| [`app/`](app/) | Pico guest (`#![no_main]`): reads witness, applies edits, commits public values |
| [`prover/`](prover/) | Host CLI: signing, proving, verification, C2PA checks, benchmarks |
| [`web/`](web/) | Local demo UI that shells out to the prover CLI |
| [`samples/`](samples/) | Small test images |
| [`DSC00050.JPG`](DSC00050.JPG) | Real Sony image with embedded C2PA manifest |
| [`scripts/`](scripts/) | CI-style fixture and negative tests |

### Guest program

[`app/src/main.rs`](app/src/main.rs) reads `EditWitness`, checks `sha256(original_pixels) == provenance_asset_hash`, runs `apply_edits`, and commits `ProofPublicValues`.

### Prover CLI

[`prover/src/main.rs`](prover/src/main.rs) implements:

| Command | Purpose |
|---------|---------|
| `mock-sign` | Create mock ECDSA P-256 signed metadata over an image hash |
| `edit-and-prove` | Verify mock metadata, edit, generate Pico proof (`prove_fast`) |
| `edit-and-prove-c2pa` | Verify C2PA on host, bind manifest fields into proof public values |
| `verify` | Verify RISC-V proof + mock metadata + edited image hash |
| `verify-c2pa-proof` | Verify proof bound to a C2PA source image |
| `verify-c2pa` | Validate embedded C2PA manifest/signature on a file |
| `perf` | Stage-level benchmark (prove, emulate, optional C2PA) |
| `prepare-series` | Generate resized image series for scaling tests |
| `emulate-cycles` | Emulation-only cycle measurement |

Proof generation: `client.prove_fast(...)` in `run_edit_and_prove` / `run_edit_and_prove_c2pa`.

Proof verification: `riscv.verify(&riscv_proof, riscv.vk())` in `run_verify` / `run_verify_c2pa_proof`.

## Build

From the repository root:

```bash
cargo fmt
cargo build
cargo pico build --manifest-path app/Cargo.toml
```

Rebuild the guest ELF whenever `app/` or shared `lib/` logic changes.

## Quick start (mock provenance)

```bash
# 1) Sign mock provenance metadata
cargo run --release -p brevis-vera-prover -- mock-sign \
  --input-image samples/input.png \
  --metadata-out artifacts/mock_metadata.json \
  --private-key-pem artifacts/mock_signer_key.pem

# 2) Edit and prove
cargo run --release -p brevis-vera-prover -- edit-and-prove \
  --input-image samples/input.png \
  --metadata artifacts/mock_metadata.json \
  --crop-x 1 --crop-y 1 --crop-w 4 --crop-h 4 \
  --brightness-delta 20 \
  --edited-image-out artifacts/edited.png \
  --riscv-proof-out artifacts/riscv_proof.bin \
  --public-values-out artifacts/public_values.json

# 3) Independent verification
cargo run --release -p brevis-vera-prover -- verify \
  --edited-image artifacts/edited.png \
  --metadata artifacts/mock_metadata.json \
  --riscv-proof artifacts/riscv_proof.bin
```

Expected output:

```text
AUTHENTICITY VERDICT: VALID
```

**Note:** Committed files under `artifacts/` can drift from the current guest ELF. Regenerate proofs after rebuilding `app/` or use fresh output paths.

### Supported edits

- **Mandatory:** crop
- **Default:** brightness
- **Optional:** invert, threshold, rotate (90° quarter turns)

`op_mask` records which transforms were applied.

## C2PA flow

Check a real C2PA image:

```bash
cargo run --release -p brevis-vera-prover -- verify-c2pa --input-image DSC00050.JPG
```

Prove with C2PA-bound public values:

```bash
cargo run --release -p brevis-vera-prover -- edit-and-prove-c2pa \
  --input-image DSC00050.JPG \
  --crop-x 100 --crop-y 100 --crop-w 512 --crop-h 512 \
  --brightness-delta 20 \
  --edited-image-out artifacts/c2pa_edited.png \
  --riscv-proof-out artifacts/c2pa_riscv_proof.bin \
  --public-values-out artifacts/c2pa_public_values.json

cargo run --release -p brevis-vera-prover -- verify-c2pa-proof \
  --source-c2pa-image DSC00050.JPG \
  --edited-image artifacts/c2pa_edited.png \
  --riscv-proof artifacts/c2pa_riscv_proof.bin
```

### C2PA boundary

- C2PA manifest validation runs on the **host** via `c2pa::Reader`.
- The guest does **not** verify certificate chains or signatures in-circuit.
- The guest binds `provenance_asset_hash` to `sha256(original_pixels)` and commits manifest hash and validation state supplied by the host.

Treat host C2PA checks as part of the trust boundary until full in-guest verification exists.

## Proof modes

| Mode | What is verified | Typical use |
|------|------------------|-------------|
| `prove_fast` + `riscv.verify` | RISC-V proof only; public values read from `pv_stream` after verification | Local / off-chain demo (current default) |
| Full `prove` pipeline | RISC-V → convert → combine → compress → embed | On-chain or embed-grade assurance (not wired as default) |

Do **not** claim embed/on-chain verification when using the default fast path.

## Web demo

```bash
cargo run -p brevis-vera-web
```

Open `http://127.0.0.1:8080`.

- **Index:** run mock sign → edit-and-prove; artifacts land in `artifacts/web_runs/run_*`
- **Verify page:** `/verify-page` — verify saved artifacts
- **C2PA:** run `verify-c2pa` (default input `DSC00050.JPG`)
- **Uploads:** browser uploads go to `artifacts/web_uploads/`

The web server binds to `127.0.0.1` and executes local `cargo run --release -p brevis-vera-prover` subprocesses. It is intended for local demos only.

## Performance

Run a benchmark:

```bash
cargo run --release -p brevis-vera-prover -- perf \
  --input-image samples/input.png \
  --metadata artifacts/mock_metadata.json \
  --crop-x 1 --crop-y 1 --crop-w 4 --crop-h 4 \
  --brightness-delta 20 \
  --iterations 3 --warmup 1 \
  --c2pa-image DSC00050.JPG \
  --json-out artifacts/perf_report.json
```

CI-style checks:

```bash
bash scripts/check-fixtures.sh
bash scripts/check-negative.sh
bash scripts/perf-matrix.sh commit
```

### Bottleneck (measured 2026-03-09)

Proving dominates runtime (~99% in `perf_report_5runs.json`). Cost scales with **full source pixel count** because the guest hashes entire `original_pixels` and edited output in-circuit.

| Input | Cycles (nightly tier) |
|-------|---------------------|
| 256×256 | 5,518,251 |
| 1024×1024 | 82,748,332 |
| 4096×4096 | 1,330,226,632 |

Optional transforms (invert, threshold, rotate) add ~2% overhead vs baseline on 256×256 inputs.

Prepare scaling inputs:

```bash
cargo run --release -p brevis-vera-prover -- prepare-series \
  --input-image DSC00050.JPG \
  --output-dir samples/series \
  --sizes 256,512,1024,2048,4096 \
  --ext jpg
```

## Security

**Read this section before treating verification output as trustworthy.**

### Critical: unauthenticated public values in `verify`

`run_verify` checks the Pico RISC-V proof, then deserializes `ProofPublicValues` from `riscv_proof.pv_stream` and compares those fields to the metadata and edited image.

`pv_stream` is **metadata attached to the proof file**, not cryptographically bound by `riscv.verify()` in the current Pico integration. An attacker can:

1. Take any valid `riscv_proof.bin` from an unrelated run.
2. Replace `pv_stream` with forged `ProofPublicValues` (matching their own metadata signature and edited image hash).
3. Pass `verify` with `AUTHENTICITY VERDICT: VALID`.

**Mitigation:** Derive public values from the proof's committed digest inside the verifier (Pico `committed_value_digest`), or verify a binding between proof and `pv_stream` before trusting deserialized fields.

### High: mock provenance is not C2PA

`edit-and-prove` accepts any ECDSA key that signs the claimed hash. There is no allowlist of trusted signers. Mock mode must not be used where real C2PA assurance is required.

### High: hash does not include image dimensions

`edited_hash` is `sha256(raw_pixels)` without width/height. Different `(width, height)` with the same pixel bytes produce the same hash, which can weaken visual integrity guarantees.

### Medium: host trust for C2PA

`edit-and-prove-c2pa` trusts host-side C2PA validation. A compromised or misconfigured host can supply incorrect manifest hash or validation state to the guest witness.

### Medium: web demo weaknesses

- API handlers accept arbitrary filesystem paths (artifact paths from forms).
- Frontend uses `innerHTML` for API responses (reflected XSS risk if the server is exposed beyond localhost).
- No authentication, rate limits, or upload size caps.
- Uploads are written under `artifacts/web_uploads/` without quotas.

### Medium: operational gaps

- Demo signing key path defaults to `artifacts/mock_signer_key.pem` (generate per environment; never use committed keys in production).
- No artifact schema versioning; format changes can confuse verifiers.
- No max image size guard — large inputs enable proving DoS.
- Stale committed proofs fail verification silently with opaque IO errors until regenerated.

### What verification does provide today

When artifacts are freshly generated and `pv_stream` is trusted (same operator, intact files):

- The RISC-V proof shows the guest program executed correctly for **some** witness.
- Mock metadata ECDSA signature matches the claimed original hash.
- Edited file bytes match `edited_hash` in public values.
- Mandatory crop is reflected in `op_mask`.

Negative tests in `scripts/check-negative.sh` cover tampered edited images and mismatched metadata.

## Roadmap

Near-term hardening priorities:

1. **Bind public values to proof verification** (fix `pv_stream` trust gap).
2. **Include dimensions in hashes** or commit width/height in public values.
3. **Trusted signer / C2PA policy** configuration for production paths.
4. **Artifact versioning** (`statement_version`, `proof_mode`) with verifier gating.
5. **Input size limits** and clearer proof-mode labeling (`fast-local` vs `high-assurance`).
6. **Full recursive prove path** when embed/on-chain assurance is required.

Longer term: in-guest C2PA verification, HSM/KMS signing, formal threat model, and performance mode with explicit weaker statement for large images.

## Agent / contributor notes

See [`AGENTS.md`](AGENTS.md) for invariants, build commands, and editing guidelines when extending the prototype.
