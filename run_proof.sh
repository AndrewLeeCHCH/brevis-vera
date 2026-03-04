#!/bin/bash
# Test script to run zk-proof-cli with a proof-id, then verify the proof

if [ -z "$1" ]; then
    echo "Usage: $0 <proof-id> [options]"
    echo ""
    echo "Arguments:"
    echo "  proof-id    The UUID from the server response"
    echo ""
    echo "Options:"
    echo "  --verbose   Enable verbose output"
    echo "  --log       Save output to logs/<proof-id>.log"
    echo "  --skip-verify  Skip proof verification"
    echo "  --help      Show this help message"
    echo ""
    echo "Example:"
    echo "  $0 1f1b8c3f-5551-46b0-bb1a-0e7724692b5a --verbose"
    exit 0
fi

PROOF_ID="$1"
shift

# Parse options
SAVE_LOG=false
SKIP_VERIFY=false
VERBOSE=""
while [ $# -gt 0 ]; do
    case "$1" in
        --log)
            SAVE_LOG=true
            ;;
        --skip-verify)
            SKIP_VERIFY=true
            ;;
        --verbose)
            VERBOSE="--verbose"
            ;;
        *)
            break
            ;;
    esac
    shift
done

# Get the directory where this script is located
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"

# Change to project root
cd "$SCRIPT_DIR"

# Create logs directory
mkdir -p logs
mkdir -p proof_data

# Run the CLI
LOG_FILE=""
if [ "$SAVE_LOG" = true ]; then
    LOG_FILE="logs/${PROOF_ID}.log"
    echo "Saving output to: $LOG_FILE"
    CLI_OUTPUT=$(./target/release/zk-proof-cli --proof-id "$PROOF_ID" $VERBOSE 2>&1 | tee "$LOG_FILE")
else
    CLI_OUTPUT=$(./target/release/zk-proof-cli --proof-id "$PROOF_ID" $VERBOSE 2>&1)
fi

# Check if CLI succeeded
CLI_EXIT_CODE=${PIPESTATUS[0]}
echo "$CLI_OUTPUT" | tail -5
if [ $CLI_EXIT_CODE -ne 0 ]; then
    echo ""
    echo "Error: CLI exited with code $CLI_EXIT_CODE"
    exit $CLI_EXIT_CODE
fi

# Extract public values from JSON output
echo ""
echo "=== Extracting proof data for verification ==="

# Extract public values
PUBLIC_VALUES=$(echo "$CLI_OUTPUT" | grep -o '"public_values": "[^"]*"' | cut -d'"' -f4)

if [ -z "$PUBLIC_VALUES" ]; then
    echo "Error: Could not extract public_values from CLI output"
    echo "CLI output was:"
    echo "$CLI_OUTPUT"
    exit 1
fi

echo "Public values (hex): ${PUBLIC_VALUES:0:64}..."

# Extract final image hash (first 64 hex chars = 32 bytes)
FINAL_IMAGE_HASH="${PUBLIC_VALUES:0:64}"
echo "Final image hash: $FINAL_IMAGE_HASH"

# Extract number of operations
NUM_OPS=$(echo "$CLI_OUTPUT" | grep -o '"num_operations": [0-9]*' | grep -o '[0-9]*')
echo "Number of operations: $NUM_OPS"

# Save proof data to proof_data directory for verifier
echo "$PUBLIC_VALUES" | xxd -r -p > proof_data/c2pa_public_values.bin

# The proof is in base64 - we need to decode it
PROOF_B64=$(echo "$CLI_OUTPUT" | grep -o '"proof": "[^"]*"' | cut -d'"' -f4)
if [ -n "$PROOF_B64" ]; then
    echo "$PROOF_B64" | base64 -d > proof_data/c2pa_proof.bin
    echo "Proof saved to proof_data/c2pa_proof.bin ($(stat -f%z proof_data/c2pa_proof.bin 2>/dev/null || stat -c%s proof_data/c2pa_proof.bin 2>/dev/null) bytes)"
fi

# Verify the proof
if [ "$SKIP_VERIFY" = false ]; then
    echo ""
    echo "=== Running Proof Verification ==="

    # Check if output image exists and compute its hash
    OUTPUT_IMAGE="output.jpg"
    if [ -f "$OUTPUT_IMAGE" ]; then
        COMPUTED_HASH=$(openssl dgst -sha256 "$OUTPUT_IMAGE" | sed 's/.*= //')
        echo "Computed hash from output.jpg: $COMPUTED_HASH"

        # Compare hashes (convert to lowercase for comparison)
        COMPUTED_HASH_LOWER=$(echo "$COMPUTED_HASH" | tr '[:upper:]' '[:lower:]')
        FINAL_HASH_LOWER=$(echo "$FINAL_IMAGE_HASH" | tr '[:upper:]' '[:lower:]')

        if [ "$COMPUTED_HASH_LOWER" = "$FINAL_HASH_LOWER" ]; then
            echo "[OK] Image hash verification PASSED"
        else
            echo "[WARN] Image hash mismatch!"
            echo "  Expected: $FINAL_HASH_LOWER"
            echo "  Got:      $COMPUTED_HASH_LOWER"
        fi
    else
        echo "Note: Output image not found at $OUTPUT_IMAGE, skipping hash comparison"
    fi

    # Run the verifier CLI
    echo ""
    echo "Running c2pa-verifier..."
    ./target/release/c2pa-verifier

    VERIFY_EXIT_CODE=$?

    echo ""
    if [ $VERIFY_EXIT_CODE -eq 0 ]; then
        echo "=== VERIFICATION COMPLETE ==="
        echo "Proof verified successfully!"
    else
        echo "=== VERIFICATION FAILED ==="
        echo "Proof verification exited with code $VERIFY_EXIT_CODE"
    fi

    exit $VERIFY_EXIT_CODE
else
    echo ""
    echo "=== Skipping proof verification (--skip-verify flag) ==="
fi
