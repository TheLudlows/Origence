# Origence

**The Intelligence Foundation for AI Agents.**

Origence is a Rust-based foundation for agent memory and knowledge. It provides scoped memory and knowledge storage, retrieval, and evidence-aware lifecycle management through HTTP and MCP.

## Features

- **Memory & Knowledge** — Store structured memories and ingest documents.
- **Retrieval** — Keyword, vector, and graph-enhanced search.
- **Governance** — Workspace isolation, versioning, provenance, and revocation.

## Quick start

Requires Rust **1.98.0** and native build tools (a C/C++ compiler and Protobuf).

```sh
cargo build --locked
./target/debug/origence --offline workspace-create demo
./target/debug/origence serve
```

Use the workspace token returned by `workspace-create` to access the API.

## Documentation

[API](docs/API.md) · [Operations](docs/OPERATIONS.md) · [Project status](docs/STATUS.md) · [Roadmap](docs/superpowers/plans/2026-10-09-next-stage-roadmap.md)

Implementation rules for contributors and coding agents: [AGENTS.md](AGENTS.md).

> The CLI is now named `origence`. Existing `OC_*` configuration variables and HTTP endpoints remain unchanged. The graph backend now uses `graph.db`; see [Operations](docs/OPERATIONS.md) for the rebuild boundary.
