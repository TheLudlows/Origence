# 五路 P0 独立试点验收（准备材料，不是通过报告）

2026-11-18 已核对整合主干 `48cfa7c643b8c077ad33c55981def51979afde05`，包含四个实施 PR
#93–#96。[主干 CI 44217031418](https://github.com/TheLudlows/Origence/actions/runs/44217031418)
的 Windows、Node UI、fast-check 和 Linux native 四项均通过。
本目录原准备基线为 `293e2bf6af3b180d43c8050731047b552ac07519`。
[#104](https://github.com/TheLudlows/Origence/pull/104) 合入后，真实客户端使用主干 `e024b78` 和公开合成报销样例完成了原生 Skill 加载预检。复跑入口及逐端证据见[预检说明](PREFLIGHT.md)。
[#91](https://github.com/TheLudlows/Origence/issues/91) 正式试点 **NOT_RUN**。未见题和评分条件待冻结，独立 DBA、
制度负责人及验收人签认待办。本路执行清单使用 `pilot.P1–P7`，PRD v0.3 条款使用
`prd-v0.3.A1–A7`，两者不是同号对应。
原文已核对；逐条映射及待补证据见 [PRD-MAPPING](PRD-MAPPING.md)。映射待联合审阅冻结，不能当成通过报告或产品扩展。

依据：[架构](../../docs/design/agent-data-foundation.md)、[Skill](../../docs/contracts/skills.md)、
[Run](../../docs/contracts/runs.md)、[存储](../../docs/contracts/storage.md)、
[客户端](../../docs/CLIENTS.md)、[运维](../../docs/OPERATIONS.md)、
[评估标准](../../docs/Origence_效果评估与对比标准.md)。本目录不改变这些合同。

## pilot.P1–P7 执行矩阵

| ID | 验收动作与必要证据 | 判定 / 负责人 |
| --- | --- | --- |
| pilot.P1 冻结与接入 | 四路功能在同一整合 SHA；全新 SQLite/Blob/AnySeek 命名空间；两客户端版本、实际模型修订、配置摘要；真实 MCP search/get 与归档往返 | 缺任一版本或只有配置注册截图为 BLOCKED；独立负责人核对冻结表 |
| pilot.P2 研发闭环 | Claude Code 处理合成数据库连接等待，形成经 DBA 审核的诊断步骤；候选依据为精确资产版本/Run 前缀，编辑后重新审阅并明确发布；Codex 在新会话复做未见题 | DBA 核验阻塞链/连接池/超时分类及安全建议，真实加载证据齐全；不准执行 kill、DDL 或生产修改 |
| pilot.P3 业务闭环 | Codex 按合成制度复核报销，生成有依据的 Skill；制度负责人审阅后发布；Claude Code 新会话复做未见题 | 独立核对金额、重复票据、缺材料和制度版本；只生成复核意见，不付款、不提交真实审批 |
| pilot.P4 更正和撤回 | v1 后发更正 v2，显式历史请求仍指 v1；旧批准不能发布编辑后的候选；撤回支持来源后重试 get/search/context/package/关联 Run/原准备重放；客户端缓存与新会话分别观察 | 精确引用不漂移；来源失效后服务端读取/分发 fail closed；受管在线重载单列为未验证要求，不由服务器拒绝新获取推断已满足。离线副本不能瞬时撤回，记录残留并清理，不能据此声称在线合同通过 |
| pilot.P5 隔离和权限 | 同逻辑 key 跨客户端归档；另一 key（含 workspace 管理员）不能读原会话/候选/Run；跨 tenant/workspace 同名冲突；轮换保持 owner、撤权阻止读和重放 | 未授权正文、引用、元数据泄露为失败；共享已批准 Skill 不授予私有原文或业务工具权限 |
| pilot.P6 追溯和收益 | 输入→确切准备 bundle→实际交付→原生加载→工具→输出→独立判定；按基线/复用、场景、客户端分别计量 | 服务端准备/客户端上报/原生加载/独立核验分别列证据；遗漏为 unknown，不能推断已使用；成功率和核验覆盖分开 |
| pilot.P7 同版本恢复与清理 | 按 RUNBOOK 停写，SQLite/WAL、Blob、AnySeek 同一停机检查点备份与恢复；验证精确 v1/v2、撤回、隔离、幂等与失败恢复 | DBA 签认完整恢复，制度负责人签认业务数据/结果；独立负责人最终签认；不可只恢复 SQLite 或使用新模型重算 |

所有隔离/撤回/版本硬门槛零违规；失败不得由质量均分抵消。试点量小只报告计数和描述性差值，不宣称统计提升。
业务通过阈值与最大时长/成本由 DBA、制度负责人在开跑前写入冻结记录并签认；未填为 BLOCKED。
Windows/Linux 行为核验按整合交付要求分别登记，不以本地 Python 校验替代。
Linux 隔离同版本恢复的 10 组工程检查和签认状态见
[验证记录](VALIDATION.md#已整合主干和非客户端证据2026-11-18)。

## 本地复跑（零网络、零模型、零产品写入）

从仓库根目录执行：

```sh
python3 -m unittest discover -s evals/pilot -p 'test_*.py' -v
python3 evals/pilot/pilot.py generate --seed public-development-1 --output /tmp/origence-pilot-development-new
python3 evals/pilot/pilot.py score /tmp/origence-pilot-development-new/observations.json
```

输出目录必须不存在；生成器只写合成题、独立答案和空观察表，不导入产品，不调用客户端或模型。
`score` 是登记完整性/分母检查，不认证证据真实性、不授予 PASS。
开发样例种子公开。真正未见题由独立负责人在开发与 Skill 冻结后，用另一个保密种子并指定 `--split held-out` 在受限目录生成；
只将 `tasks.json` 发给操作员，`oracle.json`、种子和 SHA256 承诺交独立保管人。
本仓库的生成规则公开，因此未见题只保证具体实例未提前给操作者，不保证分布保密。
操作员若见过答案/种子，标记 contaminated，整批重建；不得调参后继续沿用同一未见题集。

每个场景 8 题；任意种子生成的报销集均有正常、有效超限（按 v2 截顶且不同于 v1）、重复、缺凭证各 2 个不同实例。默认 `--split prepare`；各场景在各分组内按种子稳定重排后重新编号，避免固定题号泄露类别。开发集和未见集使用同一覆盖约束，答案仍只存于独立 oracle 文件。固定 2 个方向：研发 Claude Code→Codex，业务 Codex→Claude Code。
冻结后按题号随机分配无 Skill 基线/有 Skill 复做顺序，使用独立干净会话、相同模型/工具预算。
禁止把第一次答案交给第二次；记录顺序、失败、超时、人工干预，所有预登记题保留分母。
仅跨模型/客户端结果不能归因为 Skill；应补同客户端同模型的无 Skill 对照，分别报告，不混成一个提升值。

## 离线准备指标（不是 PRD 签收口径）

- 任务成功率：独立审阅通过题数 / 全部预登记题数；未跑、错误、超时、缺独立判定均不算成功。按组报告。
- 核验覆盖：已独立核验的必要检查数 / 冻结的必要检查总数；unknown/未采集计入分母且不给分。
- 业务成功、证据完整性、Skill 加载、隔离门槛分别记录；答案对但版本错不能判整体验收通过。
- 额外登记需引用主张总数、受支持主张数、缺引用数、返工次数、耗时与成本；零分母报告 null。

PRD 的业务成功率为独立通过数/已独立判定数，核验覆盖率为已独立判定数/到期应核验数，待定单列；三个到期任务通过/失败/待定时分别为 1/2、2/3。
上面的离线指标及 `pilot.py` 字段保持兼容，不得改名充当 PRD 指标；当前无 PRD 业务签收结果。

先用 [单独未评分预检](PREFLIGHT.md) 核对证据能力，预检不进入评分集或未见集。
模板见 `freeze.template.json`、`signoff.template.json`；证据和恢复清理见 `RUNBOOK.md`。
四路实施已汇集到上述主干。正式试点仍需确认联合映射、冻结配置及未见题，按原矩阵执行，
并由 DBA、制度负责人和独立验收人签认。
