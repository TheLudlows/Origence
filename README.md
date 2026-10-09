# openContext / ContextDB

Rust 实现的团队 Agent 记忆与知识服务。M5 将默认运行栈固定为 **SQLite + LanceDB + Kuzu + 本地文件**：一个宿主进程运行 HTTP API 和单 Worker，CLI/MCP 默认通过 HTTP 访问宿主。无需 PostgreSQL、pgvector 或独立队列服务。

数据按 tenant/workspace 隔离；writer 的结构化记忆与 capture 抽取结果直接进入发布任务，提交时重新核验身份、权限、来源与预期版本；检索只返回当前已发布、资产未删除且来源仍有效的证据。会话记忆、企业 SaaS 仍属于后续阶段。

2026-10-09 核验：PR #10 已合入 main `73fc51a`，显式身份、单身份 capture、lookup、身份标注/过滤与调用方版本前置条件已落地；轻量 CI 的 27 项 lib、58 项无原生集成和 Python 5 项通过。当前代码的原生/平台/release/container 验收尚未全部完成，真实模型效果尚未验证；最新能力与证据分别见 [STATUS](docs/STATUS.md) 和 [VALIDATION](docs/VALIDATION.md)。

## 快速启动

已实测平台为 Windows x64/MSVC、Rust 1.96.0。源码构建需要 Rust、C++ 工具链、CMake、Ninja 和 protoc；原生依赖构建较大，建议预留充足磁盘并限制并行链接。Windows 安装 Visual Studio C++ 工具后可使用脚本自动定位工具：

```powershell
./tools/build.ps1 -Action Build
$env:OC_DATA_DIR = "$PWD/.data"
./target/debug/opencontext --offline workspace-create demo
./target/debug/opencontext serve
```

`workspace-create` 输出 workspace ID、key ID 和仅展示一次的 admin token；数据库只保存 token 的 SHA-256。首次启动自动创建空库结构；已有库校验必要字段，不执行启动时版本升级或自动修复；兼容本地旧库可离线显式安装记忆身份表，见 [运维说明](docs/OPERATIONS.md#显式启用记忆身份)。第二个宿主或离线命令打开同一数据目录会失败。

在另一个终端配置 token，通过运行中的宿主管理同 workspace 的凭据：

```powershell
$env:OC_API_KEY = '替换为返回的 token'
$env:OC_SERVER_URL = 'http://127.0.0.1:8080'
./target/debug/opencontext key-create WORKSPACE_UUID --role reader
./target/debug/opencontext key-revoke KEY_UUID
curl.exe http://127.0.0.1:8080/health/ready
```

离线管理必须先停止宿主，再显式传 `--offline`。此模式依赖数据目录的操作系统访问权限；不要将目录直接交给 Agent。远程 HTTP 地址要求 HTTPS，仅回环地址允许 HTTP。

## 第一个闭环

以下 curl 示例使用 POSIX shell；Windows 可用 PowerShell `Invoke-RestMethod` 发送同样的 JSON。业务修改请求需 `Idempotency-Key`。

```sh
curl -sS http://127.0.0.1:8080/v1/memories \
  -H "Authorization: Bearer $OC_API_KEY" \
  -H 'Idempotency-Key: release-rule-1' -H 'Content-Type: application/json' \
  -d '{"fact_key":"release.policy","content":"生产发布必须经过审批"}'
curl -sS http://127.0.0.1:8080/v1/jobs/JOB_UUID -H "Authorization: Bearer $OC_API_KEY"
opencontext search '发布审批'
opencontext resolve '发布审批' --budget-tokens 2000
opencontext get ASSET_UUID
```

创建响应只代表受理；任务达到 `completed` 且 `outcome=published` 后才表示发布完成。已有事实的修改通过 expected_version 乐观校验追加版本，冲突时任务 superseded 且旧值保留。恢复追加新版本，删除资产不可恢复，撤回来源使所有引用该来源的版本不可读。完整契约见 [API](docs/API.md)。

## MCP

客户端启动 `opencontext mcp`，设置 `OC_SERVER_URL` 和 workspace 的 `OC_API_KEY`。提供只读工具 `context_search`、`context_get`、`context_resolve`；每次请求由宿主重新认证，撤销 key 后已有 MCP 进程也不能继续读取。stdout 仅传协议，日志写 stderr。

宿主不可达时明确报错，不自动打开数据库。仅停止宿主后，可用 `opencontext --offline mcp` 或 `--offline search/get/resolve` 独占读取数据。

## 配置

| 配置 | 默认值 / 用途 |
| --- | --- |
| `OC_DATA_DIR` | `.data`；SQLite、向量、图和原文件的统一根目录 |
| `OC_BIND` | `127.0.0.1:8080`；宿主监听地址 |
| `OC_SERVER_URL` | `http://127.0.0.1:8080`；CLI/MCP 目标宿主 |
| `OC_API_KEY` | 客户端 workspace 凭据 |
| `OC_ENABLE_MODELS` | `false`；默认不调用外部模型 |
| `OC_MODEL_BASE_URL` / `OC_MODEL_API_KEY` | OpenAI 兼容模型服务地址和凭据 |
| `OC_EMBEDDING_MODEL` / `OC_EMBEDDING_DIMENSION` | 可选向量模型及维度 1–4096 |
| `OC_EXTRACTION_MODEL` | 可选记忆抽取、知识摘要和实体关系抽取模型 |

模型根地址通常以 `/v1` 结尾，远程要求 HTTPS；禁用重定向，单次调用超时 45 秒，响应上限 8 MB。启用模型意味着内容发送至该服务，并可能产生费用。关闭模型仍可发布结构化记忆和知识并执行关键词检索；capture 在未配置抽取模型时明确失败。

发布固定接收时的 embedding profile。外部写前保存发布计划和 pending 账本，外部写后重新核验权限、来源、预期版本、取消状态及 generation/run_token，再原子确认版本、owner、索引与任务。启动检查后先回收遗留任务并推进 run_token，再对账清理，完成后才启动 API/Worker。已保存计划可重放；保存前的模型调用可能重复，不保证费用恰好一次。

数据布局、备份、故障恢复与旧 PG 基线处理见 [运维说明](docs/OPERATIONS.md)。

## Docker Compose

提供 Linux 容器构建配置；本次 Windows 验收不等同于 Linux 镜像运行验证。参见 [验证记录](docs/VALIDATION.md)。仅一个宿主容器挂载数据卷：

```sh
cp .env.example .env
docker compose build
docker compose run --rm admin --offline workspace-create demo
docker compose up -d host
# 离线管理前先停止 host
docker compose stop host
docker compose run --rm admin --offline key-create WORKSPACE_UUID --role reader
docker compose up -d host
```

## 开发与验证

```powershell
./tools/build.ps1 -Action Validate
```

工具已在 PATH 中时，等价检查为：

```sh
cargo fmt --all -- --check
cargo clippy --locked --all-targets -j 1 -- -D warnings
cargo test --locked -j 1 --no-fail-fast
```

默认启用 `local-storage`；业务/进程测试使用临时目录和本地模型模拟服务，无数据库账号或付费模型要求。集成测试统一编入 `tests/local.rs`，减少原生依赖重复链接。`--no-default-features --lib` 仅用于基础接口检查，不是另一套应用后端。

## 当前边界与文档

- 检索使用预分词关键词扫描、精确向量查询、摘要证据和一跳图扩展，以 RRF（k=60）融合；无 ANN、重排、自动回填或在线 profile 切换。
- 图谱共享对象以 SQLite owner 为权威；返回规范化实体名、关系及当前来源证据，不将最后一次模型描述当作共享事实。
- `budget_tokens` 使用 UTF-8 字节数的保守上界，整块保留或丢弃；不是模型精确 tokenizer。
- 文本上限 1 MB，文件 10 MiB，PDF 最多 200 页、解析超时 30 秒，无 OCR；PDF 子进程不是 OS 安全沙箱。
- 删除立即阻断读取，后台清理派生索引。原文件、事件、版本及审计暂保留；无 retention、原文物理擦除或文件孤儿自动回收。
- 单宿主、单 Worker；无多节点共享写、OIDC、文档 ACL、配额、分页游标或生产 SLO 承诺。

[文档索引](docs/README.md) 区分当前运行契约、里程碑、未来设计和历史调研；[状态](docs/STATUS.md)、[验收](docs/VALIDATION.md) 和 [M5 记录](docs/superpowers/plans/2026-09-28-local-host-delivery.md) 说明交付范围。Cognee 调研固定到 1.6.0 源码，未集成 Cognee，也未运行竞品效果对比。

