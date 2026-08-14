# Cross-platform task runner for the mf4-rs WASI Preview 2 component.
#
# Requires `just` (https://just.systems) plus the wasm32-wasip2 target,
# `wac` (component composer), `wasmtime` (runtime), and optionally
# `wasm-tools` (WIT inspection). Install the tooling with:
#
#   rustup target add wasm32-wasip2
#   cargo install wac-cli wasmtime-cli wasm-tools
#
# Common recipes:
#   just run        # build lib + examples, compose each, run under wasmtime
#   just compose    # build + compose only (skip wasmtime run)
#   just build      # build the library component and all example commands
#   just wit        # print the component's exported WIT
#
# Every recipe runs from the repository root (the justfile's directory) on
# both POSIX shells and Windows (cmd.exe), so composed components and any
# files the examples write land in the repo root.

set windows-shell := ["cmd.exe", "/c"]

target := "wasm32-wasip2"
release-dir := "target" / target / "release"
lib-wasm := release-dir / "mf4_rs.wasm"
examples-dir := "wasi_examples" / "target" / target / "release"

# List available recipes.
default:
    @just --list

# Build the library component (features = wasip2).
build-lib:
    cargo build --release --target {{target}} --features wasip2

# Build the example command components.
build-examples:
    cargo build --release --target {{target}} --manifest-path wasi_examples/Cargo.toml

# Build the library component and every example command.
build: build-lib build-examples

# Print the WIT interface exported by the library component.
wit: build-lib
    wasm-tools component wit {{lib-wasm}}

# Build everything, then compose and run every example under wasmtime.
run: build (_run "write_file") (_run "read_file") (_run "index_operations") (_run "cut_file") (_run "merge_files") (_run "visualize_layout")

# Build everything, then compose every example without running it.
compose: build (_compose "write_file") (_compose "read_file") (_compose "index_operations") (_compose "cut_file") (_compose "merge_files") (_compose "visualize_layout")

# Remove composed component artifacts.
clean:
    cargo clean --target {{target}}

# Compose one example with the library component via `wac plug`.
_compose name:
    wac plug {{examples-dir}}/{{name}}.wasm --plug {{lib-wasm}} -o {{release-dir}}/{{name}}.composed.wasm

# Compose then run one example under wasmtime (granted access to the cwd).
_run name: (_compose name)
    wasmtime run --dir . {{release-dir}}/{{name}}.composed.wasm
