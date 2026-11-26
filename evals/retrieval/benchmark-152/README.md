# #152 中文文档检索

S1 v2 冻结的 36 篇中文合成文档和 300 道题。开发集 200 题（192 可答、8 无答案），留出集 100 题（96 可答、4 无答案）。两个子集检索相同的 36 篇文档。运行前固定三方配置，未按题目或金标调参。语料来源、标签和四个数据文件的 SHA-256 见[冻结清单](../v2/manifest.json)。逐题记录含来源、原生返回的文本、预算内保留文本、状态和延迟。解压 `*.rows.jsonl.gz` 可查看。该批合成标签未独立人工审阅。

- Origence `13e2a4cb7bea7e97f67525d138453b6f9a0fdba1`，嵌入式 AnySeek `5626dadfd5414b294a3cf0f6b845f5261760f68b`。使用 `/v1/knowledge`、发布 Job 和 `/v1/search`，混合检索，关闭摘要、图和降级。#154 后合入的显式 OR 不包含在冻结版本中。
- Hindsight v1.3.0 `937886199e0e3443f41f4d8180a16b12cc95c547`，standalone slim（内置 PostgreSQL/pgvector）。原生 Knowledge Bank 同步导入和 search，`budget=high`、`max_tokens=12000`、查询重排关闭。
- cognee v1.6.10 `c8a094375821f2d8c7c4945d3b2202e2e5d6603e`，SQLite 元数据、LanceDB 0.39.0 向量、Turso 0.7.2 图存储。调用 `cognee.add` 和原生预索引任务，然后调用 `cognee.search(CHUNKS)`。此次不运行图提取。选用本地 Turso 是因为环境不允许 Ladybug 向其默认扩展路径写入。
- 三方用同一组本地 BGE 权重，模型修订 `7999e1d3359715c523056ef9478215996d62a620`，512 维 CLS 池化及 L2 归一化，未加查询指令。模型各文件哈希见摘要。生成模型调用和外部 API 费用为零，未估算 CPU 成本。

按原生排名首次出现顺序，以来源和版本去重，取前 5 个。Origence 提供原生版本号。Hindsight 和 cognee 用导入时的文档 ID 和文件名关联到合成语料的唯一版本 `1`。保留原样返回文本，统一用完整片段和来源头拼装，UTF-8 上限为 2,000 字节。可答题 Recall@5 和逐字引文覆盖分别按题宏平均，失败的可答题计 0。无答案题返回任一来源算误召回，失败另计。p50/p95 按成功查询的最近秩计算，包含本地 embedding 和各自调用边界，排除入库与评分。Origence、Hindsight 走 loopback HTTP，cognee 走进程内 Python SDK。单轮串行 CPU，Origence 为 debug 构建。延迟只适用于此环境。

## 结果

| 系统 | 划分 | 来源 Recall@5 | 逐字引文覆盖 | 无答案误召回 | p50 / p95（ms） | 失败 / 超时 |
| --- | --- | ---: | ---: | ---: | ---: | ---: |
| Origence | 全部 | 97.57%（281/288） | 97.57% | 12/12 | 216.7 / 326.9 | 0 / 0 |
| Origence | 开发 | 96.35%（185/192） | 96.35% | 8/8 | 216.9 / 340.4 | 0 / 0 |
| Origence | 留出 | 100.00%（96/96） | 100.00% | 4/4 | 216.2 / 259.5 | 0 / 0 |
| Hindsight | 全部 | 96.88%（279/288） | 96.88% | 12/12 | 28.7 / 32.8 | 0 / 0 |
| Hindsight | 开发 | 95.83%（184/192） | 95.83% | 8/8 | 27.1 / 31.5 | 0 / 0 |
| Hindsight | 留出 | 98.96%（95/96） | 98.96% | 4/4 | 29.7 / 38.3 | 0 / 0 |
| cognee | 全部 | 97.57%（281/288） | 97.57% | 12/12 | 146.1 / 166.6 | 0 / 0 |
| cognee | 开发 | 96.35%（185/192） | 96.35% | 8/8 | 147.7 / 166.6 | 0 / 0 |
| cognee | 留出 | 100.00%（96/96） | 100.00% | 4/4 | 144.2 / 160.0 | 0 / 0 |

Origence 和 cognee 的 300 题 Top 5 来源、顺序及原文全部一致。两者相对 Hindsight 共多召回 2 道可答题，约 0.69 个百分点。三方 12/12 道无答案题均返回了候选和预算内文本。比分和延迟只反映此冻结语料与配置。

| 系统 | Embedding HTTP 请求 | BGE 输入 token |
| --- | ---: | ---: |
| Origence | 336 | 7,082 |
| Hindsight | 303 | 10,402 |
| cognee | 373 | 7,325 |

用量含入库、探测和查询。三方合计完成 900 次检索，0 失败、0 超时。逐题独立复算了来源、版本、文本、预算、评分及延迟分位数。保留 [摘要和逐题文件校验值](sha256.txt)。既有 `evals` Python 单测通过 44 项。

## 复跑

以下在 Linux x86-64 执行。设 `CODE` 为冻结提交的干净 Origence checkout，`RUNNER` 为本目录 `run.py` 的绝对路径，`WEIGHTS` 为精确 BGE 快照的绝对路径。在新的工作目录运行，使用互不冲突的本地端口。先按 [S1 v2 依赖说明](../v2/README.zh-CN.md#复现)在独立环境安装模型依赖，这里只需 embedding 权重。

```bash
git -C "$CODE" checkout --detach 13e2a4cb7bea7e97f67525d138453b6f9a0fdba1
(cd "$CODE" && CARGO_PROFILE_DEV_DEBUG=0 CARGO_INCREMENTAL=0 cargo build --locked -j 2 --bin origence)
export EMBED=http://127.0.0.1:45673/v1
HF_HUB_OFFLINE=1 TRANSFORMERS_OFFLINE=1 HF_HUB_DISABLE_IMPLICIT_TOKEN=1 \
  /path/to/model-env/bin/python "$CODE/evals/local_models.py" \
  --embedding-path "$WEIGHTS" --embedding-model BAAI/bge-small-zh-v1.5 \
  --embedding-revision 7999e1d3359715c523056ef9478215996d62a620 \
  --port 45673 --cache "$PWD/model-cache" --metadata "$PWD/model.json"
```

模型服务保持运行。另开终端并复用上述变量，按顺序执行以隔离模型计数。

```bash
python3 "$RUNNER" origence --repo "$CODE" --binary "$CODE/target/debug/origence" \
  --model-url "$EMBED" --output origence-docs

docker run --rm -d --name issue152-hindsight --network host \
  -e HINDSIGHT_ENABLE_CP=false -e HINDSIGHT_API_HOST=127.0.0.1 -e HINDSIGHT_API_PORT=48352 \
  -e HINDSIGHT_API_DATABASE_URL=pg0://issue152:48353 \
  -e HINDSIGHT_API_LLM_PROVIDER=openai -e HINDSIGHT_API_LLM_API_KEY=local-fixture \
  -e HINDSIGHT_API_LLM_BASE_URL="$EMBED" -e HINDSIGHT_API_LLM_MODEL=unused-for-docs \
  -e HINDSIGHT_API_EMBEDDINGS_PROVIDER=openai \
  -e HINDSIGHT_API_EMBEDDINGS_OPENAI_MODEL=BAAI/bge-small-zh-v1.5 \
  -e HINDSIGHT_API_EMBEDDINGS_OPENAI_DIMENSIONS=512 \
  -e HINDSIGHT_API_EMBEDDINGS_OPENAI_BASE_URL="$EMBED" \
  -e HINDSIGHT_API_RERANKER_PROVIDER=rrf -e HINDSIGHT_API_KB_ENABLE_RERANKING=false \
  -e HINDSIGHT_API_KB_EMBEDDINGS_PROVIDER=openai \
  -e HINDSIGHT_API_KB_EMBEDDINGS_API_KEY=local-fixture \
  -e HINDSIGHT_API_KB_EMBEDDINGS_MODEL=BAAI/bge-small-zh-v1.5 \
  -e HINDSIGHT_API_KB_EMBEDDINGS_BASE_URL="$EMBED" \
  -e HINDSIGHT_API_KB_EMBEDDINGS_DIMENSIONS=512 -e HINDSIGHT_API_KB_EMBEDDINGS_MODALITIES=text \
  -e DO_NOT_TRACK=1 \
  ghcr.io/vectorize-io/hindsight@sha256:d311bdee313f4be08d7f33ab7070ace35ebdec27985a6678acfc6ad186515b44
# /health 返回 healthy 后执行。每次复跑使用新容器，结束后 docker stop issue152-hindsight。
python3 "$RUNNER" hindsight --repo "$CODE" --model-url "$EMBED" \
  --hindsight-url http://127.0.0.1:48352 --output hindsight-docs
```

```bash
python3 -m venv cognee-env
cognee-env/bin/pip install 'cognee[turso]==1.6.10' 'lancedb==0.39.0' 'pyturso==0.7.2' \
  'openai==3.60.0' 'tiktoken==0.14.0' 'tokenizers==0.22.2'
export HF_HOME="$PWD/cognee-hf"
revision=7999e1d3359715c523056ef9478215996d62a620
cache="$HF_HOME/hub/models--BAAI--bge-small-zh-v1.5"
mkdir -p "$cache/refs" "$cache/snapshots/$revision"
printf '%s' "$revision" > "$cache/refs/main"
cp "$WEIGHTS/tokenizer.json" "$WEIGHTS/tokenizer_config.json" "$cache/snapshots/$revision/"
export HF_HUB_OFFLINE=1
# cl100k_base 词表为本地运行依赖，首次预取并由 tiktoken 校验。
export TIKTOKEN_CACHE_DIR="$PWD/tiktoken-cache"
cognee-env/bin/python -c 'import tiktoken; tiktoken.get_encoding("cl100k_base")'
export DATA_ROOT_DIRECTORY="$PWD/cognee-state/data"
export SYSTEM_ROOT_DIRECTORY="$PWD/cognee-state/system"
export CACHE_ROOT_DIRECTORY="$PWD/cognee-state/cache"
export COGNEE_LOGS_DIR="$PWD/cognee-state/logs"
export GRAPH_DATABASE_PROVIDER=turso
export COGNEE_TRACING_ENABLED=false TELEMETRY_DISABLED=true DO_NOT_TRACK=1
export LITELLM_LOCAL_MODEL_COST_MAP=True
export EMBEDDING_PROVIDER=openai_compatible EMBEDDING_MODEL=BAAI/bge-small-zh-v1.5
export EMBEDDING_API_KEY=local-fixture EMBEDDING_DIMENSIONS=512
export EMBEDDING_MAX_COMPLETION_TOKENS=510 EMBEDDING_ENCODING_FORMAT=float
export EMBEDDING_ENDPOINT="$EMBED" LLM_ENDPOINT="$EMBED"
export LLM_MODEL=openai/unused-for-docs LLM_API_KEY=local-fixture
cognee-env/bin/python "$RUNNER" cognee --repo "$CODE" --model-url "$EMBED" --output cognee-docs
```
