# #149 检索对照

在仓库根目录运行。原始基线、固定 BGE 下载和 Python 环境见[复跑报告](../results/local-6201881-v2/README.md)。对照使用相同的 36 篇文档、300 道题、k=5 和 2,000 UTF-8 字节预算。两家竞品共享同一批本地 BGE 向量及 Jieba 分词。结果包含逐题评分及 LanceDB 全词匹配对照。

```sh
env_dir=/tmp/origence-149-env
work=$(mktemp -d)
"$env_dir/bin/pip" install lancedb==0.41.0 jieba==0.42.1 pyarrow==26.0.1
tar -xzf evals/retrieval/results/local-6201881-v2/raw-results.tar.gz -C "$work"
qdrant=$(docker run --rm -d -p 127.0.0.1::6333 -e QDRANT__TELEMETRY_DISABLED=true \
  qdrant/qdrant@sha256:d277592721d2e3b73f9b797b53cf05e6a5bb2134f6d13fdbda4ca51782afbcac)
trap 'docker stop "$qdrant" >/dev/null' EXIT
port=$(docker port "$qdrant" 6333/tcp | cut -d: -f2)
curl --fail --retry 10 --retry-all-errors --silent "http://127.0.0.1:$port/readyz"
"$env_dir/bin/python" evals/compare_retrieval.py --qdrant-port "$port" \
  --embedding-path /tmp/origence-149-models/bge-small-zh-v1.5 \
  --baseline "$work/issue149-baseline" --output "$work/competition"
```

输出中的 `vectors.json` 可用 `--vectors` 复用。对照保留引擎原生排序，同分候选的名次可能随重建索引而变化。端到端延迟不在对照口径内。

## 词法定位

[AND → OR 实验提交](https://github.com/TheLudlows/Origence/commit/18aa3f70b98cc95eee8e87c11dfc2bc2e288619b)只有 Origence 的 AnySeek 请求运算符发生变化。用基线及实验提交分别构建 Linux 二进制，并在含本评测脚本的检出中对每件二进制执行同一命令。将 `test_cases` 指向新输出目录，`test_commit` 对应该二进制的源码提交。

```sh
python3 evals/run.py --local --mode keyword --components original --resolve-no-answer \
  --require-evidence --k 5 --budget 2000 --binary "$test_binary" --commit "$test_commit" \
  --corpus evals/retrieval/v2/corpus.jsonl --cases evals/retrieval/v2/cases.jsonl --output "$test_cases"
```

`terms.rs` 可用同一次 Cargo 构建的 `jieba-rs` 库打印中文词项。在只含该次构建产物的 `target/debug/deps` 上运行。

```sh
set -- target/debug/deps/libjieba_rs-*.rlib
test "$#" -eq 1 && test -f "$1"
rustc --edition 2024 evals/retrieval/diagnostics/terms.rs \
  -L dependency=target/debug/deps --extern "jieba_rs=$1" -o "$work/terms"
"$work/terms" '上线前谁来批准' '生产发布必须经过审批。'
```
