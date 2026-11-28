# Origence

**English** · [简体中文](README.zh-CN.md)

**The unified data foundation for AI agents.**

Origence manages memory, knowledge, Skills, and traces. It provides HTTP and MCP interfaces and a management UI. The first pilot targets internal engineering and business teams using Claude Code and Codex.

## Core data

- **Memory** records scoped facts, preferences, and decisions with version and source references.
- **Knowledge** manages documents and their published versions, with source references and withdrawal.
- **Skills** package reusable methods through private candidates, exact human approval, immutable publication, and authorized package downloads.
- **Trace** stores private Run records, including server-side context preparation, reported Skill loads, tools and results, external assessment records, and collection gaps.

Authorized workspace members can reuse published memory, knowledge, and Skills.

Context preparation selects authorized evidence for a task. AnySeek supports full-text, vector, and one-hop graph retrieval, with authorization and source checks in Origence.
Agent hosts handle inference, task orchestration, and business tool execution. [Current product scope](docs/design/current-implementation.en.md#1-product-scope)

## Quick start

For Claude Code or Codex setup and local console deployment, see the [installation guide](docs/INSTALLER.en.md).

Requires Rust **1.98.0**. The default build links the AnySeek Rust SDK for local storage.
Choose a fresh data directory for this quick start.

```sh
cargo build --locked
export OC_DATA_DIR="$(mktemp -d)"
./target/debug/origence serve --local
```

First startup writes a random deployment administrator key to `$OC_DATA_DIR/deployment-admin.key`. Use it to sign in to the console and create a workspace. The issued workspace key connects an Agent. The embedded AnySeek catalog lives under `$OC_DATA_DIR/anyseek`.

## Cloud deployment

The Docker build uses the remote AnySeek client. Configure `OC_ANYSEEK_URL` and
`OC_ANYSEEK_API_KEY` for the deployed service. Start the container with its default
command or use `origence serve` on a cloud host. [Cloud configuration](docs/OPERATIONS.en.md#cloud-deployment-with-standalone-anyseek)

## Management UI

The lightweight [Node.js management UI](ui/README.en.md) calls Origence's HTTP API
for memory, knowledge, context previews and private Run evidence. Requires Node.js 24.

```sh
cd ui
npm ci --ignore-scripts
npm run build
npm start
```

Open `http://127.0.0.1:3000` and sign in with a [workspace key](ui/README.en.md#workspace-key-sign-in)
or an [existing console account](ui/README.en.md#existing-account-sign-in). Set
`ORIGENCE_API_BASE_URL` when Origence is not at `http://127.0.0.1:8080`.

## Rust library and storage

SQLite holds permissions, versioned facts, sources, durable jobs, and global knowledge copies. AnySeek stores workspace publication chunks, summaries, and search indexes, with complete Q&A and turn directories in separate private collections. Blob storage holds original uploads. [Storage ownership](docs/design/current-implementation.en.md#storage-design)

Library callers can use parsing, identity and value contracts, models, Blob storage, and the AnySeek adapter without a metadata engine.

```sh
cargo test --no-default-features
```

Enable `sqlite` for the standalone authority adapter. Empty read sets skip metadata queries. Host transactions validate required permissions at entry and again at commit.

## Documentation

[Product goals and roadmap](docs/ROADMAP.en.md) · [Documentation index](docs/README.en.md) · [API and library interfaces](docs/API.en.md) · [Operations](docs/OPERATIONS.en.md) · [Project status](docs/STATUS.en.md)

Session records, Run traces, and unpublished candidates belong to a logical API key.
Published memory, knowledge, and Skills belong to a workspace and are available to authorized members. See the [architecture](docs/design/agent-data-foundation.en.md).

The selected-experience CLI binds a drafted Skill method to one exact source, such as a Run prefix or a published Learning version. Real-client pilot acceptance remains open. See the [CLI guide](docs/SKILL-CANDIDATE-CLI.md), [client setup](docs/CLIENTS.en.md), and [pilot acceptance issue](https://github.com/TheLudlows/Origence/issues/91).

Evaluation entry points: [retrieval](evals/retrieval/README.md) and [Agent comparison](evals/agent/README.md).

Implementation rules for contributors and coding agents: [AGENTS.md](AGENTS.md). The JOIN-free SQL constraint remains in force; its former automatic guard and tests have been removed.

Windows native development (no WSL): [local startup and optional UI launcher](docs/WINDOWS-DEV.en.md).
