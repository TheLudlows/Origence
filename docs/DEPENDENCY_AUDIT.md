# 2026-10-12 MCP 依赖处置与保留风险

本轮以 main `7c8531afbfa2612a35d6869668df29f761c1d4a6` 为基线，范围仅为
`rmcp 0.8.5 → =1.8.0`、必要的 MCP API 兼容适配与真实 stdio 回归。
[PR #47](https://github.com/TheLudlows/Origence/pull/47) 不升级 LanceDB/Tantivy/PDF，
不增加 HTTP MCP、模型调用、存储 schema 或迁移。

## 变更边界

- [rmcp 1.8.0 官方 API](https://docs.rs/rmcp/1.8.0/rmcp/) 已核验；显式关闭默认
  features，只启用 `server`、`macros`、`transport-io`（派生 `transport-async-rw`）。
- 使用 Rust/Cargo 1.98.0 的 `cargo update -p rmcp --precise 1.8.0` 生成锁文件。
  仅升级 rmcp/rmcp-macros、其 darling 宏依赖族并新增 pastey；不手工生成版本或 checksum。
- 四个只读工具、字段/默认值、结果 JSON 文本、Service 权限及 HostClient 转发不变。
  SDK 新版把参数解码错误变为 `isError` 结果；本项目通过已启用路由的公开回调保持原有
  JSON-RPC `-32602` 错误，而非按字符串反向转换。显式使用已有实例 router。
- 保留原 SDK 的未知协议版本回退 `2025-03-26`。1.8.0 会返回其识别的客户端请求版本；
  回归覆盖 `2024-11-05`、`2025-03-26`、`2025-06-18`、`2025-11-25`、`2026-07-28`
  及未知版本回退。输入 JSON Schema 方言由 draft07/nullable 改为 2020-12；字段、
  必填、类型、可空、默认值和拒绝未知字段的约束保持等价，不产生 outputSchema。
- 应用领域错误继续使用既有安全消息；SDK/serde 参数解码诊断沿用上游行为，可能包含
  调用方提交的错误字段或值。本轮只保持错误协议，不宣称所有参数诊断均已脱敏。

## 本轮实际审计结果

对提交 `8e1c6cb5e06cb7a211b07b9223af69a46308328f` 的已提交锁文件运行
`cargo audit --json`，工具 0.22.2，默认 registry/yanked 检查，不设置 ignore、平台或严重度过滤。
锁文件 SHA-256 `afa7c5cf97d78d7fc6d5480d6bd7387ba1d80f139c7a79eb5eb198b7e03ea475`；
RustSec 数据库提交 `7eebec69c352c7191b1f13eb95dd510eeca5d1de`，1296 条公告、720 个锁依赖。

**实际退出码 1：1 个 vulnerability（rsa）、3 个 unmaintained、2 个 unsound（lru）及
1 个 yanked（yoke-derive）。rmcp 的 RUSTSEC-2026-0189 已从报告移除；不是 clean audit。**
审计收集 job 成功仅表示工件生成成功，不能代替审计退出码。

[持久化 JSON 证据](evidence/2026-10-12-mcp-dependency-audit.json) 包含完整审计结果、
六组 feature tree 及各命令退出码、工具版本和锁文件哈希、原始 CI ZIP 链接及 SHA-256。
无默认 features 时 lru 命令退出 101（包不在激活图中），不是构建失败；rsa 两组均无激活路径。
本轮首次锁文件由 Cargo 生成；最终审计只用 `cargo metadata --locked` 验证，再读取原样锁文件，
已逐字节确认审计工件与提交版本一致。中间一次重复 update 的工件不作为最终证据。

## 仍需独立处置的风险

- [rsa RUSTSEC-2023-0071](https://rustsec.org/advisories/RUSTSEC-2023-0071.html)：
  官方仍无 patched 版本。锁文件的 sqlx-mysql 路径不等于当前编译激活；default 与
  no-default 的全平台 feature tree 均无 rsa 激活路径。不删除锁项或设置 ignore。
- [lru RUSTSEC-2026-0002](https://rustsec.org/advisories/RUSTSEC-2026-0002.html) 与
  [RUSTSEC-2026-0253](https://rustsec.org/advisories/RUSTSEC-2026-0253.html)：默认构建
  仍经 LanceDB/Lance/Tantivy 激活 lru 0.12.5；no-default 无此包。两个公告都修复需
  `>=0.18.2`；核验时 [Tantivy 0.26.2](https://docs.rs/crate/tantivy/0.26.2/source/Cargo.toml)
  仍约束 lru `^0.16.3`，单独升级到此上游版本也不能消除两个告警。本轮不强改传递约束。
- fxhash、paste、ttf-parser 的维护状态告警继续保留；rmcp 使用 pastey 不会消除其他路径的 paste。
- 新的 registry 检查报告原锁中已有的 yoke-derive 0.8.3 被撤回；本轮不把这个事实隐去，
  也不扩大到无关依赖批量更新。此项需独立验证替代版本。

下列 2026-10-11 记录为历史基线，不代表升级后的当前数量或风险已全部关闭。

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
