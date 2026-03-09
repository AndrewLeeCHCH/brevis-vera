#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"

OUT_DIR="artifacts/ci_negative"
mkdir -p "$OUT_DIR"

cargo build --release -p brevis-vera-prover >/dev/null

target/release/brevis-vera-prover mock-sign \
  --input-image samples/input.png \
  --metadata-out "$OUT_DIR/meta_ok.json" \
  --private-key-pem artifacts/mock_signer_key.pem >/dev/null

target/release/brevis-vera-prover edit-and-prove \
  --input-image samples/input.png \
  --metadata "$OUT_DIR/meta_ok.json" \
  --crop-x 1 --crop-y 1 --crop-w 4 --crop-h 4 \
  --brightness-delta 20 \
  --edited-image-out "$OUT_DIR/edited_ok.png" \
  --riscv-proof-out "$OUT_DIR/proof_ok.bin" \
  --public-values-out "$OUT_DIR/pv_ok.json" >/dev/null

expect_fail() {
  local name="$1"
  shift
  if "$@" >/tmp/neg_${name}.out 2>&1; then
    echo "negative_failed name=$name reason=unexpected_success"
    cat /tmp/neg_${name}.out
    exit 1
  fi
  echo "negative_ok name=$name"
}

# 1) Tampered edited image should fail verification.
cp "$OUT_DIR/edited_ok.png" "$OUT_DIR/edited_tampered.png"
python3 - "$OUT_DIR/edited_tampered.png" <<'PY'
import sys
p=sys.argv[1]
with open(p,'r+b') as f:
    f.seek(120)
    b=f.read(1)
    if not b:
        raise SystemExit('cannot tamper file')
    f.seek(120)
    f.write(bytes([b[0]^0x01]))
PY
expect_fail tampered_edited \
  target/release/brevis-vera-prover verify \
    --edited-image "$OUT_DIR/edited_tampered.png" \
    --metadata "$OUT_DIR/meta_ok.json" \
    --riscv-proof "$OUT_DIR/proof_ok.bin"

# 2) Wrong metadata should fail verification.
target/release/brevis-vera-prover mock-sign \
  --input-image samples/series/series_256x256.jpg \
  --metadata-out "$OUT_DIR/meta_wrong.json" \
  --private-key-pem artifacts/mock_signer_key.pem >/dev/null
expect_fail wrong_metadata \
  target/release/brevis-vera-prover verify \
    --edited-image "$OUT_DIR/edited_ok.png" \
    --metadata "$OUT_DIR/meta_wrong.json" \
    --riscv-proof "$OUT_DIR/proof_ok.bin"

# 3) Corrupted proof file should fail verification.
cp "$OUT_DIR/proof_ok.bin" "$OUT_DIR/proof_bad.bin"
python3 - "$OUT_DIR/proof_bad.bin" <<'PY'
import sys
p=sys.argv[1]
with open(p,'r+b') as f:
    f.seek(64)
    b=f.read(1)
    if not b:
        raise SystemExit('cannot tamper proof')
    f.seek(64)
    f.write(bytes([b[0]^0x80]))
PY
expect_fail tampered_proof \
  target/release/brevis-vera-prover verify \
    --edited-image "$OUT_DIR/edited_ok.png" \
    --metadata "$OUT_DIR/meta_ok.json" \
    --riscv-proof "$OUT_DIR/proof_bad.bin"

echo "negative: all checks passed"
