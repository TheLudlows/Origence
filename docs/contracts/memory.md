# 记忆身份与语义

**简体中文** · [English](memory.en.md)

本文维护显式记忆的领域规则；受限模型提取见[受控提取](semantic-capture.md)，
接口见 [API](../API.md)，物理身份占位见[存储不变量](storage.md)。
来源有效、语法有效和版本匹配均不等于事实为真。

## 身份与更新权

| 字段 | 规则 |
| --- | --- |
| scope | tenant/workspace 来自认证；授权 principal 与被描述对象不同 |
| subject | kind + stable_id；名字、别名和相似度不作为唯一身份 |
| predicate | workspace 管理的受控属性；语法有效不等于已登记 |
| context | 显式适用条件，规范编码并精确匹配，不合并生产/测试等不同条件 |
| asset_id | 稳定记忆条目标识，身份绑定不可变，删除后占位仍保留 |
| source/evidence | 原始事件、精确 quote/locator、parser 与来源版本 |
| expected_version | 并发写条件，不证明真值、适用期或来源权威性 |

[MemoryIdentity](../../src/memory_identity.rs) 以固定 JSON tuple 编码版本、scope、subject、
predicate 与排序后的 context，再 SHA-256，前缀为 `memory-identity:v1:`。
不折叠大小写或 Unicode，不把正文、principal 或版本条件加入身份。

语义近似不授予覆盖权、别名绑定或跨主体合并权。`fact_key` 保留独立入口，
不能静默改写已绑定的结构化记忆条目。

## 属性目录

管理员在 workspace 注册 predicate、single/multi 和 required_context。定义不可变；
相同定义可重放，不同定义拒绝。已有身份（含未发布、已删除）的 predicate 不允许
被新目录接管。目录属性通过语义入口写入，旧 identified/capture 入口不能绕过目录规则。
当前新库包含完整目录 schema；缺表或不兼容库拒绝打开，不提供旧库升级流程。

## 显式写入

输入包含原始 content、可选 identity、operation、值和逐字 quote/UTF-8 半开区间。
精确字符串等价不做同义词或 Unicode 折叠；原文校验只证明溯源，不证明分类正确。

- `assert`：无当前值则建立事实；同值幂等；与单值当前事实不同则保留冲突证据。
- `update`：仅单值，带当前 expected_version，追加版本。
- `supplement`：仅多值，集合并集；重复值不制造版本，每个成员保留原始来源与区间。
- `negate/propose/ambiguous` 或缺少 identity：独立未归一化证据，不改当前事实。
- 单值输入含多个不同值则保留 conflict；未知属性或缺少必需条件则拒绝。

写入必须给 expected_version，0 表示尚未发布。确定事实在受理与 Worker 发布时双重
校验；版本竞争导致 superseded。未归一化证据不更新建议身份，其输入版本不成为
建议身份的更新条件。来源、幂等响应、记忆条目和作业同事务登记，重试复用持久计划。

## 发布、检索与撤回

确定事实复用已绑定身份的记忆条目；不确定证据使用无 fact_key/身份的独立 记忆条目。
chunk 保留状态、原始成员来源及字节范围，search/resolve 返回 normalization_status。
精确身份过滤在 top-k 前限定当前事实，返回前复核来源、墓碑、版本和权限。

多值采用整版本发布：本次操作来源或任一早期成员来源撤回，整个快照退出读取，
不自动回退历史。保留成员不能改记为新来源。当前发布来源仍有效时，后续显式补充
可重建仍有支持的集合；当前发布来源本身失效时须重新提供事实，不从历史自动补回。
通用 restore 不能绕过语义写入规则，历史正文与 locator 不改写。

Learning 的条件与完整支持链见[学习契约](learning.zh-CN.md)；召回预算、确定性冲突与
会话引用限制见 [EvidenceBundle](evidence-bundle.zh-CN.md)。工程测试结果统一见
[VALIDATION](../VALIDATION.md)，不在本契约维护阶段进度或历史数量。
