# M3 跨库账本与恢复契约

修订：2026-09-28。底层账本已在 M3 实现，应用编排在 [M5](2026-09-28-local-host-delivery.md) 接入。本文保留设计决策与验收入口，删除重复的整文件代码草稿；当前实现以 `src/storage/ledger.rs`、`sqlite.rs`、`sqlite/app.rs`、`worker.rs` 为准。历史草稿可从 Git 查阅。

## 权威数据与身份

SQLite 维护三组记录，向量/图自身不能授权读取：

| 记录 | 身份与用途 |
| --- | --- |
| `oc_artifact_owners` | scope + source/version + artifact_type/artifact_id；记录共享产物的来源，chunk/summary 还关联 chunk_id |
| `oc_index_entries` | scope + artifact + field + model_id + generation；记录维度和 pending/ready/removed |
| `oc_artifact_ledger` | scope + source/version + artifact + surface + generation；记录 pending/committed/retry_wait/orphan 及重试信息 |

`LedgerKey` 明确 tenant/workspace、来源版本、产物、存储面和 generation。幂等键由这些字段确定；不得只按 artifact ID 覆盖不同来源版本或执行代次。向量身份完整保留这些维度，图身份按规范化名称/端点/谓词确定并由多个 SQLite owner 共同拥有。

## 发布顺序

1. 短读事务确认创建者权限、来源、资产、任务 generation/run_token 和预期版本。
2. 事务外解析与模型调用；形成带确定性 chunk ID 的发布计划。
3. 新 IMMEDIATE 事务再次检查，保存计划并登记 pending；提交后才写 LanceDB/Kuzu。
4. 原生写完成后，新 IMMEDIATE 事务重新核验全部门槛，写版本、chunk、摘要、owner、ready 索引和 committed 账本，与任务完成原子提交。

底层 `confirm_committed` 核验账本键及来源有效性；任务取消、当前 run_token、权限和预期版本由 Worker 在同一最终事务中检查。不得将底层账本方法当作完整任务发布授权。

## 失败与删除

- 作业层持久化 attempt/next_retry_at，存储错误最多尝试 5 次；模型或输入错误需显式重试。显式 retry 提升 generation，旧计划不被复用；同 generation 的启动恢复可以复用已保存计划。
- 账本提供 retry_wait/orphan 和重试接口；当前宿主以作业重试驱动整份计划。失败后或启动时清除没有有效 owner 的外部产物，再重放计划，不把 pending 当成可见结果。
- 独占启动恢复 processing 并提升 run_token；旧执行不可提交。取消和删除同样阻断迟到结果。
- 删除先在 SQLite 墓碑/撤回并登记 cleanup，读取立即检查这些状态。清理移除失效 owner，向量按精确产物 ID 删除，图按剩余 owner 集合清孤儿，摘要先于 chunk 删除。
- 有其他有效来源的共享图对象保留；原始事件、版本、候选、文件及审计暂不物理删除。

## 验收入口

| 测试模块 | 范围 |
| --- | --- |
| `sqlite_ledger` / `storage_recovery` | 状态机、来源撤回、重试预算、对账与迟到确认 |
| `sqlite_owner` | owner 登记、来源分离和共享归属 |
| `local_ledger` | 真实 SQLite/LanceDB/Kuzu 的幂等外部写组合 |
| `local_app` | 保存计划后的外部图写边界重放、真实宿主强退、取消/撤销/删除、共享来源和最终孤儿清理 |

统一执行 `cargo test --locked -j 1 --test local`。具体结果见 [VALIDATION](../../VALIDATION.md)。进程强退和边界注入不等同于断电测试，也不证明跨库在线备份一致性。
