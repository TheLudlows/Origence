# AML 本地预检与运行演练

本工具只生成自己的合成数据，不接收评测平台的私有题目或消息。它启动独立
Origence 二进制和单 Worker，使用仓库 target/aml-drills 下的新临时目录，结束后
终止自己启动的宿主并删除该目录。不会打开业务 OC_DATA_DIR，不修改模型服务。

## 本地优先与官方 Smoke 的区别

本地使用 Ollama BGE-M3（1024 维）演练，无需公网或域名。官方 Smoke 需平台 AML Key 和可访问的 Add/Search，公开仓库未提供完整离线 runner；规则核对记录与正式进入条件见 [AML 计划](AGENT_MEMORY_LEADERBOARD.md)，当前结果见 [STATUS](STATUS.md)。本地模拟不能登记为官方 Smoke 通过。

## 模型与运行

先构建声明工具链下的宿主：

~~~text
cargo build --locked -j 2
~~~

通过进程环境设置 OC_MODEL_BASE_URL、OC_MODEL_API_KEY、OC_EMBEDDING_MODEL、
OC_EMBEDDING_DIMENSION。凭据不要放在命令参数、报告或仓库中。端点必须为 HTTPS
或本机 loopback HTTP；兼容接口地址须包含提供商需要的 /v1 等前缀。
只提供 chat/completions 的网关不能代替 embeddings；不从聊天文本伪造向量。
本工具固定关闭抽取、摘要和图，不继承调用环境中的业务目录或应用 API key。

本机 PowerShell 配置（ollama 是本地占位 key，不是远端凭据）：

~~~powershell
$env:OC_MODEL_BASE_URL = 'http://127.0.0.1:11434/v1'
$env:OC_MODEL_API_KEY = 'ollama'
$env:OC_EMBEDDING_MODEL = 'bge-m3'
$env:OC_EMBEDDING_DIMENSION = '1024'
~~~

~~~text
python tools/aml_drill.py --preflight-only --report target/aml-preflight-001.json
python tools/aml_drill.py --binary target/debug/origence.exe --users 8 --concurrency 2 --rounds 3 --report target/aml-drill-001.json
python -m unittest discover -s tools -p 'test_*.py' -v
~~~

Linux/macOS 将 binary 改为 target/debug/origence。每次使用新报告路径；工具拒绝
覆盖既有报告。预检真实调用 embeddings，检查维度、有限数值与非零向量；请求
不跟随重定向。失败只记录固定错误码和 HTTP 状态，不输出服务端诊断或凭据。

演练每用户写入两个 session，使用相同 request_id 验证用户范围；测并发 Add
完成时间及 Search 延迟，验证消息原文、用户隔离、跨 session、未知用户、稳定
证据 ID、重复 Add、已发布数据的强退重启，以及停止宿主后复制完整数据目录并从副本恢复。报告包含实际二进制 SHA-256、模型、
维度、端点哈希、p50/p95/p99 和临时树清理结果。并发值是客户端请求数，宿主仍
只有一个 Worker。重启恢复计时包含启动、重复 Add 和检索验证。

本工具只验证小规模协议与持久性，不验证语义排序、持续容量、正式 Smoke 或 SLO。没有峰值内存和供应商 token/费用统计，磁盘值仅为测量时目录大小之和；每用户两个片段的完整召回不代表真实检索质量。
HTTP 观察超时不取消持久任务；测试停止会杀死自己启动的宿主，故报告不声称优雅
退出已验证。本演练工具的重启点在发布后。另有 tools/test_aml_recovery.py 在固定模型请求阻塞期间强退宿主，验证恢复原任务、没有部分发布或重复版本；设置 AML_TEST_BINARY 指向构建的宿主后执行上述 unittest 命令可运行。该用例不是原生存储写到一半的强杀或断电验证。

## 专用部署的进入条件

部署门槛见 [AML 计划 B1/B3](AGENT_MEMORY_LEADERBOARD.md#b3-冻结smokefull-与审核)。使用固定构建/模型、专用空数据卷和凭据，保持单宿主/单 Worker。网关等待覆盖 Add 的 25 分钟，或以相同 request_id 重试；ready 不代表模型正常。compose.yaml 仅为单实例起点，实际配置需目标服务器/域名、正式模型与容量预算，不代表部署已验收。

## 退役与清理执行顺序

1. 为每次正式评测维护副本清单：专用数据卷、SQLite WAL/SHM、LanceDB、graph.db、
   Blob、暂存、网关/宿主日志、模型缓存、备份、云快照、供应商保留设置及到期时间。
2. 停止该实例的新请求与备份计划，停止并确认其宿主、离线客户端和恢复任务结束。
   记录版本、停止时间和退出结果；不要通过删除锁文件强行获得访问权。
3. 将待删除目标解析为绝对路径，核对它确实是该次运行的专用根目录且不含业务库、
   仓库、历史公开评测工件或其他运行。逐项处置登记副本，使用平台原生删除接口或
   同一 shell 的 LiteralPath 操作，不拼接跨 shell 删除命令。
4. 验证目录/卷、快照及备份副本已不可读，撤销专用应用凭据并关闭恢复通道。
   只保存不含消息正文的副本 ID、操作结果、时间与检查结论。
5. 对无法控制的供应商保留或介质残留分别记录边界。逻辑删除、墓碑、目录不存在
   都不证明介质物理擦除；不得把本工具的临时目录清理扩写成生产数据销毁验收。

本地工具的清理范围为自己新建的临时目录及其中的测试备份，不创建外部备份或快照；本机模型服务的
缓存/日志属于独立保留边界。正式数据的 30 天删除期限及当期规则须在申请时重新核对。

## 带标签的检索质量评测

协议演练之外，运行 `python tools/aml_quality.py --binary target/debug/origence.exe --report target/aml-quality-new.json`，沿用上述模型环境。语料、指标和已运行结果见 [AML 本地质量基线](../evals/aml/README.md)。报告目标必须不存在；只使用内置合成开发数据。

## 查询扩展与答题验证

[实验流程与预算](../evals/aml/experiments/README.md)及[完整结果](../evals/aml/experiments/results/README.md)保留失败 pilot、真实网关调用、来源核对与 Answer 拒答指标。聊天凭据仅经环境传入，正式服务仍维持原文 vector。
