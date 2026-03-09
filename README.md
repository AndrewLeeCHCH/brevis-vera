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

## Proving Bottleneck Analysis (2026-03-09)

The current proving bottleneck is dominated by in-circuit hashing and memory trace growth, not by edit arithmetic.

Evidence:

1. `prove_fast` dominates end-to-end runtime (from `artifacts/perf_report_5runs.json`):
- `prove_fast`: ~23340.81 ms average
- total: ~23571.21 ms average
- proving is ~99% of total runtime in this benchmark profile.

2. Emulation cycles scale nearly linearly with source image pixels (from `artifacts/perf_matrix_nightly.csv`):
- 256x256: 5,518,251 cycles
- 512x512: 20,964,267 cycles
- 1024x1024: 82,748,332 cycles
- 2048x2048: 332,244,154 cycles
- 4096x4096: 1,330,226,632 cycles
- observed cycles/pixel remains roughly stable (~79), indicating pixel-count dominated cost.

3. Optional transforms add comparatively small overhead:
- In local `emulate-cycles` checks on 256x256 input, baseline was 5,518,251 cycles; enabling `invert + threshold + rotate` raised this to 5,622,756 cycles (~1.9%).
- This indicates crop/brightness/invert/threshold/rotate logic is not the primary cost driver.

Root cause in circuit design:

- Guest computes SHA-256 on full original pixels and full edited pixels in-circuit.
- Witness carries full `original_pixels`, so proving complexity follows full source image size rather than only cropped region size.
- As image size grows, hashing and memory accesses dominate proving resources.

## Redesign Proposal (Performance + Security)

Goal: reduce proving resource usage while preserving verification integrity and existing minimal public outputs (`original_hash`, `edited_hash`, `op_mask`).

### Priority 1: Immediate, low-risk changes

1. Add proof profile modes in CLI/API
- Keep current default behavior for compatibility.
- Add explicit modes (for example `fast-local` vs `high-assurance`) with clear labels in outputs and docs.
- Benefit: avoids security over-claim without changing circuit semantics.

2. Enforce operational guardrails on image size
- Add configurable caps for max input dimensions/pixels in `edit-and-prove*`.
- Fail fast with explicit error when over cap.
- Benefit: prevents pathological proving latency/cost spikes and reduces DoS risk.

3. Improve performance telemetry
- Persist per-run cycles/time/image-size metrics by default (CSV/JSON append mode).
- Benefit: enables regression tracking and capacity planning.

### Priority 2: Main performance redesign

4. Split proof paths by trust boundary
- Path A (current semantics): prove on full source pixels for strongest source-to-output statement.
- Path B (performance mode): prove only edit correctness on a bounded working set (for example pre-cropped ROI), while source provenance remains host/C2PA verified and explicitly marked in provenance state/mode.
- Benefit: allows large resource reduction for practical deployments while preserving a high-assurance option.
- Tradeoff: Path B weakens the statement compared with full-source in-circuit linkage and must be policy-gated.

5. Replace in-circuit generic SHA-256 over large buffers with proof-friendly commitment strategy
- Keep externally visible hashes compatible for verifiers, but redesign internal commitment/check flow to reduce zkVM cost.
- Example direction: host computes chunk commitments; circuit verifies structured consistency and edited-result commitment rather than re-hashing full source bytes naively.
- Benefit: targets the dominant bottleneck (full-buffer hashing/memory trace).
- Tradeoff: requires careful security review and migration tests.

### Priority 3: Longer-term architecture

6. Introduce versioned statement schema
- Add statement/proof version field and mode field to artifacts.
- Keep verifier backward-compatible during migration window.
- Benefit: safe rollout of new proving semantics without silent mismatches.

7. Move to full recursive/on-chain profile when needed
- Add full `prove` + recursive stage verification path (`convert/combine/compress/embed`) for deployments that require it.
- Benefit: aligns assurance level with on-chain/integrated verifier expectations.
- Tradeoff: higher proving cost than `prove_fast`.

### Recommended rollout plan

1. Implement Priority 1 in current branch (low risk, immediate operational value).
2. Prototype Priority 2 as opt-in mode behind explicit CLI flag and artifact version.
3. Add differential tests:
- same input/witness must match expected edited image and `op_mask`
- verifier must reject mode/version mismatch
- negative tests for oversized inputs and policy violations
4. Promote new mode to default only after benchmark and security sign-off.

## Execution Milestones And Tasks

### Milestone M1: Operational hardening baseline (1-2 weeks)

Deliverables:

1. Add proving mode flag and explicit mode reporting in CLI/API output.
2. Add max-image-size/max-pixel guardrails for `edit-and-prove*`.
3. Add default run-metrics persistence for proving/emulation.

Acceptance criteria:

1. Commands fail fast on oversized input with deterministic error messages.
2. Mode is present in machine-readable output artifacts/logs.
3. CI captures per-run benchmark artifacts for regression comparison.

Task checklist:

- [ ] Add `--proof-mode` flag in prover CLI (`fast-local`, `high-assurance` placeholder policy labels).
- [ ] Surface proof mode in JSON/public reporting path (without breaking existing verifier behavior).
- [ ] Add `--max-input-pixels` with sane default and override.
- [ ] Persist benchmark rows to `artifacts/perf_history.csv` (append mode).
- [ ] Add CI job gate for max-cycle thresholds on commit-tier matrix.

### Milestone M2: Versioned artifact schema + verifier gating (1-2 weeks)

Deliverables:

1. Versioned statement/artifact schema.
2. Verifier rejection on unsupported or mismatched mode/version.

Acceptance criteria:

1. Old artifacts still verify under compatibility mode.
2. New artifacts include explicit `statement_version` and `proof_mode`.
3. Negative tests prove mismatch rejection.

Task checklist:

- [ ] Add `statement_version` and `proof_mode` fields to public artifact JSON.
- [ ] Update `verify` and `verify-c2pa-proof` to enforce compatibility rules.
- [ ] Add golden fixtures for at least two schema versions.
- [ ] Add negative fixtures for mode/version mismatch.

### Milestone M3: Performance-path prototype (2-4 weeks)

Deliverables:

1. Optional performance-oriented proving path with explicit trust boundary.
2. Bench report comparing current full-source path vs new path.

Acceptance criteria:

1. New mode is opt-in only.
2. Benchmarks show material resource reduction on >=1024x1024 inputs.
3. Security note clearly states statement difference from full-source mode.

Task checklist:

- [ ] Implement alternate witness/commit flow for bounded working set mode.
- [ ] Keep `op_mask` semantics unchanged.
- [ ] Preserve existing full-source path unchanged as reference.
- [ ] Add side-by-side perf matrix output (`full_source` vs `bounded_mode`).
- [ ] Add verifier policy check that disallows bounded mode where full assurance is required.

### Milestone M4: Security and production readiness review (1-2 weeks)

Deliverables:

1. Threat model document.
2. Key-management and trust-policy integration plan.
3. Final go/no-go checklist for production pilot.

Acceptance criteria:

1. Threat model covers spoofing, tampering, downgrade, and DoS cases.
2. Key custody plan is HSM/KMS based and rotation-tested.
3. Trust anchor/revocation policy is explicitly documented and test-covered.

Task checklist:

- [ ] Add `docs/threat-model.md`.
- [ ] Add `docs/trust-policy.md`.
- [ ] Add `docs/key-management.md`.
- [ ] Add security regression tests for downgrade and policy-bypass attempts.
- [ ] Add release checklist section to README for pilot readiness.

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
