# Origence · Documentation

**English** · [简体中文](README.md)

Origence is the unified data foundation for AI agents, covering memory, knowledge, Skills, and Trace. AnySeek provides full-text, vector, and one-hop graph retrieval. SQLite holds authoritative metadata and durable records. Blob storage preserves original files.

The first release targets pilot use by internal engineering and business teams on enterprise intranets, initially with Claude Code and Codex.

## Start here

| What you need | Documentation |
| --- | --- |
| Build and start | [Project README](../README.md) |
| Node.js management UI | [Startup, configuration, and testing](../ui/README.en.md) |
| Product positioning, core data, and milestones | [Product roadmap](ROADMAP.en.md) |
| Confirmed product scope, target architecture, and initial acceptance | [Architecture plan](design/agent-data-foundation.en.md) |
| What is implemented, blockers, and next steps | [STATUS](STATUS.en.md) |
| Current components, storage responsibilities, and consistency | [Implementation architecture](design/current-implementation.en.md) |
| HTTP, MCP, and public library interfaces | [API](API.en.md) |
| Configuration, deployments, failures, backup and restore | [OPERATIONS](OPERATIONS.en.md) |
| Claude Code / Codex integration and Skill export | [CLIENTS](CLIENTS.en.md) |
| Test results, failures, and verification limits for precise revisions | [VALIDATION](VALIDATION.en.md) |

The target architecture describes the product to be delivered. Implementation status is maintained in STATUS, and test results in VALIDATION. Domain contracts do not repeat milestone schedules or historical test counts.

## Implementation references

| Domain contract | Responsibilities |
| --- | --- |
| [Memory identity and semantics](contracts/memory.en.md) | Identity, conditions, versions, single or multiple values, and source withdrawal |
| [Controlled extraction](contracts/semantic-capture.en.md) | Model candidates, explicit authorization, source text validation, and durable plans |
| [Improve / Learning](contracts/learning.md) | Learning from explicit feedback, applicability conditions, and the complete support chain |
| [EvidenceBundle](contracts/evidence-bundle.md) | Retrieval lanes, budgets, conflicts, and citations |
| [Skill](contracts/skills.md) | Private candidates, selected review packages, approval, publication, and distribution |
| [Trace / Run](contracts/runs.md) | Private traces, context preparation records, client reports, and collection gaps |
| [External Run verification](contracts/run-verification.md) | External assessment records for precise Run prefixes and their withdrawal |
| [Storage invariants](contracts/storage.en.md) | Identity reservations, publication ledger, original evidence, and same-version recovery |
| [Owning workspace and message batches](API.en.md#common-owning-workspace-and-message-batch-library-apis) | Common library interfaces, derived authorization, and batch publication state |

The [evaluation and comparison standard](Origence_%E6%95%88%E6%9E%9C%E8%AF%84%E4%BC%B0%E4%B8%8E%E5%AF%B9%E6%AF%94%E6%A0%87%E5%87%86.en.md) defines the evaluation method. See [evals](../evals/README.en.md) for runnable entry points. Enterprise pilot and competitive comparisons have separate completion criteria.

## Historical material

Earlier integration logs, resolved architecture reviews, inventory records, dependency audits, and per-milestone acceptance documents are no longer active documentation. For reference, see the [pinned revision before cleanup](https://github.com/TheLudlows/Origence/tree/0a1a56c1da74e78be0118782b04a3f5a37251060/docs). Dated original verification artifacts are in the [archive at the pinned commit](https://github.com/TheLudlows/Origence/tree/0a1a56c1da74e78be0118782b04a3f5a37251060/docs/evidence). See [STATUS](STATUS.en.md) for current progress.

Each document comes in Simplified Chinese and English. Use the links at the top to switch languages. Update both versions in the same change and keep sections, tables, commands, parameters, numbers, and verification conclusions aligned. Code examples and original evidence use the same content.
