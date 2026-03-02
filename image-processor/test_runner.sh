#!/bin/bash

# Test script for image-processor
# Runs the prover with specified operations and saves results to JSON

set -e

# Configuration
SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
ORIGINAL_IMAGE="$SCRIPT_DIR/../DSC00050.JPG"
OUTPUT_IMAGE="$SCRIPT_DIR/test_output.jpg"
PROOF_DIR="$SCRIPT_DIR/../test_proofs"

# Create proof directory
mkdir -p "$PROOF_DIR"

# Default operations
OPERATIONS=("crop,100,100,800,600" "resize,400,300")

# Override with command line arguments if provided
if [ $# -gt 0 ]; then
    OPERATIONS=("$@")
fi

echo "=== Image Processor Test Script ==="
echo "Original image: $ORIGINAL_IMAGE"
echo "Operations: ${OPERATIONS[*]}"
echo ""

# Build the prover first
echo "[1/4] Building prover..."
cd "$SCRIPT_DIR/.."
RUSTUP_TOOLCHAIN=nightly-2025-08-04 cargo build -p c2pa-prover --release

# Build the ELF
echo "[2/4] Building ELF..."
cd "$SCRIPT_DIR/app"
RUSTUP_TOOLCHAIN=nightly-2025-08-04 cargo pico build

# Run the prover with operations
echo "[3/4] Running prover with operations..."

# Build the command arguments
PROVER_ARGS="$ORIGINAL_IMAGE"
for op in "${OPERATIONS[@]}"; do
    PROVER_ARGS="$PROVER_ARGS --op $op"
done
PROVER_ARGS="$PROVER_ARGS -o $OUTPUT_IMAGE"

echo "Running: cargo run --release -p c2pa-prover -- $PROVER_ARGS"

cd "$SCRIPT_DIR/.."
RUSTUP_TOOLCHAIN=nightly-2025-08-04 cargo run --release -p c2pa-prover -- $PROVER_ARGS 2>&1 | tee "$PROOF_DIR/prover_output.log"

# Parse the output to get the final hash and number of operations
FINAL_HASH=$(grep "Final image hash:" "$PROOF_DIR/prover_output.log" | tail -1 | sed 's/.*Final image hash: \[//' | sed 's/\]//' | tr -d ' ' | tr ',' '\n' | tr -d '\n')
NUM_OPS=$(grep "Number of operations:" "$PROOF_DIR/prover_output.log" | tail -1 | sed 's/.*Number of operations: //')

echo ""
echo "Extracted from prover output:"
echo "  Final hash: $FINAL_HASH"
echo "  Num ops: $NUM_OPS"

# Create test case JSON
TIMESTAMP=$(date -u +"%Y-%m-%dT%H:%M:%SZ")

# Convert operations array to JSON
OPERATIONS_JSON="["
first=true
for op in "${OPERATIONS[@]}"; do
    if [ "$first" = true ]; then
        first=false
    else
        OPERATIONS_JSON+=","
    fi

    # Parse operation type and params
    op_type=$(echo "$op" | cut -d',' -f1)
    params=$(echo "$op" | cut -d',' -f2-)
    OPERATIONS_JSON+="{\"type\":\"$op_type\",\"params\":\"$params\"}"
done
OPERATIONS_JSON+="]"

# Create the JSON file
cat > "$PROOF_DIR/test_case.json" << EOF
{
    "timestamp": "$TIMESTAMP",
    "original_image": "DSC00050.JPG",
    "output_image": "test_output.jpg",
    "operations": $OPERATIONS_JSON,
    "num_operations": $NUM_OPS,
    "final_image_hash": "$FINAL_HASH"
}
EOF

# Copy the output image to the proof directory
cp "$OUTPUT_IMAGE" "$PROOF_DIR/"

# Copy proof files
cp proof_data/c2pa_proof.bin "$PROOF_DIR/" 2>/dev/null || true
cp proof_data/c2pa_public_values.bin "$PROOF_DIR/" 2>/dev/null || true

echo ""
echo "=== Test Case Complete ==="
echo "Output image: $OUTPUT_IMAGE"
echo "Proof directory: $PROOF_DIR"
echo ""
echo "Test case JSON:"
cat "$PROOF_DIR/test_case.json"
echo ""
echo "Files in $PROOF_DIR:"
ls -la "$PROOF_DIR"
