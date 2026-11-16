# Origence · 记忆身份与上下文规则

更新：2026-11-16。本文负责领域规则；进度见 [STATUS](../../STATUS.md)，
接口见 [API](../../API.md)。包含记忆、知识、Skill 等的统一上下文目标规划见[数据底座架构](2026-11-16-agent-data-foundation-architecture.md)，
本文的显式身份、条件和现有语义合同继续有效。

## 身份与更新权

| 字段 | 规则 |
| --- | --- |
| scope | tenant/workspace 来自认证；授权 principal 与被描述对象不同 |
| subject | kind + stable_id；名字、别名和相似度不作为唯一身份 |
| predicate | workspace 管理的受控属性；语法有效不等于已登记属性 |
| context | 显式适用条件，规范编码并精确匹配，生产/测试等条件不合并 |
| asset_id | 资产稳定标识；身份绑定不可变，删除后身份占位仍保留 |
| source/evidence | 原始事件、版本、精确 quote/locator 与 parser 标记 |
| expected_version | 并发写条件，不证明真值、适用期或来源权威性 |

[MemoryIdentity](../../../src/memory_identity.rs) 以固定 JSON tuple 编码版本、scope、subject、
predicate、排序后的 context，再 SHA-256，前缀 `memory-identity:v1:`。
不折叠大小写或 Unicode，不把正文、principal 和版本条件加入身份。
Turso 将身份字段并入 `oc_assets`；物理规则见[身份布局](../../contracts/identity-asset-layout.md)。
当前没有离线身份表安装、旧身份自动归并或旧库转换流程。

## S2 与受控提取

- 重复事实不增加业务版本；显式单值更新追加版本，多值 supplement 保留成员原始来源。
- 提议、否定、冲突与歧义保留为未归一化证据，不覆盖确定事实。
- 模型只在调用方明确指定的主体/属性/条件内提案，并提供精确原文 quote；服务器校验区间与内容。
- 更新单值仍需调用方明确授权和版本检查；语义近似不授权更新、别名绑定或跨主体合并。
- 多值快照的任一支持来源失效，整个快照退出读取，不自动回退历史；后续显式补充可重建。
- `fact_key` 仍是独立旧入口，不能静默解释为显式身份或改写已绑定资产。

精确 lookup/search 先在授权 scope 内按完整身份限定候选，不创建资产或锁定后续更新。
候选限制在 top-k 前执行，返回前复核当前版本、来源、墓碑和权限。
完整操作与提取规则分别见 [S2](../plans/2026-10-11-s2-memory-semantics.md)和
[semantic capture](../plans/2026-10-11-s3-semantic-capture.md)。

## 会话与 Learning

SessionTurn/幂等 Feedback、显式 Improve → Learning 已有工程实现；真实模型质量待验收。
调用方归档实际使用的证据，反馈不自动改变排序或赋予事实真值。
Learning 保留条件和整个支持轮次/历史证据/传递支持的有效性；失效先隐藏，再清理。
现有 Learning 不生成可能丢失条件的 summary/graph，不递归触发 Improve，也没有 Guidance 生命周期。

## S4 上下文

memory、knowledge、Learning 分路召回并独立预算；session 默认关闭且仅 writer 可启用。
只按完整类型/来源/版本/位置合并重复证据，不按文本相似删除相反内容。
冲突检测限于相同完整身份、已登记单值属性的明确 confirmed/conflict 值，
不推断普通文本矛盾或自动选择“记忆优先/文档优先”。保留不确定状态、条件及有效引用。
预算按 UTF-8 字节计算，外部证据不升级为系统指令。

Session-turn 引用不能回填现有 chunk/entity/relation 的 used_evidence 合同。
这些预算、冲突和归档边界已实现；真实 Agent 收益与通用事实核验尚未验收。
精确行为见 [S4 合同](../plans/2026-10-12-s4-evidence-bundle.md)和 API。
