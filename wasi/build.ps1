#!/usr/bin/env pwsh
# Build the mf4-rs WASI Preview 2 library component, build the example command
# components, compose them with `wac`, and run each composed component with
# `wasmtime`.
#
# Usage:
#   ./wasi/build.ps1            # build, compose, and run every example
#   ./wasi/build.ps1 -NoRun     # build and compose only (skip wasmtime run)

[CmdletBinding()]
param(
    [switch]$NoRun
)

$ErrorActionPreference = 'Stop'

# Resolve repository root (parent of this script's directory).
$RepoRoot = Split-Path -Parent $PSScriptRoot
Push-Location $RepoRoot
try {
    $Target = 'wasm32-wasip2'
    $LibWasm = Join-Path $RepoRoot "target/$Target/release/mf4_rs.wasm"
    $ExampleDir = Join-Path $RepoRoot 'wasi_examples'
    $ExampleOut = Join-Path $ExampleDir "target/$Target/release"
    $ComposedDir = Join-Path $RepoRoot 'target/wasi-composed'

    $Examples = @(
        'write_file',
        'read_file',
        'index_operations',
        'cut_file',
        'merge_files',
        'visualize_layout'
    )

    Write-Host '==> Building library component (features = wasip2)' -ForegroundColor Cyan
    cargo build --release --target $Target --features wasip2
    if (-not (Test-Path $LibWasm)) {
        throw "Library component not found at $LibWasm"
    }

    Write-Host '==> Building example command components' -ForegroundColor Cyan
    Push-Location $ExampleDir
    try {
        cargo build --release --target $Target
    } finally {
        Pop-Location
    }

    New-Item -ItemType Directory -Force -Path $ComposedDir | Out-Null

    foreach ($name in $Examples) {
        $exampleWasm = Join-Path $ExampleOut "$name.wasm"
        $composedWasm = Join-Path $ComposedDir "$name.composed.wasm"

        if (-not (Test-Path $exampleWasm)) {
            throw "Example component not found: $exampleWasm"
        }

        Write-Host "==> Composing $name" -ForegroundColor Cyan
        wac plug $exampleWasm --plug $LibWasm -o $composedWasm

        if ($NoRun) {
            Write-Host "    composed -> $composedWasm"
            continue
        }

        Write-Host "==> Running $name" -ForegroundColor Green
        wasmtime run --dir . $composedWasm
        Write-Host ''
    }

    Write-Host 'All examples built and composed successfully.' -ForegroundColor Green
} finally {
    Pop-Location
}
