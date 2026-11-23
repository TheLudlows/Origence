# Origence · Product goals and long-term roadmap

**English** · [简体中文](ROADMAP.md)

**Origence is the unified data foundation for AI agents, covering memory, knowledge, Skills, and Trace.** Agents can read data that matches the current task and their permissions. They can submit task records for users to verify outcomes and decide which experiences to reuse.

This document describes product goals and milestone acceptance. The initial release pilots Claude Code and Codex on enterprise intranets for engineering and business teams. See the [architecture plan](design/agent-data-foundation.en.md) for the first release's technical contracts, [STATUS](STATUS.en.md) for implementation progress, and [VALIDATION](VALIDATION.md) for specific verification evidence.

## Core data

| Data | What it manages |
| --- | --- |
| Memory | Project facts, preferences, decisions, and reusable experiences with conditions, versions, and provenance. |
| Knowledge | Documents, team standards, and business material with publication, updates, withdrawal, and precise version citations. |
| Skill | Reusable methods, procedures, and references. Selected experiences can form private candidates that people with publishing permission share after approving the precise version. |
| Trace | Uses Run to organize evidence, including server-prepared context, client-reported Skill loads, tools and results, collection gaps, and external verification. |

## How the data is used

- **Retrieval and context assembly** select evidence by task, permission, and budget, and record context preparation in a specified Run.
- **Private archives and reusable experiences** retain explicitly submitted conversations and feedback. A logical key can query its own archive. In the explicit Improve workflow, eligible feedback can form Learning, and selected experiences can serve as the basis for Skill candidates.
- **Traceability and evaluation** use Trace to verify evidence for specific tasks and compare versions and outcomes on fixed tasks. Fact maintenance, knowledge publication, and Skill review can proceed independently.

The platform exposes these functions through HTTP, MCP, and the management UI. Logical keys isolate private archives, Runs, and unpublished candidates. Published memory, knowledge, and Skills belong to a workspace, are available to authorized members, and follow source, version, and withdrawal rules.

## Milestones

| Milestone | Delivery priorities | Acceptance criteria |
| --- | --- | --- |
| **P0 · Intranet pilot** | Private archives and isolation in Claude Code / Codex. Fact correction, and knowledge publication and withdrawal. Learning from feedback, and candidate creation from selected experience followed by review and publication. Runs link delivery and verification. | The same version passes real-client pilots for database troubleshooting and expense auditing. Check exact citations, actual Skill loads, isolation, and withdrawal, and obtain independent sign-off. Exports, client self-reports, or successful scripts cannot substitute for acceptance. Track in [#91](https://github.com/TheLudlows/Origence/issues/91). |
| **P1 · Analysis and evaluation** | Freeze evaluation datasets. Fill gaps in tiered calls, retries, aggregation, and measurable actual usage and costs. Improve original-text locators and governance operations. | Reproduce comparisons on identical tasks, report task success and evidence verification coverage separately, and retain configurations, versions, and denominators. Mark uncollected measurements as unknown instead of zero. |
| **P2 · Expansion and automatic distillation** | Extend hosts and external telemetry as needed. Explore automatically proposing candidates across Runs, version regression alerts, enterprise identity, and more granular source authorization. | Have a named business owner and enough verified samples to evaluate candidate quality, false positives, and the cost of manual correction. Verify revocation and cross-workspace isolation. Automatic proposals do not imply automatic publication. |

Progress through milestones depends on data reliability, reuse of experience, and task outcomes. New hosts, metrics, and automation follow the same authorization and evidence requirements.

## Scope and open decisions

- The agent host handles inference, task orchestration, and business tool execution. Skill publication and tool execution each follow their own authorization rules.
- Keep the established split of [SQLite for authoritative data, AnySeek for retrieval, and Blob for original files](design/agent-data-foundation.en.md). A new retrieval or database engine is outside this roadmap. The current stage targets new databases. Same-version recovery still needs verification.
- Managed deployment, production cost thresholds, ContextBase interoperability, and Agent one integration are separate decisions based on demand and are not committed deliverables.
