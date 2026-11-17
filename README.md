# Origence

**The Intelligence Foundation for AI Agents.**

Origence is a Rust-based data foundation for internal enterprise agents. It manages memory, knowledge, reviewed Skills, and private archives, with HTTP/MCP access and explicit Run evidence. SQLite holds authoritative data, AnySeek provides retrieval, and Blob storage preserves original files.

SQLite is bundled with the binary through rusqlite; no database service, ORM,
or separate SQLite installation is required. Version hashes are derived from the
original content; ledger registration uses its existing composite key directly.

Library callers can use parsing, identity/value contracts, models, Blob storage
and the AnySeek adapter without a metadata engine:

```sh
cargo test --no-default-features
```

Enable `sqlite` for the standalone authority adapter, or `local-storage` (the
default) for the complete host. Empty read sets do not query metadata; host
transactions validate required permissions once at entry and again at commit.

## Features

- **Context** — Scoped memory and knowledge, private archives, versioning, and source references.
- **Retrieval** — Unified AnySeek full-text, vector, and one-hop graph retrieval with SQLite-authorized evidence.
- **Skills** — Private candidates, exact human approval, immutable publication, and Claude Code / Codex project exports.
- **Run evidence** — Context preparation records, reported tools/Skills/results, and explicit collection gaps.

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

## Management UI

The lightweight [Node.js management UI](ui/README.md) calls Origence's HTTP API
for memory, knowledge, context previews and private Run evidence. Requires Node.js 24.

```sh
cd ui
npm ci --ignore-scripts
npm run build
npm start
```

Open `http://127.0.0.1:3000` and connect with your own API key. Set
`ORIGENCE_API_BASE_URL` when Origence is not at `http://127.0.0.1:8080`.

## Documentation

[Documentation index](docs/README.md) · [API and library interfaces](docs/API.md) · [Operations](docs/OPERATIONS.md) · [Project status](docs/STATUS.md)

The [enterprise Agent data foundation architecture](docs/design/agent-data-foundation.md)
defines the confirmed product scope. Private archives belong to a logical user API key;
published memories, facts, knowledge, and Skills belong to a workspace.
Experience-to-Skill candidate generation and real-client task acceptance remain unfinished;
see [current status](docs/STATUS.md) and [validation](docs/VALIDATION.md).
[Client setup](docs/CLIENTS.md) covers Claude Code and Codex MCP access and approved Skill export.

Evaluation entry points: [retrieval](evals/retrieval/README.md) and [Agent comparison](evals/agent/README.md).

Implementation rules for contributors and coding agents: [AGENTS.md](AGENTS.md). The JOIN-free SQL constraint remains in force; its former automatic guard and tests have been removed.
