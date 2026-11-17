# Origence

**The Intelligence Foundation for AI Agents.**

Origence is a Rust-based foundation for agent memory and knowledge. It provides scoped memory and knowledge storage, retrieval, and evidence-aware lifecycle management through HTTP and MCP.

## Features

- **Memory & Knowledge** — Store structured memories and ingest documents.
- **Retrieval** — AnySeek full-text and vector search with graph-enhanced evidence.
- **Storage** — Native Rust Turso authority, AnySeek retrieval projections, and original blobs.
- **Governance** — Workspace isolation, versioning, provenance, and revocation.

## Quick start

Requires Rust **1.98.0** and a dedicated AnySeek collection. Follow the
[AnySeek setup](docs/OPERATIONS.md#anyseek-全文与向量) to provision the collection
and set the host configuration before serving. Use a fresh data directory;
existing databases are not migrated by startup.

```sh
cargo build --locked
./target/debug/origence --offline workspace-create demo
./target/debug/origence serve
```

Use the workspace token returned by `workspace-create` to access the API.

## Documentation

[Documentation index](docs/README.md) · [API and library changes](docs/API.md) · [Operations](docs/OPERATIONS.md) · [Project status](docs/STATUS.md)

The proposed [enterprise Agent data foundation architecture](docs/design/agent-data-foundation.md)
treats memory, knowledge, Skills, and runtime state as context, with unified context delivery and run evidence for Claude Code and Codex.
Private sessions are bound to authenticated user API keys; long-term memories, facts, Skills, and knowledge belong to a workspace.
Planned Skills are distilled from memory and Agent experience into proposals with supporting evidence for user review, revision, and publication.
The MVP deployment and Skill approval decisions are confirmed; planned capabilities are separate from the features implemented today.

See [CLI client setup](docs/CLIENTS.md) for Claude Code and Codex MCP configuration.

The index separates current contracts and operations from version-specific validation and historical research. AML production endpoints remain documented in the API; AML-specific test and experiment facilities have been removed.

Evaluation entry points: [retrieval](evals/retrieval/README.md) and [Agent comparison](evals/agent/README.md).

Implementation rules for contributors and coding agents: [AGENTS.md](AGENTS.md). The JOIN-free SQL constraint remains in force; its former automatic guard and tests have been removed.

> The CLI is now named `origence`. Existing `OC_*` configuration variables and HTTP endpoints remain unchanged. AnySeek provides one-hop graph candidates, and Turso still authorizes their source evidence. See [Operations](docs/OPERATIONS.md) for the storage boundary.
