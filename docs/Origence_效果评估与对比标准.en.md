# Origence: Effects Evaluation and Comparison Criteria

**English** · [简体中文](Origence_%E6%95%88%E6%9E%9C%E8%AF%84%E4%BC%B0%E4%B8%8E%E5%AF%B9%E6%AF%94%E6%A0%87%E5%87%86.md)

This document sets evaluation methods and thresholds for memory and knowledge in the unified data foundation. Full Agent pilots include private archives, Skill reuse, and Trace verification and are accepted under the product roadmap's P0 conditions. See the [product roadmap](ROADMAP.en.md#milestones) and [P0 acceptance #91](https://github.com/TheLudlows/Origence/issues/91).

S1's 300 questions are specific versions of historical engineering retrieval experiments. Competitor measurements, complete intelligent agent evaluations, and enterprise pilots are still underway. Current results can be found in [STATUS](STATUS.en.md), [VALIDATION](VALIDATION.en.md), and [evals](../evals/README.en.md).

Reading order: [metrics](#3-six-core-metrics) → [governance gates](#4-hard-barriers-for-enterprise-scenarios) → [fair comparisons](#6-three-fair-comparison-trails) → [statistics](#8-experimental-control-and-statistics) → [report template](#10-report-template-for-the-first-round-of-work).

## 1. What to evaluate

First verify authorization, publication, withdrawal, and version correctness, then compare evidence retrieval, memory update, Agent task success, and quality/delay/cost. Do not use QPS, feature count, or a single score to replace product effectiveness; unpublished, unauthorized, or invalid evidence should not be used to improve performance in the context.

## 2. Comparison objects and identities

| Object | Known state | Handling method |
| --- | --- | --- |
| Origence | Local base and engineering retrieval experiments delivered, scope see STATUS | Fixed commit, model, and dataset; design objectives not considered as operational facts |
| User-mentioned "rds contextdb" | Specific project yet to be confirmed with full name or link | Keep as a test item, do not assume it is a cloud database or a project with the same name |
| Memoryless Agent | Baseline | Fixed model and tools, disable external context |
| Simple hybrid retrieval | Engineering baseline | Keywords + vectors + basic fusion on the same authorization corpus, record actual backend |
| Correct evidence input | Diagnostic reference | Provide necessary gold-standard evidence for the same answering model, not as a deployable competitor |

Open-source and commercial configurations are listed separately. The OSS cost cannot be combined with commercial features. For products without certain enterprise capabilities, record as "not supported natively" or "not verified yet". If an adaptation layer is added to complement these capabilities, it forms an independent "product + adaptation layer" configuration, and the adaptation cost is calculated accordingly.

## 3. Six Core Metrics

| Dimension | Main Metrics | Measurement Object |
| --- | --- | --- |
| Retrieval Quality | Evidence Recall@k, nDCG@k, Evidence Coverage@B, Null Hit/False Hit Rate | What necessary evidence was found and is it within the token budget? |
| Memory Quality | Fact Extraction Precision/Recall, Current Fact Accuracy, Historical Version Accuracy, Conflict Handling Accuracy | Newly added, updated, explicit historical versions, and concurrent conflicts. Record separately for business-effective time queries |
| Agent Performance | Task Success Rate, Fact Error Rate, Citation Support Rate, Answerless Handling, Manual Redoing Rate | Performance in completing real business tasks |
| Enterprise Accuracy | Tenant/Workspace Leakage, Unpublished Product Leakage, Retracted Residuals, Version Confusion, Duplicate Releases | Whether data is used in the correct state and scope |
| Performance Reliability | Query p50/p95/p99, Throughput, Queueing, Release Delay, Failure Rate, Recovery Time | Real API and backend links |
| Total Cost | Import/Incremental/Query Costs, Infrastructure, Review Time, Integration and Maintenance Costs | Total cost per thousand queries and per successful task |

### 3.1 Retrieval Metrics

Evidence Recall@k is "Number of relevant evidence units returned / Number of necessary evidence units marked". It is aggregated after evaluating the problem. Evidence units use stable source document versions, page numbers/paragraphs, or event IDs. Do not use internal system chunk IDs directly, otherwise, different chunking schemes cannot be compared.

Evidence Coverage@B measures the necessary evidence coverage under a unified token budget B, and additionally reports whether the topics that require multiple pieces of evidence are fully covered. Figure paths, summaries, and text fragments must be mapped to verifiable sources; if there is no source mapping, the task can still be measured, but this metric cannot be measured, and it is not possible to determine the correctness of the source based solely on text similarity in the summary.

Questions without answers do not participate in the Recall denominator; separately measure false recalls, incorrect responses, and correct refusals. Measure unnecessary refusals for answerable questions to prevent the system from achieving a low error rate by refusing all answers.

### 3.2 Memory and Response Metrics

The precision rate of fact extraction focuses on whether the extracted content is supported by the original text, while the recall rate focuses on whether facts that conform to the predetermined memory strategy are extracted. The annotation strategy and the definition of "important facts" are fixed before testing, and not all original sentences should be considered as memories to be extracted.

The current fact accuracy is judged based on the published current version, and explicit historical versions are verified through get. Proposed, guessed, and confirmed facts are marked separately, and misjudgments by the extraction model are counted as a failure in quality. Conflicting questions are checked against `expected_version` and task results. Tasks that are not supported are recorded separately based on business valid time (`as_of`, `valid_from`, `valid_to`).

The citation support rate checks whether the evidence supports the substantive claims in the response item by item; citation coverage checks which claims that require evidence lack citations. If the answer is correct but the cited version is incorrect, both metrics are recorded separately, and it cannot be judged as fully passed.

Task success is prioritized based on the ability to execute business assertions or blind review; pure text tasks can be judged by an LLM with a fixed rubric, and disputed items and sampling results are manually reviewed. Judges do not accept the system brand, and versions, prompts, and scoring logs are all retained.

## 4. Hard Barriers for Enterprise Scenarios

The following are the acceptance requirements for this project, not a unified certification claimed for the industry. If the barrier fails, the quality score can still be reported for research purposes, but a high average score cannot offset production adaptation failures.

| Barrier | Scenario | Passing Conditions |
| --- | --- | --- |
| G1 Space Isolation | Two workspaces have projects with the same name and opposite configurations | No information leakage across spaces, references, documents, or tasks in the authorization test set |
| G2 Publish Visibility | External graphs/vectors have been written but SQLite has not yet confirmed the publication | Search/get/resolve does not return pending results; only readable after completed + published |
| G3 Submission Authorization Validity | Revoking keys, retracting sources, or canceling tasks during model execution | Late results cannot be reviewed through final permissions, sources, generation/run_token for publication |
| G4 Source Retraction | Retracting documents that support a certain memory | After the defined submission/authorization boundaries, new readings do not return affected content |
| G5 Conflict Management | Two publishing tasks use the same expected_version | The first submitter publishes, the second submitter supersedes; no silent coverage or addition of duplicate versions |
| G6 Idempotent Publishing | Response loss, concurrent retries, Worker strong recovery from termination | No duplicate logical events or multiple business publications generated |
| G7 Traceability | Read the original version through the returned reference | The source exists, its version matches, and access is authorized |
| G8 Budget and Error Contract | Minimal budget, dependency failures, no answers | Does not exceed the declared budget; does not pretend to be a complete success |

Reporting "observed 0 leaks in N scenarios, M executions" should not be interpreted as a mathematical guarantee of zero risk. It needs to cover concurrency, caching, historical versions, and error information; it cannot only test one happy path.

## 5. Corporate Evaluation Set

The following 300 questions are suggested components of a complete corporate quality set, with additional governance/fault sets; it cannot be assumed that coverage is achieved just because S1 also has 300 questions, nor is statistical sufficiency guaranteed.

| Type | Initial Number of Questions | Examples |
| --- | --- | --- |
| Precise Facts and Document Locating | 60 | A certain project leader, publishing rules, original document page numbers |
| Chinese Synonyms and Domain Terms | 40 | Abbreviations, Product Designations, Spoken Questions |
| Multi-Document/Event Associations | 40 | Basis for Certain Decisions, Scope of Influence, and Subsequent Tasks |
| Version Updates and Explicit Historical Reading | 40 | Compare Current Regulations with Specified Historical Versions |
| Automatic Publishing, Extraction, and Conflicts | 40 | Fact/Proposal Discrimination, Batch Extraction and Deduction, Concurrent Updates |
| No Answers, Negations, and Distractions | 40 | Approvals Not Present in Data, Negated Incorrect Statements |
| Real Team Tasks | 40 | Organize Publishing Inspection Lists, Handover Projects, Respond to Change Justifications |

Governance/Fault Testing Covers Collisions Data, Duplicate Events, Withdrawals, Deletions, and Worker Interruptions across Multiple Workspaces, Not Included in the 300-question Ordinary Q&A Calculation.

Split development, validation, and test sets by workspace, project, or complete incident history, for example 20%/20%/60%. Keep paraphrases about the same document in one set. For temporal reasoning tests, expose only events that were permitted to be seen before the query time. Once the dataset is large enough, expand the frozen test set. Determine the sample count from the target effect size and confidence interval width.

Each Use Case Preserves question, as_of, workspace, input_history, scheduled authorization/updates/withdrawals, gold_facts, necessary evidence, allowed outputs, prohibited outputs, and business assertions. Production Data Is First Prepared According to Corporate Authorization and Masking Strategies.

## 6. Three Fair Comparison Trails

### 6.1 Trail A: Controlled Retrieval

Fixed Authorized Corpus/Source ID, Answer Model/Tip Words, Final Token Budget, and Hardware Limits; Embedding Should Be Consistent, with Differences Made Explicit. Different Blocking/Indexing/Graph Construction Are Allowed, with Additional Fixed Block Ablations. Abstracts/Graphs Precalculated Are Counted as Import Costs. Only Systems That Return Final Answers Enter the End-to-End Trail, Not Pretending Generated Answers As Retrieval Evidence.

### 6.2 Trail B: Native Memory Lifecycle

Freeze the same event flow: Authorized facts → Proposals/Guesses → Updates → Concurrent Conflicts → Withdraw/Delays, gradually inspect extraction, publish visibility, current values, sources, and permissions. Retain misextraction, omission, failures, and time consumption, and cannot be filtered with gold before being stored. Origence writer/capture is published directly; other products are reported according to native strategies, with additional approval adaptation layers listed separately and with costs.

### 6.3 Track C: Complete Agent Tasks

Fixed Agent runtime, basic model, tool permissions, input, iteration count, and costs, only replacing context services; compared with memoryless and simple hybrid retrieval, tasks show success, errors, rework, time consumption, model costs, and review times. The best performance of the recommended configuration is run separately, not mixed with controlled tracks.

## 7. Reuse of open-source evaluation resources

| Resource | Content Suitable for Reuse | Boundaries to Current Needs |
| --- | --- | --- |
| [LongMemEval](https://github.com/xiaowu0162/LongMemEval) | Information extraction, cross-conversation, knowledge update, time reasoning, and evaluation with no answers; official scoring script | Focused on long-term interactive memory, does not replace enterprise permissions, publish and withdraw tests |
| [LongMemEval-V2](https://github.com/xiaowu0162/LongMemEval-V2) | Enterprise and web agent histories, state changes, and workflow knowledge, with an emphasis on accuracy and query latency | Contains multimodal trajectories. Label a text-only subset explicitly and report its results separately from full official results |
| [LoCoMo](https://github.com/snap-research/locomo) | Long conversations and cross-conversation memory experiments | Different from the system's workspace fact governance, serves as a supplement |
| [BEIR](https://github.com/beir-cellar/beir) | General retrieval evaluation tools and data formats | Static retrieval measurements do not represent memory updates/removals |
| [Ragas](https://docs.ragas.io/en/stable/concepts/metrics/available_metrics/) | Evaluation metrics such as context precision/recall, faithfulness, etc. | LLM scoring requires fixed versions and calibration; enterprise permissions use deterministic assertions |

Prioritize using existing scoring tools and adaptation interfaces for these projects. Only add the Origence adapter, enterprise event script, source mapping, and permission assertion. Statistical, data format, and basic scoring do not need to be implemented repeatedly.

When the public memory questions can be used directly as input facts, use the authorized corpus track; the lifecycle track uses frozen authorization, update, and revocation scripts. Do not disable submission permissions and source review to increase scores.

## 8. Experimental control and statistics

Save commit/tag, dependency locks, deployment topology, hardware, data hash, model/Embedding/judge version, prompt, random seed, retrieval configuration, budget, and timeout for each run. Provide the same tuning frequency or cost limit for each system, only tuning parameters on the development/verification set.

If the type labels of public questions are used for routing, all systems should receive the same information. During evaluation of actual deployment, only the question text can be provided, and the router determines the type and includes it in its cost.

Measure cold start and hot cache separately. Record costs for storage, query, response, and scoring separately; scoring costs are not included in the cost of a single online query, but are included in the evaluation budget. Mark parts of internal model calls that cannot be observed as unknown, and do not treat them as zero cost.

Repeat the final test across several inference runs to observe model variability. Rerunning a question does not increase the independent question count. Use paired quality comparisons and cluster bootstrap intervals over independent units such as workspaces or sessions, so that strongly correlated questions from the same history are not treated as independent samples. Report subgroup results to make failures on version updates and conflicts visible.

Errors, timeouts, and empty results are included in the denominator of the success rate for all assigned tasks; quality evaluation can supplement the successful request subset, but must be listed separately. p95/p99 is displayed together with load and timeout rate, and the appearance of lower latency cannot be achieved by discarding timeout requests.

## 9. Cost and selection rules

```text
周期总成本 = 文档初次处理 + 增量处理 + 查询/回答模型 + 基础设施
           + 人工复核/返工 + 接入与维护成本

每个成功任务成本 = 同一周期纳入的总成本 / 成功任务数
```

When labor costs cannot be monetized, report the number of facts extracted per minute and the average rework time, without forcibly combining them with API fees into a single precise amount. Construction and migration costs should be listed separately, with an explanation of the amortization period. Report the corpus size and daily increment per thousand queries; otherwise, the re-computation scheme and the light-write scheme are not comparable.

Suggested decision-making order:

1. Whether the native or adapted solution passes the enterprise correctness threshold.
2. Whether it leads to a reproducible increase in task success compared to memoryless and simple retrieval.
3. Whether it falls within a reasonable range of quality, latency, and cost under the same budget.
4. Whether missing capabilities can be compensated for through minor adaptations, or whether the core lifecycle needs to be redone.
5. If an open-source system is more suitable in terms of effectiveness and total cost after meeting the threshold, prioritize reusing that system or its components.

Do not presuppose that the in-house system must win or use a universal pass threshold without business approval, such as requiring 90% Recall in all cases. Freeze thresholds after evaluation on the validation set and a business risk review, before testing.

## 10. Report template for the first round of work

One row for each actual test configuration, indicating unsupplied/unverified/unobserved respectively, with no score or cost filled in if not applicable.

| Configuration and version | Governance threshold | Evidence coverage within budget | Task success rate and range | p95/timeout rate | Cost per thousand queries and import | Review time | Integration workload |
| --- | --- | --- | --- | --- | --- | --- | --- |
| Fixed system/model/data/code | Number of passes and failures | Based on declared budget | Total allocated tasks as denominator | Cold and hot loads separated | Unknown for unobserved items | Minutes | Actual records |

Execute in the order of “confirmation object/version → enterprise samples and gold standards → adapter and baseline → frozen configuration → actual measurement”. This document does not record current scores; historical results from S1 are moved from STATUS, and cannot be filled in the enterprise/competitive comparison table that has not yet been run.
