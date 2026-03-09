#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"

TIER="${1:-commit}"
OUT="artifacts/perf_matrix_${TIER}.csv"
mkdir -p artifacts

cargo build --release -p brevis-vera-prover >/dev/null

target/release/brevis-vera-prover prepare-series \
  --input-image DSC00050.JPG \
  --output-dir samples/series \
  --sizes 256,512,1024,2048,4096 \
  --ext jpg >/dev/null

if [[ "$TIER" == "commit" ]]; then
  SIZES=(256 512 1024)
  ITER=1
  WARM=0
elif [[ "$TIER" == "nightly" ]]; then
  SIZES=(256 512 1024 2048 4096)
  ITER=1
  WARM=0
else
  echo "Unknown tier: $TIER (use commit|nightly)" >&2
  exit 2
fi

echo 'tier,label,width,height,pixels,cycles,emu_ms,accessed_addrs' > "$OUT"

for s in "${SIZES[@]}"; do
  img="samples/series/series_${s}x${s}.jpg"
  meta="artifacts/perf_series_${s}_meta.json"

  target/release/brevis-vera-prover mock-sign \
    --input-image "$img" \
    --metadata-out "$meta" \
    --private-key-pem artifacts/mock_signer_key.pem >/dev/null

  out=$(target/release/brevis-vera-prover emulate-cycles \
    --input-image "$img" \
    --metadata "$meta" \
    --crop-x 10 --crop-y 10 --crop-w 64 --crop-h 64 \
    --brightness-delta 20 \
    --iterations "$ITER" --warmup "$WARM" 2>&1)

  cyc=$(echo "$out" | sed -n 's/.*avg=\([0-9][0-9]*\).*/\1/p' | head -n1)
  emu=$(echo "$out" | sed -n 's/Emulation time: avg=\([0-9.][0-9.]*\)ms.*/\1/p' | head -n1)
  addrs=$(echo "$out" | sed -n 's/.*accessed_addrs len: \([0-9][0-9]*\).*/\1/p' | head -n1)
  px=$((s*s))

  echo "$TIER,series_${s}x${s},$s,$s,$px,$cyc,$emu,$addrs" | tee -a "$OUT"
done

# Commit-tier hard caps to catch big regressions.
if [[ "$TIER" == "commit" ]]; then
  cap_256="${MAX_CYCLES_256:-8000000}"
  cap_512="${MAX_CYCLES_512:-30000000}"
  cap_1024="${MAX_CYCLES_1024:-120000000}"

  python3 - "$OUT" "$cap_256" "$cap_512" "$cap_1024" <<'PY'
import csv,sys
path,cap256,cap512,cap1024=sys.argv[1:]
cap={"series_256x256":int(cap256),"series_512x512":int(cap512),"series_1024x1024":int(cap1024)}
with open(path,newline='') as f:
    rows=list(csv.DictReader(f))
for r in rows:
    label=r['label']
    cyc=int(r['cycles'])
    if label in cap and cyc>cap[label]:
        raise SystemExit(f"perf regression: {label} cycles={cyc} > cap={cap[label]}")
print("perf caps passed")
PY
fi

echo "perf matrix written: $OUT"
