# Origence

**The Intelligence Foundation for AI Agents.**

Origence is a Rust-based foundation for agent memory and knowledge. It provides scoped memory and knowledge storage, retrieval, and evidence-aware lifecycle management through HTTP and MCP.

## Features

- **Memory & Knowledge** — Store structured memories and ingest documents.
- **Retrieval** — Native LanceDB full-text/vector search and graph-enhanced evidence.
- **Storage** — Native Rust Turso authority, local LanceDB indexes, and original blobs.
- **Governance** — Workspace isolation, versioning, provenance, and revocation.

## Quick start

Requires Rust **1.98.0**, a C/C++ compiler, CMake, and a native build tool
(Make on Linux or Visual Studio C++ Build Tools on Windows). Cargo automatically
builds the protobuf compiler from source through Lance; no separate `protoc`
installation or `PROTOC` setup is needed. The first native build is slower; Cargo
caches the result for subsequent builds. On Windows, use an x64 MSVC developer
shell or `./tools/build.ps1 -Action Build`, which initializes that shell.
The repository Cargo configuration keeps protobuf and Abseil on the same MSVC
runtime for native Windows x64 builds. Existing generic or target-specific
CMake toolchain environment variables take precedence. If supplying a custom
Windows toolchain, include `tools/cmake/protobuf-msvc.cmake` from that toolchain.

`cargo build --locked --no-default-features --lib` skips Lance and the protobuf
compiler for authority-only library development.

```sh
cargo build --locked
./target/debug/origence --offline workspace-create demo
./target/debug/origence serve
```

Use the workspace token returned by `workspace-create` to access the API.

## Documentation

[Documentation index](docs/README.md) · [API and library changes](docs/API.md) · [Operations](docs/OPERATIONS.md) · [Project status](docs/STATUS.md)

The index separates current contracts and operations from version-specific validation and historical research. AML production endpoints remain documented in the API; AML-specific test and experiment facilities have been removed.

Evaluation entry points: [retrieval](evals/retrieval/README.md) and [Agent comparison](evals/agent/README.md).

Implementation rules for contributors and coding agents: [AGENTS.md](AGENTS.md). The JOIN-free SQL constraint remains in force; its former automatic guard and tests have been removed.

> The CLI is now named `origence`. Existing `OC_*` configuration variables and HTTP endpoints remain unchanged. The graph backend now uses `graph.db`; see [Operations](docs/OPERATIONS.md) for the rebuild boundary.
