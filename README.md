# Origence

**The unified data foundation for AI agents.**

Origence manages memory, knowledge, Skills, and traces. It provides HTTP and MCP interfaces and a management UI. The first pilot targets internal engineering and business teams using Claude Code and Codex.

## Core data

- **Memory** records scoped facts, preferences, and decisions with version and source references.
- **Knowledge** manages documents and their published versions, with source references and withdrawal.
- **Skills** package reusable methods through private candidates, exact human approval, immutable publication, and Claude Code / Codex project exports.
- **Trace** stores private Run records, including server-side context preparation, reported Skill loads, tools and results, external assessment records, and collection gaps.

Context preparation selects authorized evidence for a task. AnySeek supports full-text, vector, and one-hop graph retrieval, with authorization and source checks in Origence.

## Quick start

Requires Rust **1.98.0** and a compatible AnySeek service. Follow the
[AnySeek setup](docs/OPERATIONS.md#anyseek-全文与向量) to configure the service URL
and credentials. Origence initializes each workspace collection on first use.
Choose a fresh data directory for this quick start.

```sh
cargo build --locked
export OC_DATA_DIR="$(mktemp -d)"
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

## Rust library and storage

The Rust host bundles SQLite through rusqlite and uses it for permissions, version metadata, and durable records. AnySeek stores published text and serves retrieval. Blob storage preserves original files. Version hashes are derived from the original content, and ledger registration uses its existing composite key.

Library callers can use parsing, identity and value contracts, models, Blob storage, and the AnySeek adapter without a metadata engine.

```sh
cargo test --no-default-features
```

Enable `sqlite` for the standalone authority adapter or `local-storage` (the default) for the complete host. Empty read sets skip metadata queries. Host transactions validate required permissions at entry and again at commit.

## Documentation

[Product goals and roadmap](docs/ROADMAP.md) · [Documentation index](docs/README.md) · [API and library interfaces](docs/API.md) · [Operations](docs/OPERATIONS.md) · [Project status](docs/STATUS.md)

Private archives, Run traces, and unpublished candidates belong to a logical API key.
Published memory, knowledge, and Skills belong to a workspace and are available to authorized members. See the [architecture](docs/design/agent-data-foundation.md).

The selected-experience CLI binds a drafted Skill method to one exact source, such as a Run prefix or a published Learning version. Real-client pilot acceptance remains open. See the [CLI guide](docs/SKILL-CANDIDATE-CLI.md), [client setup](docs/CLIENTS.md), and [pilot acceptance issue](https://github.com/TheLudlows/Origence/issues/91).

Evaluation entry points: [retrieval](evals/retrieval/README.md) and [Agent comparison](evals/agent/README.md).

Implementation rules for contributors and coding agents: [AGENTS.md](AGENTS.md). The JOIN-free SQL constraint remains in force; its former automatic guard and tests have been removed.

Windows native development (no WSL): [one-command launcher and separate first-time setup](docs/WINDOWS-DEV.md).
