# 2026-10-11 依赖审计与处置边界

代码基线：035aa12f83fd46665636d436caf0a7e0b9a98aa3；未修改 Cargo.toml/Cargo.lock。
使用 [RustSec 官方 cargo-audit 0.22.2](https://github.com/rustsec/rustsec/releases/tag/cargo-audit%2Fv0.22.2)
Windows 发行包，下载包 SHA-256 与官方发布元数据一致。工具及数据库只放在 target。
源码安装尝试取消后改用校验后的发行包，没有向项目加入新依赖。

命令：先拉取数据库运行 cargo-audit audit --db target/audit-release/advisory-db --no-yanked --json，
再使用同一数据库执行 cargo-audit audit --db target/audit-release/advisory-db --no-fetch --json，
第二次启用默认撤回版本检查。数据库提交为 7eebec69c352c7191b1f13eb95dd510eeca5d1de，
更新时间 2026-10-09，1296 条公告、719 个锁文件依赖；未设置平台过滤或 ignore。
原始结果及工具/锁文件哈希见 [审计工件](evidence/2026-10-11-rustsec-audit.json)。

**命令退出码为 1：2 个 vulnerability、3 个 unmaintained 和 2 个 unsound 告警。不能标为审计通过。**
报告没有 yanked 告警。锁文件扫描和运行路径分析分开记录：

| 公告/依赖 | 当前证据 | 处置 |
| --- | --- | --- |
| [RUSTSEC-2026-0189](https://rustsec.org/advisories/RUSTSEC-2026-0189.html)，rmcp 0.8.5 | 公告针对 Streamable HTTP；cargo tree -e features -i rmcp 只有 server/macros/base64/transport-async-rw/transport-io，src/mcp.rs 两个入口均为 stdio | 当前未启用受影响传输；升级到 >=1.4.0 应单独验证协议兼容。添加 HTTP MCP 前必须处理；不忽略告警 |
| [RUSTSEC-2023-0071](https://rustsec.org/advisories/RUSTSEC-2023-0071.html)，rsa 0.9.10 | cargo tree --offline --locked -i rsa --target all 无激活依赖路径；锁文件仍包含它 | 不手改锁文件隐藏告警；如启用新的数据库/加密 feature，重新评估。公告当前没有 patched 版本 |
| [RUSTSEC-2026-0002](https://rustsec.org/advisories/RUSTSEC-2026-0002.html)、[RUSTSEC-2026-0253](https://rustsec.org/advisories/RUSTSEC-2026-0253.html)，lru 0.12.5 | 激活路径为 lancedb 0.23.1 → lance/lance-index 1.0.1 → tantivy 0.24.2 → lru。Tantivy store reader 使用 LruCache<usize, Block> 的 get/put/len/peek_lru，未观察到公告中的 IterMut 或 pop 调用 | 当前调用检查降低这些具体触发条件的可达性疑虑，但不是安全证明。两个公告均修复需 >=0.18.2，超出上游 0.12 约束；需单独评估上游升级与存储兼容，禁止随意替换锁文件版本 |
| fxhash 0.2.1、paste 1.0.15、ttf-parser 0.25.1 | 维护状态公告 RUSTSEC-2025-0057、RUSTSEC-2024-0436、RUSTSEC-2026-0192 | 纳入依赖更新计划；停止维护不等同于本轮已验证漏洞利用 |

当前向量实现使用 nearest_to/only_if 查询，未配置 Lance/Tantivy 全文索引；关键词
检索在 SQLite。此事实只描述当前代码，不能保证所有上游内部执行路径都不可达。

下一次依赖维护应独立于 AML 协议改动：确认上游可用版本及所需 features，使用 Cargo
更新依赖与锁文件，复核旧库读取/拒绝边界、原文定位、MCP stdio、上传/job/search、
恢复与三平台/容器 CI。不通过 ignore、局部手改传递版本或删除测试把审计变绿。
公网生产验收仍需明确处置这些保留项。
