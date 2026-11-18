# #91 本地验证记录

## 公开双场景跨客户端复做（2026-11-18）

Windows 使用产品主干 `e024b78`、AnySeek `6a46fc7`、Claude Code 2.1.327（报告模型 `glm-5.3`）和 Codex 0.170.0（指定 `gpt-6.2-sol`）。每个方向使用独立的新库、workspace、首做 writer、复做 writer 和审阅凭据。首做端从公开题面提取结构化规则，测试程序校验规则后，以首做 Run 的不可变前缀为依据提交候选。测试审阅账户明确批准并发布。两份 Skill 均为 v1，报销规则对应题面制度 v2。[完整的最终运行记录](public-handoff.windows.json)

| 场景 | 首做 | 复做 |
| --- | --- | --- |
| 数据库连接等待 | Claude Code 在快照中发现阻塞会话 312，判定锁等待，建议只读检查阻塞会话 | Codex 加载获批规则后处理另一份快照：无阻塞会话，连接池 12/12。返回 `pool`，引用 `pool_active`，建议查看连接池指标 |
| 报销复核 | Codex 按 v2 对 120 TEST 的重复申请给出暂缓、金额 0，并提取决策表 | Claude Code 加载获批规则后复核另一笔 123 TEST 申请：有凭证、无重复，按 v2 建议 97 |

两端复做者读取首做 Run 和私有候选共四次，均返回 HTTP 404；各自获批包仍可在线导出。Codex 客户端的模型请求记录含完整包正文及复做 Run ID。Claude Code 原生会话记录含成功的 Skill 调用和正文，供应商侧请求体未采集。两端都返回仅存在于包内的随机校验串。复做 Run 的第一条事件登记加载版本与摘要，第二条登记输出摘要，两条均由测试程序上报并读回核对。

| 尝试 | 耗时 | 数据库 | 报销 |
| --- | --- | --- | --- |
| 1 | 120.22 秒 | 首做超时 | 串行执行，尚未启动 |
| 2 | 50.08 秒 | 首做输出未通过 JSON 格式检查 | 完成首做和复做 |
| 3 | 33.22 秒 | 首做决策表未通过校验，未发布 | 已加载，复做结果未匹配 |
| 4 | 40.91 秒 | 完成首做和复做 | 完成首做和复做 |

第二次开始两场景分别使用新库并行执行。后续明确了字段格式、枚举取值和整数单位，最终运行的业务预期与前三轮一致。四次报告累计 244.43 秒。19 项 Python 测试、脚本编译、Rust 1.98.0 格式检查和全目标 Clippy（警告按错误处理）通过。未改 Rust 或 SQL。[复跑方法](PREFLIGHT.md#复跑双场景跨客户端交接)

两道复做题及判定规则均公开，报销首做时从题面取得制度，数据库诊断只使用合成快照。两条链路没有执行生产操作、正式未见题、无 Skill 对照或独立业务判定。两场景按正式分母登记仍为 `NOT_RUN`。

## 双客户端原生 Skill 加载（2026-11-18）

Windows，产品代码基线 [`e024b78`](https://github.com/TheLudlows/Origence/commit/e024b78eb2ecbf1aa1851783c89be8238ca819a1)，AnySeek `6a46fc7`。在全新 workspace 公开测试一笔 110 TEST 单位的合成报销。脚本创建 v1 来源并由不同的 writer、reviewer 测试账户提交及审批候选。两个新项目分别从服务端导出同一个 Skill v1，逐项核对 manifest 及文件字节。最终复跑耗时 8.83 秒。[机器可读记录](native-skill.windows.json)

| 客户端 | 本轮观察 |
| --- | --- |
| Codex CLI 0.170.0，指定 `gpt-6.2-sol` | 原生 `skills/list` 找到导出的精确文件，`turn/start` 显式选中它。一份发往模型的请求记录同时含 Run ID、Skill 名及完整正文。最终答案给出 100 和包内随机串。 |
| Claude Code 2.1.327，`haiku` 在本轮报告为 `glm-5.3` | 新项目的原生会话记录到与 Skill 名匹配的一次 `Skill` 调用、成功回执和注入正文。最终答案给出同样的金额和随机串。供应商端请求正文未采集。 |

两端分别建立 Run。脚本在核对原生证据后写入 `skill_loaded_reported`，按 Run 读回并核对 `reporter=test`、事件内容及 `reported` 归属，最后读取第一条事件的不可变前缀。JSON 保留审批摘要、精确版本、文件哈希、会话与 Run 标识、请求或输出流摘要。临时目录中的原始模型请求和客户端转录在结束时清理。

撤回唯一支持来源后，直接读取包返回 HTTP 404。两种客户端的新导出均返回资源不存在，新目录内零文件；原两个项目中的 manifest 和 SKILL.md 仍与发布版本一致。已打开的会话、已注入上下文的撤回和受管在线重载仍待验证。

早期两轮在 Claude Code 的 `--bare` 配置下未取得加载证据；第一轮撤回请求还因缺幂等键返回 422。补上幂等键，改用仅启用项目配置的正常启动模式和 `Skill` 工具后，两次观察到两端加载。模型的精确不可变修订仍未冻结。本轮只验证一个由测试程序编写的 Skill，双向首做、未见题复用、独立业务判定及正式评分均为 NOT_RUN。

Rust 1.98.0 的 `cargo fmt --all -- --check`、`cargo clippy --offline --locked --all-targets -j 2 -- -D warnings` 通过。默认目标中的 Windows 可执行文件被占用，在单独的 `target/ci-binary-check` 目录以 `cargo build --offline --locked` 构建并执行。15 项验收材料单测（含 2 项原生请求证据归属测试）及脚本编译检查通过。此次未改 Rust 或 SQL。Linux 原生加载仍待验证。

## 初次准备（2026-11-17）

日期：2026-11-17。基线为 `293e2bf6af3b180d43c8050731047b552ac07519`，
`git ls-remote origin refs/heads/main` 已核实一致；原检出 HEAD 为
`c70a1aaa0b3cb75872109406dc17b1b461d1f771`，未在其上编辑。
独立工作树 `/workspace/Origence-pilot`，分支 `eval/p0-independent-pilot`。
最终提交 SHA 由提交后根线程报告/`git rev-parse HEAD` 记录，避免文件自引用哈希。

已阅读根 AGENTS.md、当前 STATUS、架构设计、Skill/Run/存储合同、CLIENTS、
OPERATIONS、评估标准和相关 client_skill/runs 实现。基线与 `/workspace/.agents`
未发现 `.agents/skills`，未声称加载任何缺失 Skill。

- `python3 -m unittest discover -s evals/pilot -p 'test_*.py' -v`：10 tests PASS。
- `python3 evals/pilot/pilot.py generate --seed public-development-1 --output /tmp/origence-pilot-91-smoke`：成功生成 16 题和 32 条 baseline/reuse 空观察记录。
- `python3 evals/pilot/pilot.py score /tmp/origence-pilot-91-smoke/observations.json`：4 组各 8 题、32 必要检查，成功/核验均 0；返回 NOT_CERTIFIED / NOT_CHECKED。这是未跑登记，不是产品失败率或模型成绩。
- `cargo fmt --all -- --check`：未执行成功，环境 `cargo: command not found`。Rust toolchain 声明为 1.98.0；Clippy/Rust suite 同样未运行。本路不改 Rust、生产 SQL、服务/DTO、UI、CI 或共享状态。
- SQL 手工范围审查：本路 Python 没有 SQL 执行/生成接口，无数据库连接、网络或模型调用；未触及 JOIN-free 事务、权限或发布合同。

因本路明确限定新验收目录，记录放此处，不改共享 `docs/VALIDATION.md` / `docs/STATUS.md`。
合成 smoke 输出保留在上述 /tmp 路径，目录由本次生成器创建；没有创建产品数据库、集合、凭据或客户端副本。

## 未验收 / 发布边界

A1–A7 联合编号/合同仍待协调确认。四路功能须落在同一整合 SHA 后重新验收。
真实 Claude Code/Codex、服务/模型冻结、原生 Skill 加载与撤回、浏览器/Windows、
实际 SQLite/AnySeek/Blob 同版本恢复、DBA/制度负责人/独立签认全部 NOT_RUN。
未运行的收费/生产访问不构成阻塞的绕过理由。无迁移工作。
仅本地提交；不推送、不开 PR、不更新 Issue、不合入、不部署。

## #92 草稿复审修复（2026-11-17）

修复有效超限样例缺口：报销正常、有效超限、重复、缺凭证各两个不同实例，
准备/未见分组保持同一覆盖。超限非重复且有凭证，独立断言 v2 截顶、低于申请额，
且不等于按 v1 可全额报销的结果。总题数 16、观察数 32、必要检查分母均不变。
每个场景/分组用种子稳定重排后重新编号，原始种子、分组/类别标签和 oracle 答案
不进入 tasks.json。公开测试种子不是真实封存未见种子。

13 项 Python 测试通过，包括 102 个种子 × prepare/held-out 两组覆盖检查、
相同种子重现/不同种子改变类别映射、分组隔离与 Agent 输入不含 oracle。
prepare/held-out 两组生成及评分 smoke 通过，均 NOT_CERTIFIED。
主干检查仍为 293e2bf6af3b180d43c8050731047b552ac07519，修复前远端 PR head
为 045843a970944eec502b3c415805cf64b69b869b，与本地父提交一致。
未运行模型或真实试点；原有 NOT_RUN 与独立签认门槛保持。

## PRD v0.3 本地待审文档补丁（2026-11-17）

精确父提交：810f6900ff7bc990cdacd95862d5cf2da1688b75；独立本地分支
`docs/pilot-prd-v03-local`。未推送、未更新 PR/Issue、未触发远端 CI。
已通过 Library 规定流程取得 PRD v0.3 原 PDF，文件 SHA/章节及 Issue #87–#91、
草稿 #94/#96 的精确读取边界记于 PRD-MAPPING.md。

原 pilot A* 明确改为 pilot.P*，另列 prd-v0.3.A*；P5/P7 不能冒充 PRD A5/A7。
模板变更仅用于新的待审表；未迁移已有结果。`pilot.py`、测试和观察数据格式完全未改，
保留 pending 文本兼容并明确含义。尚未调用生产消费者验证模板兼容；本地检索未见
生成器/评分器读取模板。PRD 指标与离线指标分母差异已披露，未在本次悄然改评分逻辑。

最小验证：原 13 项 Python 测试通过；两份 JSON 解析、P/A 各 7 项独立命名空间、
全部 NOT_RUN、预检不评分、受管在线重载 NOT_RUN 断言通过；`git diff --check` 通过。
补丁只含文档/模板；无生产代码或 SQL 变更，无 Rust 检查要求新增。
未运行真实客户端、模型、安装、登录、生产数据、恢复或受管重载；未生成真实未见种子，
单独公开合成样例仅供未来预检，不评分、不签认。仍等待四路同 SHA、联合映射冻结及独立签认。

## #92 文档补丁发布前复验（2026-11-18）

沿用本地 6e0ac96a277be92ae7734396a51411bcdc9ad3cf，远端 #92 head 核对仍为
810f6900ff7bc990cdacd95862d5cf2da1688b75，main 仍为
293e2bf6af3b180d43c8050731047b552ac07519。按后续授权仅快进更新原草稿分支。
已检查根 AGENTS.md；/workspace/.agents 为空，仓库/工作树未发现 .agents/skills。

PREFLIGHT 补充 Windows 新 DataDir/独立 AnySeekDataDir/workspace 与初始化要求，
将 #95 旧库缺 oc_run_verifications 的拒绝归为启动 schema 不兼容；另列题面不含规则/答案的
授权真实 v2 检索和无权限干净会话对照，不进入正式分母。客户端触发/JSONL 信号仍待冻结版实查。
13 项 Python 用例、两模板命名空间/状态检查及 diff --check 均通过；不改代码/评分格式。
未执行模型、安装、登录、业务签认、Windows 或真实检索预检。#91 仍 NOT_RUN，未合入或部署。

## 已整合主干和非客户端证据（2026-11-18）

远端 main 和独立测试工作树均核对为 `48cfa7c643b8c077ad33c55981def51979afde05`。
四个实施 PR 已合入，GitHub 合入状态与本地祖先提交一致：

| 实施范围 | PR | 主干提交 |
| --- | --- | --- |
| #90 控制台治理 | [#93](https://github.com/TheLudlows/Origence/pull/93) | `9c4ed2ea6fdf17ed8b931ddcddf4f081c4367719` |
| #88 Skill 候选 | [#94](https://github.com/TheLudlows/Origence/pull/94) | `a56246406a0688f646a45a85cc2954fb14a8d28a` |
| #89 Run 核验与审阅 | [#95](https://github.com/TheLudlows/Origence/pull/95) | `e78b6ba5f3d6fcb4730789981a092c85cdc4c12a` |
| #87 MCP 与私有 Run | [#96](https://github.com/TheLudlows/Origence/pull/96) | `48cfa7c643b8c077ad33c55981def51979afde05` |

[主干 CI 44217031418](https://github.com/TheLudlows/Origence/actions/runs/44217031418)
的 `windows`、`node-ui`、`fast-check`、`linux-native` 均为 completed/success。
[#92](https://github.com/TheLudlows/Origence/pull/92) 保持草稿。正式试点仍为 **NOT_RUN**。

### 同版本恢复

本轮 Linux x86_64 使用上述 Origence 主干和 CI 固定 AnySeek
`6a46fc791c9c425e0f94fea3d04be4c1fb84dc3a`。SQLite/Blob 使用新临时权威目录，
并创建获授权和无权限两个独立合成 workspace。AnySeek 使用专用 loopback 端口和新存储根。
宿主停写并正常关闭后，在同一停写窗口复制完整权威目录，通过 AnySeek 原生服务备份
`POST /api/v1/service-backups` 和恢复 `POST /api/v1/service-restores` 生成独立目标根。
源测试实例关闭后用该新根启动恢复实例，沿用专用端口保留绑定。测试进程、目录、快照和恢复副本
在结束后清理。

按 [RUNBOOK 的同版本恢复流程](RUNBOOK.md#同版本-sqlite--anyseek--blob-恢复演练) 核对了以下
**10 组工程检查**：

1. 停机复制的完整权威目录与源目录文件哈希一致，清单包含 SQLite 和 Blob 文件。
2. AnySeek 服务备份使用相同操作 ID 重试时，返回相同回执。
3. AnySeek 恢复生成独立、尚未启动的根。同一恢复请求重试返回相同回执。
4. 恢复后的 SQLite 通过同版本离线结构检查。
5. 历史 v1 与当前 v2 正文可按精确版本读取，当前指针仍为 v2。
6. v1 和 v2 的两份原始 Blob 字节均与上传时一致。
7. 恢复后的关键词命中保留精确版本、来源、块、locator 和正文。
8. 已撤回来源的定版读取返回 404，关键词检索零命中。
9. 原 Run 前缀和同事件的准备重放回执保持一致。
10. 无权 workspace 的定版资产和原 Run 读取均返回 404，关键词检索零命中。

本次工程检查覆盖 Linux 合成数据。缺失或冲突对象负例、恢复后 Skill 摘要和任务集、
Windows 专门恢复演练及生产恢复仍待验。正式 pilot.P7 签认 **NOT_RUN**。
独立 DBA 待复查停写点、快照对应、完整断言和清理，制度负责人及独立验收人尚未签认。

验收材料自身验证通过 13 项 Python 用例、模板命名空间/未签认状态检查及 `git diff --check`。
Codex 与 Claude Code 产品客户端测试暂停，正式试点和评分均维持 `NOT_RUN`。
