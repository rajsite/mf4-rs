# wasi_examples

WASI Preview 2 (WebAssembly Component) ports of the native `examples/` programs.

Each crate here is a **command component**: it imports the `mf4:core`
interfaces (see `../wasi/wit/world.wit`, world `mf4-consumer`) and is composed
against the library component built from the crate root with
`--features wasip2`. Composition is done with [`wac`](https://github.com/bytecodealliance/wac)
(`wac plug`) and the result is run with [`wasmtime`](https://wasmtime.dev/).

## Layout

| Crate | Ports | Demonstrates |
|-------|-------|--------------|
| `write_file` | `examples/write_file.rs` | `mdf-writer` — build a 2-group file, read it back |
| `read_file` | `examples/read_file.rs` | `mdf` reader — walk groups/channels, decode values |
| `index_operations` | `examples/index_operations.rs` | `mdf-index` — build, save, reload, byte ranges, read |
| `cut_file` | `examples/cut_file.rs` | `ops.cut-by-time` |
| `merge_files` | `examples/merge_files.rs` | `ops.merge` |
| `visualize_layout` | `examples/visualize_layout.rs` | `mdf.file-layout-json` |

## Build & run

Requires the `wasm32-wasip2` target, `wasm-tools`, `wac`, and `wasmtime`. From
the repository root:

```bash
# Windows PowerShell
./wasi/build.ps1

# Linux/macOS
./wasi/build.sh
```

The scripts build the library component, build each example command, compose
them with `wac plug`, and run each composed component with
`wasmtime run --dir .` (so the components can read/write files in the current
directory).
