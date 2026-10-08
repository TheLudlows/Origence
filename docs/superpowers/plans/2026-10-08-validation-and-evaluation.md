# P0 验收补齐与真实评估基线

日期：2026-10-08。依据：[STATUS](../../STATUS.md)、[目标设计](../specs/2026-09-22-memory-knowledge-platform-design.md) §11/§12/A3/A4，以及 [评估协议](../../ContextDB_效果评估与对比标准.md)。不重开已完成 M0–M5/A1，不将 P2 时间语义提前放入 P1。

## 批次 A：文档与交付验收

- [x] 修订存储主计划、M5 记录及整体设计中的过期状态。
- [x] 核验现有 Windows 远端 CI 通过，并记录 commit/run/job 证据。
- [x] 将评估 G2/G3/G5 和样例调整到直接发布、提交复核及 expected_version 冲突语义。
- [x] 新增 Linux/macOS 默认后端 fmt/clippy/测试、release 构建和无模型 HTTP smoke 作业。
- [x] 新增 Linux Rust 1.88 默认后端 all-targets check，不关闭原生后端掩盖 MSRV 问题。
- [x] 新增 Linux Docker 构建、Compose 配置检查和镜像 HTTP smoke 作业。
- [ ] 读取新增 CI 结果与失败日志，修复具体问题后重新运行；通过前不标平台已支持。
- [ ] Rust 1.88 完整运行测试；若锁定依赖不兼容，先核对并固定兼容版本或如实调整最低版本，不静默放宽。
- [ ] 运行 cargo audit 并逐条记录 RustSec 告警；评估 fs2/fs4 的锁生命周期和平台兼容，不先行替换。

验收证据统一进入 VALIDATION，完成状态同步 STATUS。当前新增 CI 配置不等于上述未勾项通过。

## 批次 B：真实评估（可与 A 并行）

- [ ] 基于 evals/cases.jsonl 的样例格式扩展标注协议，冻结 source/version/locator 证据映射与数据 hash。
- [ ] 沿评估协议构建 300 个质量用例及独立治理/故障测试集；P1/P2 未支持项另列，不混入当前通过率。
- [x] 接入隔离宿主 HTTP API 的知识入库、任务等待、search 与结果导出 adapter；保留原始响应、配置、commit、模型标识和数据 hash，禁止输出凭据。已附 12 文档/24 查询合成种子；fixture 指标与传输测试通过，真实宿主运行待 CI。
- [ ] 扩展 resolve/预算证据、capture/记忆更新、人工证据标注与精确费用计量；现有 adapter 不宣称覆盖这些能力。
- [ ] 先跑关键词、向量、现有 hybrid 基线。现行 API 没有摘要/图独立开关；完整摘要/图消融须先实现明确的评估控制，不把 keyword/vector/hybrid 三种模式等同于组件消融。
- [ ] 报告 Recall@k、证据正确性、Agent 引用/任务完成率、p50/p95/p99、导入与查询成本；真实模型需明确配置及费用预算后运行。

模型 stub 只证明行为正确；没有真实结果前不报告排名。BM25、reranker、ANN 和 profile 重建以实测缺陷与规模需求选择，不预设全部立即实施。

## 本轮语义前置设计

已新增 [记忆身份与上下文契约](../specs/2026-10-08-memory-identity-and-context.md)：将授权主体与业务主体区分，明确跨主体/环境隔离、精确身份更新、未归一化状态和矛盾证据呈现。已新增 I1 的独立身份类型/编码与 8 项待 CI 执行的 Rust 测试；I1 持久化和新身份 API/表、抽取匹配、会话与上下文策略尚未实现，按 I0–I4 切片推进。短记忆不强制建图，图收益以评估决定。

## 后续阶段

按 STATUS 继续检索增强和运维治理：retention、物理擦除、孤儿文件、备份恢复演练。P1 按 A3 推进会话问答/证据与反馈、guidance、阶段化 improve、水位和长期经验入库；P2 保留 GraphCompletion、个性化、valid_from/to/as_of 和企业服务化。未来 PG 适配及本地多 Worker 不属于本批次。
