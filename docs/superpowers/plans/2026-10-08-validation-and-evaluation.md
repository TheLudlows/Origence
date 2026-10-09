# P0 验收补齐与真实评估基线

日期：2026-10-08；状态更新：2026-10-09。依据：[STATUS](../../STATUS.md)、[目标设计](../specs/2026-09-22-memory-knowledge-platform-design.md) §11/§12/A3/A4，以及 [评估协议](../../Origence_效果评估与对比标准.md)。M0–M5/A1 基础及现有 CI 已验收；完整质量与运维验收仍需补齐。本计划区分接口/用例已加入、轻量验证和原生运行通过，不将 P2 时间语义提前放入 P1。

当前验收代码 `8d8d224`（PR #11），基于已合入的 PR #10。[CI run 37871058933](https://github.com/TheLudlows/openContext/actions/runs/37871058933) 已 7/7 通过，覆盖轻量、三平台原生生命周期测试、Linux/macOS release/HTTP smoke 与关键词宿主评估、容器及 Rust 1.88 默认后端 check；全部日志和两平台工件已核验，精确结果见 [VALIDATION](../../VALIDATION.md)。本轮 CI 与文档收口完成，候选过滤、真实质量评估和 P1 功能仍是后续工作。

## 批次 A：文档与交付验收

- [x] 修订存储主计划、M5 记录及整体设计中的过期状态。
- [x] 核验现有 Windows 远端 CI 通过，并记录 commit/run/job 证据。
- [x] 将评估 G2/G3/G5 和样例调整到直接发布、提交复核及 expected_version 冲突语义。
- [x] 新增 Linux/macOS 默认后端 fmt/clippy/测试、release 构建和无模型 HTTP smoke 作业。
- [x] 新增 Linux Rust 1.88 默认后端 all-targets check，不关闭原生后端掩盖 MSRV 问题。
- [x] 新增 Linux Docker 构建、Compose 配置检查和镜像 HTTP smoke 作业。
- [x] 读取 PR #10 历史失败日志，修复 rustfmt 与 readiness ConnectionResetError；修复已合入 main，轻量检查和 native fmt 已通过。
- [x] 核验当前 main 的 27 项 lib、58 项无原生集成及 Python 5 项日志，将 commit/run/job 与失败修复证据写入 VALIDATION。
- [x] 读取 macOS 原生失败日志，修复综合测试 `published` 变量遮蔽导致 MCP get 的 asset_id 为 null；保留读取/撤销断言并增加错误响应诊断。
- [x] 修复代码 `8d8d224` 的 fmt/clippy 与三平台完整原生生命周期测试通过；记录实际运行证据。
- [x] 统一 STATUS 当前能力表、整体/身份设计、API、README 与存储/M5 记录；将历史状态与最新快照分开，修正应用层厂商解耦的完成边界。
- [x] 验收代码 `8d8d224` 的 Windows、Linux/macOS 默认后端完整检查/测试通过。
- [x] 验收代码 `8d8d224` 的 Linux/macOS release HTTP smoke 与无模型关键词宿主评估通过；下载两平台工件，核对 commit、输入 hash、逐题标签、原始命中来源及 UTF-8 区间，独立重算指标一致。
- [x] 验收代码 `8d8d224` 的 Rust 1.88 默认后端 all-targets check 与镜像构建/HTTP smoke 通过。
- [ ] Rust 1.88 完整运行测试；若锁定依赖不兼容，先核对并固定兼容版本或如实调整最低版本，不静默放宽。
- [ ] 运行 cargo audit 并逐条记录 RustSec 告警；评估 fs2/fs4 的锁生命周期和平台兼容，不先行替换。

验收证据统一进入 VALIDATION，完成状态同步 STATUS。当前新增 CI 配置不等于上述未勾项通过。

## 批次 B：真实评估（可与 A 并行）

- [ ] 基于 evals/cases.jsonl 的样例格式扩展标注协议，冻结 source/version/locator 证据映射与数据 hash。
- [ ] 沿评估协议构建 300 个质量用例及独立治理/故障测试集；P1/P2 未支持项另列，不混入当前通过率。
- [x] 接入隔离宿主 HTTP API 的知识入库、任务等待、search 与结果导出 adapter；保留原始响应、配置、commit、模型标识和数据 hash，禁止输出凭据。12 文档/24 查询合成种子已在 Linux/macOS release 宿主运行并核验工件；两平台无模型 keyword 的 Recall@5 均为 63.6%，相同 8 个可答问句漏召回，不代表真实语义质量。
- [ ] 扩展 resolve/预算证据、capture/记忆更新、人工证据标注与精确费用计量；现有 adapter 不宣称覆盖这些能力。
- [ ] 保留已核验的关键词失败用例，评估全词匹配对问句/多来源的影响，补 keyword/vector/hybrid 对照基线。现行 API 没有摘要/图独立开关；完整摘要/图消融须先实现明确的评估控制，不把三种检索模式等同于组件消融。
- [ ] 报告 Recall@k、证据正确性、Agent 引用/任务完成率、p50/p95/p99、导入与查询成本；真实模型需明确配置及费用预算后运行。

模型 stub 只证明行为正确；没有真实结果前不报告排名。BM25、reranker、ANN 和 profile 重建以实测缺陷与规模需求选择，不预设全部立即实施。

## 本轮语义前置设计

已新增 [记忆身份与上下文契约](../specs/2026-10-08-memory-identity-and-context.md)，I1 编码/SQLite 唯一绑定/显式写入/离线安装/lookup/调用方版本条件已实现；受限 I2 单身份 capture 与来源读取、I4 标注及精确过滤也已实现。验收代码的无原生与三平台原生 HTTP/Worker、render/预算、CLI 用例，Linux/macOS release/HTTP smoke 及容器 smoke 均通过。自动身份推断、属性目录、未归一化状态、语义冲突、I3 会话与完整 I4 类型策略仍未交付，不能以显式绑定或 quote 校验替代语义质量。短记忆不强制建图，图收益以评估决定。

最新的 S0–S4 交付顺序、P1 Session/Feedback/Improve/Learning 切片及完成标准见 [2026-10-09 剩余任务路线图](2026-10-09-next-stage-roadmap.md)。此文件保留 P0 验收、检索实验与既有历史证据，不用设计清单替代 CI 状态。

## 后续阶段

CI 证据已收口，后续按 STATUS 的顺序推进：身份候选过滤下推及反例 → 关键词失败用例、人工标注与真实检索基线/组件消融 → I2 必要语义契约与 P1 最小会话闭环。P1 第一版先做会话及实际证据归档、幂等反馈、显式 improve、有来源的经验重新入库，不要求一次实现全部 A3 阶段。ANN/重排/profile/tokenizer 由实测缺陷与规模需求选择。Rust 1.88 完整运行测试、依赖审计、retention、物理擦除、孤儿文件及备份恢复保留独立待办；P2 保留 GraphCompletion、个性化、valid_from/to/as_of 和企业服务化。未来 PG 适配及本地多 Worker 不属于本批次。

## I1 显式身份表升级执行清单

- [x] 提供独占现有库的 `--offline memory-identity-upgrade` 与 dry-run，不在 startup 初始化中迁移。
- [x] fresh-data 与升级共用身份 DDL，事务安装并拒绝不兼容基础结构/身份约束。
- [x] 保留 legacy asset/fact_key/source/version，不做身份推断或回填。
- [x] 补 SQLite/跨进程锁/CLI 用例，轻量 CI 扩至完整无原生集成套件；同步 API 与运维文档。
- [x] 本轮 Rust fmt、8 项身份编码与 52 项无原生集成测试通过（CI run 37765440784；代码 `0dd6f4e`）。
- [x] 三平台默认原生构建 CLI 用例、Linux/macOS release 与容器烟测通过（run 37871058933）。

## I2 首个受限入口

- [x] 调用方明确完整 identity 的 capture API，scope 从认证取得。
- [x] 抽取只返回精确原文片段与 UTF-8 区间，持久化 locator；单身份不同断言拒绝更新。
- [x] 使用受理时 expected_version，复用来源/幂等/Worker 发布治理，不改变旧 capture。
- [x] 新证据解析用例通过 CI（run 37768221568：22 项 lib、52 项无原生集成）。
- [x] 三平台原生 HTTP/Worker 用例通过；Linux/macOS release 和容器的身份发布/版本/隔离 smoke 通过（run 37871058933）。
- [ ] 自动身份推断、未归一化资产状态、属性目录、别名/语义匹配及真实模型评估。

## I2 读取契约补齐

- [x] 资产输出显式身份/旧未识别/非记忆状态，不回填旧身份或声称语义验证。
- [x] writer 来源原文读取，跨 scope/撤回/文件删除不可见；补 SQLite 与 HTTP 用例。
- [x] 新 SQLite 用例通过 CI run 37771835095（58 项无原生集成）。
- [x] 原生 HTTP 权限与治理用例在三平台通过（run 37871058933）。
- [ ] 未归一化资产状态机、自动匹配及真实模型评估，仍按后续切片推进。

## I2 精确身份查找

- [x] reader 精确查找完整身份的当前已发布记忆，scope 取自认证。
- [x] 复用 asset_view 来源和墓碑过滤；未命中不创建占位资产，不回退历史版本。
- [x] 增加 SQLite 无写入/隔离/治理和原生 HTTP reader/跨 scope 用例；同步设计进度。
- [x] Rust fmt、22 项 lib 与 58 项无原生集成通过 CI run 37771835095。
- [x] 原生 HTTP 用例在三平台通过（run 37871058933）。
- [ ] 自动身份推断、属性目录、未归一化状态和语义匹配，不能由精确查找替代。

## I4 身份标注首个切片

- [x] search 最终授权读取中绑定业务身份，resolve 引用与文本保留类型和身份状态。
- [x] 全部标注计入既有预算，整块渲染；增加 context_policy 版本标识。
- [x] 增加解码/类型状态、预算边界和原生 HTTP 标注断言。
- [x] 当前 main 无原生 lib 测试通过（27 项，run 37868289334）；不覆盖 retrieval.rs 的原生 render 测试。
- [x] 原生 render/预算与 HTTP 用例在三平台通过（run 37871058933）。
- [ ] 冲突识别、类型/任务预算策略、会话与条件化经验仍未交付。

## I4 精确身份检索范围

- [x] search/resolve 支持完整身份过滤，保持 reader/scoped/current/source 权限边界。
- [x] 精确身份匹配先于最终响应 limit，排除旧未识别记忆、知识与图扩展；省略过滤时保持混合检索。
- [ ] 身份限定下推到候选生成，覆盖分支已取 top-100 后目标被排除的反例；当前实现/API 明确保留候选范围限制。
- [x] 同步 HTTP/MCP Schema 和 CLI 默认值，补默认兼容、身份反例及原生 HTTP 隔离用例。
- [x] 当前 main 默认请求/Schema/身份匹配等无原生测试通过（run 37868289334）。
- [x] 原生 retrieval/HTTP 用例在三平台通过（run 37871058933）。
- [ ] 部分主体条件过滤、冲突策略、类型预算、自动匹配与会话闭环仍未交付。

## I1/I2 调用方版本前置条件

- [x] 显式写入与单身份 capture 接受可选非负 expected_version，在受理事务中比对。
- [x] 保留 Worker 复核、成功幂等重放与旧请求 payload；0 明确表示尚无已发布版本。
- [x] 补版本/兼容单元测试和原生 HTTP 拒绝/重放断言，同步文档。
- [x] PR #10 已合入 `73fc51a`，3 项新增版本/兼容 lib 测试随 27 项 lib 通过当前 main CI。
- [x] 新增原生 HTTP 与 PR #9 过滤用例在三平台通过（run 37871058933）。
- [ ] 自动匹配、属性目录、未归一化状态、冲突策略及会话闭环仍未交付。

