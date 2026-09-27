# 实现状态与后续工作

更新：2026-09-28。M5 本地宿主已经接入 SQLite/LanceDB/Kuzu/本地 Blob，默认构建不依赖 PG/Apalis。实际检查及平台范围见 [VALIDATION](VALIDATION.md)，使用入口见 [文档索引](README.md)。

## 已交付

- 单宿主共享引擎：先初始化、检查、恢复任务与对账清理，再运行 HTTP 和单 Worker。OS 文件锁拒绝另一进程打开同库；Kuzu 同步调用通过有界阻塞执行器串行执行。
- `OC_DATA_DIR` 统一数据目录；CLI 的 `serve`、workspace/key 管理、search/get/resolve 和 MCP。客户端默认转发 HTTP；离线显式 `--offline`，宿主不可达不回退。
- SQLite 显式 scope 过滤，复合外键、WAL、IMMEDIATE 写事务、原子幂等/审计/入队；撤销与短写事务串行化，外部 IO 后重新认证和核验提交条件。
- 记忆候选、审核、授权首次发布、知识入库、不可变版本、历史标题、追加恢复、文件上传和文本 PDF 解析。
- 持久化发布计划与跨库账本；确定性产物 ID、generation/run_token 隔离、取消、有限自动重试、显式重试和启动恢复。外部产物只有在 SQLite 最终提交后可作为证据。
- 中文预分词关键词检索、精确向量检索、摘要/图谱证据与 RRF、引用和保守预算。最终结果核验当前版本、来源、资产和授权。
- 删除立即阻断读取，后台按 SQLite owner 清理派生数据；共享实体和关系有其他有效来源时保留，最后一个 owner 消失才删除。
- 普通测试用临时数据目录运行，涵盖 API/CLI/MCP/PDF 子进程、本地模型模拟和恢复；CI、容器配置及文档统一到单宿主。

旧 PostgreSQL 运行代码、Apalis SQL 和专用测试从活动树裁剪，历史参考为提交 `72fb5aa`。未提供 PG 到本地库的数据迁移或可选 PG 运行模式；未来可实现存储接口，不能据此宣称已支持。

## 接下来

1. 按 [自动发布计划](superpowers/plans/2026-09-22-auto-publish.md) 单独调整治理语义；当前模型抽取仍不能自我授权发布。
2. Linux/macOS/release 和容器运行验收、最低 Rust 版本验证、远端 CI 结果；Windows 本地通过不替代这些证据。
3. 冻结真实检索与 Agent 评估集，验证图谱/摘要收益和共享实体合并策略；当前模型 stub 不代表语义质量或性能。
4. 在线 profile 重建、ANN/重排、精确 tokenizer、检索分页和规模控制。
5. retention、原文物理擦除、孤儿文件回收、完整备份恢复演练；目前只能停止宿主后整体复制数据目录。
6. 企业身份、文档 ACL、配额、公平调度、审计查询、指标和多节点架构；不扩展本地多 Worker。
7. P1 会话记忆/指导/反馈/经验蒸馏及 P2 服务化，按 [目标设计](superpowers/specs/2026-09-22-memory-knowledge-platform-design.md) 推进。

当前仍是本地原型交付，不承诺生产 SLO、完整 V3.1 场景或竞品效果排名。依赖安全审计、PDF OS 资源沙箱、断电恢复和长期压测尚未完成。
