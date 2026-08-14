#!/usr/bin/env bash
# Build the mf4-rs WASI Preview 2 library component, build the example command
# components, compose them with `wac`, and run each composed component with
# `wasmtime`.
#
# Usage:
#   ./wasi/build.sh            # build, compose, and run every example
#   ./wasi/build.sh --no-run   # build and compose only (skip wasmtime run)

set -euo pipefail

NO_RUN=0
if [[ "${1:-}" == "--no-run" ]]; then
    NO_RUN=1
fi

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"
cd "$REPO_ROOT"

TARGET="wasm32-wasip2"
LIB_WASM="$REPO_ROOT/target/$TARGET/release/mf4_rs.wasm"
EXAMPLE_DIR="$REPO_ROOT/wasi_examples"
EXAMPLE_OUT="$EXAMPLE_DIR/target/$TARGET/release"
COMPOSED_DIR="$REPO_ROOT/target/wasi-composed"

EXAMPLES=(
    write_file
    read_file
    index_operations
    cut_file
    merge_files
    visualize_layout
)

echo "==> Building library component (features = wasip2)"
cargo build --release --target "$TARGET" --features wasip2
if [[ ! -f "$LIB_WASM" ]]; then
    echo "Library component not found at $LIB_WASM" >&2
    exit 1
fi

echo "==> Building example command components"
(cd "$EXAMPLE_DIR" && cargo build --release --target "$TARGET")

mkdir -p "$COMPOSED_DIR"

for name in "${EXAMPLES[@]}"; do
    example_wasm="$EXAMPLE_OUT/$name.wasm"
    composed_wasm="$COMPOSED_DIR/$name.composed.wasm"

    if [[ ! -f "$example_wasm" ]]; then
        echo "Example component not found: $example_wasm" >&2
        exit 1
    fi

    echo "==> Composing $name"
    wac plug "$example_wasm" --plug "$LIB_WASM" -o "$composed_wasm"

    if [[ "$NO_RUN" -eq 1 ]]; then
        echo "    composed -> $composed_wasm"
        continue
    fi

    echo "==> Running $name"
    wasmtime run --dir . "$composed_wasm"
    echo ""
done

echo "All examples built and composed successfully."
