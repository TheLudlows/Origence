# 试点执行、恢复与清理手册

## 开跑前

使用[冻结表](freeze.template.json)登记整合 SHA、服务和客户端二进制哈希、OS、实际模型修订及配置、SQLite 和 AnySeek 版本、Blob 清单、时间与费用预算。单独初始化测试资源，记录逻辑 key 别名。数据负责人签认合成或脱敏范围；DBA 和制度负责人在开跑前签认两场景的 rubric、阈值和停止条件。缺少必需配置或签认时记 BLOCKED。

## 题集与评分表

在仓库根目录运行。以下种子均公开，仅用于演示，输出目录须为新路径。

```sh
python evals/pilot/pilot.py generate --seed public-development-1 --output pilot-prepare-new
python evals/pilot/pilot.py generate --split held-out --seed public-example-only --exclude-tasks pilot-prepare-new/tasks.json --output pilot-example-new
python evals/pilot/pilot.py score --prepare pilot-prepare-new/observations.json
```

每个场景 8 题，各有无 Skill 基线和有 Skill 复做两行。报销覆盖正常、有效超限、重复和缺凭证，各两例。生成器按实际输入内容排重，再重排题号。独立保管人应为每批已暴露的题目重复传入 `--exclude-tasks`，清单会登记这些文件的 SHA-256。若可用输入不足，生成器报错。
正式未见题须在开发和 Skill 冻结后使用独立保密种子生成，仅向操作者交付 `tasks.json`。保管人封存种子、oracle 和摘要；若答案已泄露，登记污染并换批。按题号随机安排两臂顺序，分别使用干净会话和相同客户端、模型及工具预算，记录顺序、超时与人工干预。
填写同目录的 v2 `observations.json`，保留分组、整表题集摘要和每行输入摘要。观察行可随核验更新。
评分器按原始字节核对任务文件、清单和观察表。`--prepare` 接收 prepare，输出 `PREPARATION_ONLY`。
公开交接演练记录单独保存。未绑定的历史观察表只供查阅。

独立保管人应将正式留出批次的原始 `tasks.json` SHA-256 登记到可信冻结记录的 `data.tasks_sha256`。
留出评分需独立指定该文件，校验通过时标记 `FROZEN_BATCH`：

```sh
python evals/pilot/pilot.py score "$held_out_observations" --freeze "$trusted_freeze"
```

模板的空哈希和仅自洽的其他批次会被拒绝。`FROZEN_BATCH` 仅表示题集字节与所传冻结记录的摘要匹配，不证明冻结已获批准。保管人仍需确认冻结文件来源及签认。
有已核验项或填报成功的观察行须提供非空白的评审人，以及逐项非空白的证据引用。评分器不打开引用或核验身份、内容和签认；输出将组指标标为 `OBSERVATION_SHEET_ONLY`，即使填表满分也不是试点实绩。
预登记题全部保留在离线分母中。正式业务成功率和核验覆盖率须独立核实，按 [PRD 口径](PRD-MAPPING.md#计量差异必须披露) 单列。

## 执行矩阵

| 检查 | 记录与判定 |
| --- | --- |
| P1 接入 | 在同一整合提交和新库上验证双端 MCP search/get 和归档往返，由独立验收人核对冻结表。 |
| P2 数据库排障 | Claude Code 首做、Codex 复做未见题。DBA 按快照核验阻塞链、连接池、超时分类和只读建议。 |
| P3 报销复核 | Codex 首做、Claude Code 复做未见题。制度负责人核对金额、重复票据、缺材料和适用制度版本。只给复核意见。 |
| P4 更正与撤回 | 更正到 v2 后按精确引用读 v1；候选编辑后重新审阅。撤回来源后检查 get/search/context/package、关联 Run 和准备重放。分别核对新会话、缓存及受管在线重载。 |
| P5 隔离 | 同 key 跨端归档；异 key（含管理员）读私有会话、候选和 Run 均应拒绝。验证跨 tenant/workspace 同名隔离、凭据轮换及撤权后的读取和重放。 |
| P6 追溯 | 逐题关联输入、精确准备 bundle、实际交付、原生加载、工具、输出及独立判定。按场景、客户端和基线/复用分组，已执行但缺失观察记 UNKNOWN，未执行记 NOT_RUN。[独立签认原件与 Run 核验关联](P6-EVIDENCE.md) |
| P7 恢复 | 依下节演练停写、同版本恢复与清理。DBA、制度负责人和独立验收人分别签认。 |

隔离、撤回、版本检查须零违规。Windows/Linux 分开登记；小样本报告计数和描述性差值。业务结果、证据覆盖和 Skill 加载分别核验。

## 真实闭环与精确加载证据

保留首做的精确资产版本或不可变 Run 前缀、候选修订、脱敏依据、审阅人和批准的 packet digest。编辑后用旧批准尝试发布应被拒绝，再按新修订审阅并明确发布。
复做端在新项目和新会话中在线导出获批包。登记 tenant、workspace、Skill UUID、版本、包摘要、各文件哈希、原生会话、Run、时间。分别记录服务端授权获取、客户端原生加载事件、可观察的模型输入与实际操作。撤回后分别记录新下载、本地字节、会话上下文和受管在线重载；无法观察记 UNKNOWN，未执行记 NOT_RUN。
证据存入受限目录，索引包括相对路径、SHA-256、字节数、UTC 时间、采集人、证据类型、PRD 条款、执行项和题号。保留原始来源及定位（含 PDF 页码和页内 UTF-8 范围）、工具请求回执、输出工件和独立判定。公开附件先脱敏，登记采集遗漏；记录主张与引用数量、返工、耗时和成本。

## 同版本 SQLite / AnySeek / Blob 恢复演练

整合主干上的 Linux 合成环境已补充工程恢复检查，范围及缺口见 [VALIDATION](VALIDATION.md#已整合主干和非客户端证据2026-11-18)。
正式 pilot.P7 仍待按本节完整复查并独立签认。

1. 仅在专用新库完成样本导入、精确 v1→v2 更正、来源撤回、私有 Run、Skill 发布与 pending/failed job；保存可预期 get/search/context/package、Run prefix、权限拒绝结果。
2. 记录同一整合 SHA、二进制哈希、schema/分析/embedding 配置和 AnySeek 版本。协调停止客户端、所有写入方、宿主与 worker，确认进程退出；等待/记录未完成请求的幂等身份。没有在线跨库一致快照协议，无法停写就 BLOCKED。
3. 在同一个停写窗口保存**整个** OC_DATA_DIR（context.db、存在的 WAL/SHM、绑定配置、blobs 及关联文件），逐文件哈希；按冻结 AnySeek 版本的正式备份流程保存所有匹配 tenant/workspace collection 的正文、摘要、全文/向量/图索引与配置。由 DBA 确认具体命令和 snapshot ID；本手册不臆造 AnySeek 备份接口。
4. 把两侧备份绑定同一 checkpoint ID 和完成时间；缺集合/缺 Blob 或不同时点为无效备份。原现场保留，不清空 AnySeek，不从来源重解析重建历史证据。
5. 恢复到新的隔离目录与 AnySeek 实例/命名空间，保留原 scope/collection 对应与绑定；禁止连回原实例。用完全相同二进制/配置先执行该版本支持的离线检查，再启动 ready 检查；不迁移，不用新版本打开。
6. 对比恢复前后精确 v1/v2 正文、来源原始字节与 locator/hash、Skill digest、Run 前缀、当前授权与撤回拒绝；get/search/context/准备重放/关联 Run 逐一验证。重试原幂等请求不重复发布，合法 pending/failed 状态按原计划恢复；记录失败与耗时，不以启动成功替代。
7. 在恢复副本做负例：缺失 Blob、缺失/冲突 AnySeek 正文、配置不匹配、结构不兼容应明确失败且不能静默补空/转换；每次从完整备份创建独立副本，禁止破坏原备份。权限和撤回仍不得绕过。
8. DBA 签认 checkpoint 一致性、完整性和恢复结果，制度负责人核验恢复后的业务版本结论，独立人签认 pilot.P7（额外恢复门槛，不是 prd-v0.3.A7）。未实际执行则 NOT_RUN。Linux/Windows 分开记结果；本路 Python 不构成恢复演练。

## 清理和保留

实验前登记所有临时目录、workspace、collection、凭据别名、客户端项目/缓存、模型会话、备份、证据保留期与负责人。
先封存必要脱敏证据及签认，再撤销测试 key、停服务/客户端；按资源清单由负责人核对精确测试目标后删除临时副本和隔离集合。
不执行宽泛 rm/glob，不删除共享目录/历史库/未登记集合。备份独立记录保留与到期销毁。
清除受管 Skill 和显式导出副本，关闭旧上下文会话；无法控制的客户端缓存/供应商留存列 remaining，不声称物理擦除。
核对资源清单的 cleaned/retained/blocked 状态与复核人；保留数据以受限权限管理，不上传公共仓库。
