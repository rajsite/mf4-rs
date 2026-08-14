# mf4-rs WASI Preview 2 component

This directory holds everything needed to build the **`mf4-rs` WebAssembly
Component** (WASI Preview 2) and to compose and run the example command
components in [`../wasi_examples`](../wasi_examples).

## What gets built

1. **Library component** — `cargo build --release --target wasm32-wasip2
   --features wasip2` compiles `src/wasi/mod.rs` into a component that
   **exports** the `mf4:core/{types,reader,writer,index,ops}` interfaces
   defined in [`wit/world.wit`](wit/world.wit) (world `mf4`). Output:
   `target/wasm32-wasip2/release/mf4_rs.wasm`.
2. **Example commands** — each crate in `../wasi_examples` compiles to a
   command component that **imports** those same interfaces (world
   `mf4-consumer`). Output: `wasi_examples/target/wasm32-wasip2/release/*.wasm`.
3. **Composed components** — [`wac`](https://github.com/bytecodealliance/wac)
   plugs the library's exports into each example's imports, yielding a
   self-contained runnable component per example under
   `target/wasi-composed/`.

Each composed component is then executed with
[`wasmtime`](https://wasmtime.dev/) (`wasmtime run --dir .`), which grants the
component access to the current directory so the examples can read and write
`.mf4` files.

```
wasi_examples/<name>.wasm  (imports mf4:core)  ─┐
                                                ├─ wac plug ─▶ <name>.composed.wasm ─▶ wasmtime run --dir .
target/.../mf4_rs.wasm      (exports mf4:core) ─┘
```

## Prerequisites

```bash
rustup target add wasm32-wasip2
cargo install just           # cross-platform task runner
cargo install wasm-tools     # optional: inspect the component's WIT
cargo install wac-cli        # provides the `wac` composer
cargo install wasmtime-cli   # provides the `wasmtime` runtime
```

## Build & run everything

A cross-platform [`justfile`](../justfile) at the repository root drives the
whole flow (POSIX shells and Windows `cmd.exe`). From the repository root:

```bash
just run        # build lib + examples, compose each, run under wasmtime
just compose    # build + compose only (skip the wasmtime run)
just build      # build the library component and all example commands
just wit        # print the component's exported WIT
```

`just run`:

1. builds the library component,
2. builds all example command components,
3. composes each example with the library via `wac plug`, and
4. runs each composed component with `wasmtime run --dir .`.

## Inspecting the exported interface

```bash
wasm-tools component wit target/wasm32-wasip2/release/mf4_rs.wasm
```

This prints the `mf4:core` package with the `reader`, `writer`, `index`, and
`ops` interfaces plus the shared `types`.
