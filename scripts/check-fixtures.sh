#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"

OUT_DIR="artifacts/ci_fixtures"
mkdir -p "$OUT_DIR"

cargo build --release -p brevis-vera-prover >/dev/null
(cd app && cargo pico build >/dev/null)

target/release/brevis-vera-prover prepare-series \
  --input-image samples/input.png \
  --output-dir samples/ci \
  --sizes 64,128,256 \
  --ext png >/dev/null

run_case_emu() {
  local name="$1"
  local input="$2"
  local crop_x="$3"
  local crop_y="$4"
  local crop_w="$5"
  local crop_h="$6"
  local brightness="$7"
  local invert="$8"
  local threshold="$9"
  local rotate="${10}"

  local metadata="$OUT_DIR/${name}_meta.json"

  target/release/brevis-vera-prover mock-sign \
    --input-image "$input" \
    --metadata-out "$metadata" \
    --private-key-pem artifacts/mock_signer_key.pem >/dev/null

  local extra=()
  if [[ "$invert" == "1" ]]; then
    extra+=(--invert)
  fi
  if [[ "$threshold" != "none" ]]; then
    extra+=(--threshold "$threshold")
  fi

  local cmd=(
    target/release/brevis-vera-prover emulate-cycles
    --input-image "$input"
    --metadata "$metadata"
    --crop-x "$crop_x" --crop-y "$crop_y" --crop-w "$crop_w" --crop-h "$crop_h"
    --brightness-delta "$brightness"
    --rotate-quarters "$rotate"
    --iterations 1 --warmup 0
  )
  if [[ ${#extra[@]} -gt 0 ]]; then
    cmd+=("${extra[@]}")
  fi
  local out
  out="$("${cmd[@]}" 2>&1)"
  local cyc
  cyc="$(echo "$out" | sed -n 's/.*avg=\([0-9][0-9]*\).*/\1/p' | head -n1)"
  if [[ -z "$cyc" || "$cyc" == "0" ]]; then
    echo "failed to parse emulation cycles for $name" >&2
    echo "$out" >&2
    exit 1
  fi
  echo "fixture_ok name=$name emulate_cycles=$cyc"
}

run_case_proof() {
  local name="$1"
  local input="$2"
  local crop_x="$3"
  local crop_y="$4"
  local crop_w="$5"
  local crop_h="$6"
  local brightness="$7"
  local invert="$8"
  local threshold="$9"
  local rotate="${10}"

  local metadata="$OUT_DIR/${name}_meta.json"
  local edited="$OUT_DIR/${name}_edited.png"
  local proof="$OUT_DIR/${name}_proof.bin"
  local pv="$OUT_DIR/${name}_pv.json"

  target/release/brevis-vera-prover mock-sign \
    --input-image "$input" \
    --metadata-out "$metadata" \
    --private-key-pem artifacts/mock_signer_key.pem >/dev/null

  local extra=()
  if [[ "$invert" == "1" ]]; then
    extra+=(--invert)
  fi
  if [[ "$threshold" != "none" ]]; then
    extra+=(--threshold "$threshold")
  fi

  local cmd=(
    target/release/brevis-vera-prover edit-and-prove
    --input-image "$input"
    --metadata "$metadata"
    --crop-x "$crop_x" --crop-y "$crop_y" --crop-w "$crop_w" --crop-h "$crop_h"
    --brightness-delta "$brightness"
    --rotate-quarters "$rotate"
    --edited-image-out "$edited"
    --riscv-proof-out "$proof"
    --public-values-out "$pv"
  )
  if [[ ${#extra[@]} -gt 0 ]]; then
    cmd+=("${extra[@]}")
  fi
  "${cmd[@]}" >/dev/null

  target/release/brevis-vera-prover verify \
    --edited-image "$edited" \
    --metadata "$metadata" \
    --riscv-proof "$proof" >/dev/null

  python3 - "$pv" "$invert" "$threshold" "$rotate" <<'PY'
import json,sys
pv_path,invert,threshold,rotate=sys.argv[1:]
with open(pv_path,'r') as f:
    d=json.load(f)
mask=1|2
if invert=='1':
    mask|=4
if threshold!='none':
    mask|=8
if int(rotate)%4!=0:
    mask|=16
assert d['op_mask']==mask, f"op_mask mismatch {d['op_mask']} != {mask}"
assert d['provenance_mode']==0
assert d['provenance_state']==0
assert d['provenance_manifest_hash_hex']=='0'*64
assert len(d['provenance_asset_hash_hex'])==64
assert len(d['original_hash_hex'])==64
assert len(d['edited_hash_hex'])==64
PY

  echo "fixture_ok name=$name proof_and_mask_checked"
}

run_case_emu case_a_emu samples/ci/series_64x64.png 2 2 32 32 15 0 none 0
run_case_emu case_b_emu samples/ci/series_128x128.png 4 4 48 48 20 1 120 1
run_case_proof case_c_proof samples/ci/series_64x64.png 4 4 32 32 10 0 80 2

echo "fixtures: all checks passed"
