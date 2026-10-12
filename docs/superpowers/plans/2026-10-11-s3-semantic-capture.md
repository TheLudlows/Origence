# S3 首个切片：对话候选提取接入 S2

范围：用户已确认串行完成候选提取、S2 写入接入、端到端验证；只面向新库，不开发升级。完整 SessionTurn → Feedback → Improve → Learning 仍为后续工作，本切片不宣称反馈学习或真实 Agent 效果。

## 契约

- 新增库 API 与 POST /v1/captures/semantic。调用方提交不可变原文 content 和最多 20 个允许的完整 identity/expected_version 目标；目标属性必须来自当前 workspace 目录，条件完整。targets 可以为空，此时只能产出未归一化证据。
- 模型仅返回目标索引（或 null）、operation、带原文 UTF-8 字节区间的 statements。不能返回身份、租户、批准或发布字段；未知字段/越界/重复目标/伪造引用导致整批失败。总计最多 20 个候选、32 个 statement；原文最多 64 KiB。
- 提取是模型判断，不证明事实真假。目标稳定 ID 和适用条件由调用方给出，不自动解析别名。对话中提议、否定、歧义归入未归一化；模型请求单值 update 只有目标 allow_update=true 时才可覆盖，否则降为 ambiguous。
- 受理时检查全部 targets 的 expected_version，任一过期即拒绝且不创建来源或任务。受理后，确定性候选在 S2 路由时检查所用目标版本；计划保存后只复核实际路由资产，包含 duplicate 结果的版本与当前可见性，不持续锁定未使用的 targets 或未归一化证据中的 suggested_identity。确定性 assert 路由为 conflict 前仍检查目标版本，之后独立发布证据；不确定候选不更新建议身份。实际确定事实的版本竞争使整批 superseded。
- 使用同一个 S2 路由函数，实现重复不增版本、显式单值更新、多值并集和冲突保留。每个候选独立记录路由结果；所有非重复结果在一个关系事务中发布，任何错误都不部分发布。
- 在原有串行 durable worker 内提取；模型/分词工作在事务外。路由计划在 embedding 前持久化，重试复用已保存的候选/目标/来源，不重新分类。进程在模型返回但计划提交前退出可能重新请求模型；不保证供应商调用恰好一次。
- 当前来源与集合成员来源在每次恢复和最终提交前复核，沿用 S2 整快照撤回。locator 保留原始引用和来源；新增 extraction 元数据 semantic-capture:v1 与模型名称，不覆盖已有成员来源。
- 不增加 worker、自动调度、重试循环、硬取消保证或数据库升级。失败、取消、权限撤销、重启、搜索与撤回沿用现有治理。

## 串行交付

1. 纯库提取契约、严格校验与模型适配。
2. 共享 S2 路由、持久计划和 HTTP/Service/Worker 接入。
3. 确定性模型桩端到端回归、Rust 1.98.0 检查、API/运行/状态/验证文档。模型桩验证工程契约，不计为独立语义质量验收。

## 交付结果

三个步骤已完成：纯库测试 3 项、端到端 7 项；默认 lib 38/local 127、无默认特性 lib 36/local 89 全通过，格式及两套严格 Clippy 通过。精确命令、夹具修正和最终源码哈希见 [VALIDATION](../../VALIDATION.md) 与 [证据](../../evidence/2026-10-11-s3-semantic-capture-validation.json)。实现经 [PR #37](https://github.com/TheLudlows/Origence/pull/37) 合入 main `daf25b1f`；head `a0ec23d1` 的 [PR CI run 38150659736](https://github.com/TheLudlows/Origence/actions/runs/38150659736) 成功，fast-check、Linux 通过，Windows/macOS/container 按 main-only 策略跳过。这些是该实现的历史工程验证，不替代后续提交 CI，也不代表真实模型语义质量验收；当前 main 状态见 [STATUS](../../STATUS.md#当前证据与结论)。
