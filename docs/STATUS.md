# 实现状态与后续工作

更新：2026-09-28。存储后续方向以 [整体设计 A2：存储与初始化](superpowers/specs/2026-09-22-memory-knowledge-platform-design.md#storage-design) 为准：本地固定 SQLite/LanceDB/Kuzu，PostgreSQL/pgvector 仅保留未来接口扩展能力。本地存储已推进到 **M4 适配器阶段**：接口契约、SQLite 关系库/自轮询队列/BlobStore、单 Worker OS 文件锁、跨库账本及 LanceDB/Kuzu 适配器已实现。应用仍未切换到本地后端；当前 PG 连接自动初始化新库，无版本化升级机制。

M0 独立探针已在 Windows x64/MSVC、Rust 1.88.0 完成直接使用三个 Rust 库的构建和 14 项检查。Kuzu 的写宿主拒绝其他进程打开，因此本地目标采用 API/Worker 同进程共享引擎；应用装配尚未实现。M4 已提供真实后端及账本幂等写组合测试，尚未完成整个 M4 的导入→检索→撤回→清理验收。详见 [可插拔存储计划](superpowers/plans/2026-09-22-pluggable-storage-engine.md)、[M4 适配器计划](superpowers/plans/2026-09-27-local-vector-graph.md) 与 [验证记录](VALIDATION.md)。

首版基于此前 ContextDB V3.1 业务设计，以 Rust 取代早期 Python 组件建议。应用代码复用 Axum、SQLx、Apalis、pgvector、rmcp、pulldown-cmark、pdf-extract 和 Jieba；没有引入 mem0 或 Skill。

## 本地存储后续顺序

1. 补齐 M2 的 SQLite IMMEDIATE 写事务与中文预分词查询契约。
2. M5 装配本地宿主：统一初始化、启动对账、同进程 API/Worker、CLI/MCP 转发和独占访问；接入任务取消/generation/权限复核。
3. 按图谱 G3/G4 完成 M4 剩余项：证据融合、命中可见性复核、共享 owner 清理规划和崩溃重放集成测试。`KuzuStore::delete_source` 仅按图中来源投影删除，不能替代 SQLite owner 驱动的共享对象清理。
4. 完成本地 CLI/HTTP/MCP 生命周期验收后再调整默认构建、部署和启动文档；自动发布作为随后独立产品阶段推进。

## 已实现

- workspace 级 API key 角色、事务级 RLS 上下文、运行角色安全检查、key 撤销、只存 token hash。
- 结构化记忆、原始 capture、候选审核与冲突保护、直接授权发布、审计及来源关联。
- 文本、Markdown、文本 PDF 入库；不可变版本及标题快照；追加式恢复。
- 原子业务/Apalis 入队、独立 Worker、重复执行幂等、generation/run_token 隔离、取消和显式重试。
- 删除墓碑、来源撤回、迟到任务提交阻断及派生索引清理；保留原始证据。
- 中文分词关键词检索、同 profile 精确向量检索、RRF、引用和预算组装。
- 默认关闭的兼容模型适配器、只读 MCP stdio、CLI、Compose 与 CI 配置。

## 生产前未完成

1. 企业身份、审计查询和角色管理控制面；限流、租户配额、分页和可观测指标。
2. 上游队列升级及多 Worker 并发正确性/公平调度验证。目前通过全局会话锁限制单 Worker，避免 Apalis 0.7.4 的启动重领及旧 ACK 风险。
3. retention 分类、天数和显式物理擦除协议；文件孤儿回收；备份恢复演练和删除账本重放。当前仅逻辑删除与派生索引清理，不能声称已物理擦除。
4. 共享对象存储适配、PDF OS 级资源沙箱、恶意文件压力测试。
5. 模型 profile 的在线重建/切换、精确 tokenizer、向量 ANN/重排（依据基准需求决定）。
6. 实际数据规模下的延迟、吞吐、资源和成本测量；供应商兼容性及冻结检索/Agent 评估。没有运行竞品对比。
7. 评估 fs2 → fs4 替换：fs2 疑似归档/不再维护，需 `cargo audit` 验证 RustSec 告警；当前单 Worker OS 文件锁依赖 fs2。

现有验证覆盖首版核心闭环，不等于全部 V3.1 验收场景完成。尤其备份恢复、向量 generation 切换、多租户公平性、长期压测、真正的生产供应商与最低 Rust 版本尚未验证。
