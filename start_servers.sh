#!/bin/bash

# Start both backend and frontend servers for image-editor

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"

echo "=== Starting Image Editor Servers ==="

# Start backend server (Rust Axum server on port 3000)
echo "[1/2] Starting backend server on port 3000..."
cd "$SCRIPT_DIR/image-processor/server"
cargo run &
BACKEND_PID=$!

# Wait for backend to start
sleep 3

# Start frontend server (Python HTTP server on port 8080)
echo "[2/2] Starting frontend server on port 8080..."
cd "$SCRIPT_DIR/image-editor-frontend"
python3 -m http.server 8080 &
FRONTEND_PID=$!

echo ""
echo "=== Servers Started ==="
echo "Backend:  http://localhost:3000"
echo "Frontend: http://localhost:8080"
echo ""
echo "Backend PID:  $BACKEND_PID"
echo "Frontend PID: $FRONTEND_PID"
echo ""
echo "Press Ctrl+C to stop servers"

# Wait for Ctrl+C
trap "echo ''; echo 'Stopping servers...'; kill $BACKEND_PID $FRONTEND_PID 2>/dev/null; exit 0" SIGINT SIGTERM
wait
