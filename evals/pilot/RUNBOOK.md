# 证据、恢复和清理手册

## 开跑前

1. 填冻结表：Issue/合同修订、四路交付 SHA 与整合 SHA、二进制哈希、客户端版本及安装来源、OS、AnySeek 二进制/配置/集合 schema、SQLite 运行版本、Blob root/manifest、模型供应方与不可变修订、token/时间/成本预算。版本不可固定则 BLOCKED，不填 guessed/latest。
2. 仅授权合成或已脱敏样本；本包默认只合成。数据负责人签认数据边界。新建测试 workspace、最小角色 key 和独立新库；记录资源清单，但只记 key 的内部别名/逻辑 owner，不记密钥。测试身份不等于允许连接真实业务数据库。
3. 由 DBA 和制度负责人冻结 rubric、必要检查、成功阈值与停止条件。实际模型调用及费用需单独已授权预算；本地自动检查不触发此类调用。
4. 四路负责人在同一整合提交登记交付和测试；独立验收人重新跑联合路径。分支单测和导出夹具只能是辅助证据。

## 真实闭环与精确加载证据

Codex 与 Claude Code 的产品客户端测试均已暂停。本节保留恢复试跑后的原验收步骤。

研发首做必须在 Claude Code，业务首做必须在 Codex。记录真实终端版本输出、会话 ID、任务输入摘要、MCP 工具请求/响应及 server Run ID。连接等待题使用给定合成快照，DBA 校验诊断，不让客户端触达生产。

从首做的精确资产版本或不可变 Run 前缀生成候选，保留 revision、选定脱敏依据、审阅人、审阅 packet digest、批准时间和发布 skill ID/version/package digest。修改候选后验证旧批准失效并重新审批；不能自动发布。

复做客户端在隔离项目、干净会话中，先通过服务器在线授权校验取得/显式导出精确版本，再独立观察真实客户端原生加载。两步不能合并称为受管在线加载。保存：

- 服务器 scope/version/source 检查与返回 digest 的证据；
- 原生客户端实际加载的 Skill 路径/标识、SKILL.md 与 references 字节哈希、客户端 session/run 对应及原生加载事件/可读诊断；
- 内容进入实际模型上下文的可观测记录，及该记录是否由客户端报告、由服务端准备或独立观察；
- 工具调用、输出工件哈希、独立负责人复核与必要检查结果。

精确绑定是 `(tenant, workspace, skill UUID, version, package digest, artifact hashes, client session, Run, time)`。
包 digest 按产品规范校验，不能把单个 SKILL.md 哈希当成 canonical package digest。
目录存在、skill-export 成功、MCP 注册、`skill_loaded_reported` 或模型说“我已加载”均不能单独证明原生加载。
若冻结客户端没有可核验加载证据，记录 UNKNOWN/BLOCKED，不补造截图/日志；不以离线回放替代。
来源撤回后分别观察新的在线获取/导出是否被拒、既有文件/缓存是否残留、既有会话与新会话的实际加载状态。阻止新在线获取不等于回收副本或清除已注入上下文；停止使用旧会话并记录残留边界。

受管在线重载是独立未验证项：需要另有受管机制、精确版本切换/再授权与撤回行为的真实证据，现有导出和 MCP Run 上报不提供此证明。本补丁不降低原要求，也不新增该功能。缺加载/模型上下文证据标 UNKNOWN，未执行标 NOT_RUN；业务成功不能填补两者。能力预检见 [PREFLIGHT](PREFLIGHT.md)，依据与交付边界见 [PRD-MAPPING](PRD-MAPPING.md)。

## 证据封装

每次实验单独受限目录，包含 freeze、tasks hash、oracle 承诺、bindings、原始客户端转录、服务日志、trace/bundle、加载记录、输出、独立判分和签认。
证据索引每条含相对路径、SHA256、字节数、采集时间 UTC、采集者、证据类型与分别对应 prd-v0.3.A*、pilot.P*、题号及精确版本。
原始敏感项留受限存储，公开附件另做脱敏副本并标记来源；不把私有原文、密钥、真实 endpoint 诊断贴入提交。
对缺失日志记 not-collected；正文提及一个工具不证明调用。索引哈希只能证明固定字节，不能证明真实性。

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
