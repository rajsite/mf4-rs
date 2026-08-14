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

Requires the `wasm32-wasip2` target plus `just`, `wac`, and `wasmtime` (and
optionally `wasm-tools`). A cross-platform [`justfile`](../justfile) at the
repository root drives the flow — from the repository root:

```bash
just run        # build lib + examples, compose each, run under wasmtime
just compose    # build + compose only (skip the wasmtime run)
```

`just run` builds the library component, builds each example command, composes
them with `wac plug`, and runs each composed component with
`wasmtime run --dir .` (so the components can read/write files in the current
directory).
