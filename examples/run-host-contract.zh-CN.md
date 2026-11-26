# 最小宿主 Run 适配器 — Issue #87

**简体中文** · [English](run-host-contract.md)

下文记录历史 stdio 验收。当前使用 [Streamable HTTP MCP](../docs/CLIENTS.md#直连-http-mcp)。

基线固定于 main `293e2bf6af3b180d43c8050731047b552ac07519`。当时的 Codex/Claude Code stdio 配置使用 `--server-url` 和继承的 `OC_API_KEY`。不要提交令牌。嵌入式本地 MCP 未实现 Run 工具，调用会明确提示需要 `--server-url`。既有归档工具在两种模式下都可用。

| MCP 工具 | 参数 | HTTP 映射 |
| --- | --- | --- |
| `context_run_create` | `run_id`、脱敏 `summary` | POST `/v1/runs` |
| `context_run_prepare` | `run_id`、`event_id`、`context`（既有 EvidenceInput） | POST `/v1/runs/{id}/context` |
| `context_run_report` | `run_id`、`report`（既有 RunEventInput） | POST `/v1/runs/{id}/events` |
| `context_run_get` | `run_id`、可选 `after` 序号及 `limit`（默认 50，最大 100） | GET `/v1/runs/{id}` |

HostClient 向非 MCP 调用方提供有类型的 `create_run`、`prepare_run_context`、`append_run_event` 和 `run_trace`。MCP 上报信封解码现有严格 RunEventInput，不新增事件或核验类型、存储字段。HTTP 错误沿用安全 MCP JSON-RPC 错误映射。用同一 UUID 和负载重试，改变负载返回冲突。轮换凭据返回新凭据 key ID 并保留逻辑所有权，撤销该替换 key ID 即撤销新凭据。不进行隐藏重试。传输失败结果未知，应读回或以原身份重试，不能推断未写入。恢复失败准备时使用新事件 ID。

`context_run_report` 参数示例（仅合成值）。

```json
{
  "run_id": "11111111-1111-4111-8111-111111111111",
  "report": {
    "event_id": "22222222-2222-4222-8222-222222222222",
    "reporter": "client",
    "observed_at_ms": 0,
    "event": {
      "kind": "coverage",
      "component": "external tools",
      "status": "unknown",
      "summary": "No host observation collected"
    }
  }
}
```

只上报实际观测。既有事件有 `context_reported`、`tool_reported`、`skill_loaded_reported`（精确 UUID、版本和 SHA256）、`artifact_reported`（不透明 UUID）、`result_reported`、`feedback` 和 `coverage`。上报不能证明执行、模型使用、交付或核验。来源撤回及精确版本检查仍由服务端负责。`context_prepared` 与 `context_preparation_failed` 仅服务端可生成，调用方上报会被拒。Feedback/Improve HTTP 操作及显式轮次归档不变。Run 中的反馈是 Run 声明，不会隐式调用学习或 Improve 流程。

## 可观测事件矩阵与待做的实际宿主验收

矩阵记录这些适配器的观测边界，不证明已有真实宿主执行。两端均须显式调用工具，无 Hook 抓取无关宿主活动。缺失的观测视为未知。

| 观测 | Claude Code 通过本适配器 | Codex 通过本适配器 |
| --- | --- | --- |
| 同逻辑 key 的归档 | 显式读取会话或轮次 | 显式读取会话或轮次 |
| 独立任务身份 | 用唯一 Run UUID 显式创建 | 用唯一 Run UUID 显式创建 |
| 服务端准备或安全失败 | 权威 HTTP 事件 | 权威 HTTP 事件 |
| 上下文被接收或模型使用 | 无独立上报为未知，上报只是声明 | 同左 |
| 外部工具调用或结果 | 显式 `tool_reported`，否则未知 | 同左 |
| 原生 Skill 加载 | 显式精确版本的 `skill_loaded_reported`，否则未知 | 同左 |
| 工件或最终任务结果 | 显式上报，不是独立核验 | 同左 |
| 覆盖范围 | 显式 unknown/partial/not_collected，缺失为未知 | 同左 |
| 此次的真实宿主、版本和任务证据 | 未采集（缺少 Claude 可执行文件） | 未采集（没有调用真实任务或模型） |

协议测试启动两个真实 Origence stdio 进程，接入本机回环 HTTP 宿主、新建临时 SQLite 和合成 AnySeek 协议夹具，模型关闭。测试对端**不是** Claude Code 或 Codex。真实双宿主任务、Windows 运行、冻结宿主版本和企业任务结果仍待验收，不能将夹具通过记为这些项目通过。

## 通过既有 HTTP 显式归档 → 反馈 → Improve

四个 Run 工具不替代轮次归档、轮次或证据反馈与 Improve。`context_turn_archive` / `context_turn_get` 仍为 MCP 归档入口。两种宿主可以用同一逻辑 key 访问以下现有 HTTP 路由，无需额外 MCP 工具或存储。Run `feedback` 仅为轨迹报告，不写会话反馈或触发 Improve。Helpful 反馈不代表经独立核验的业务结果（#89）。

在已授权、可销毁的宿主上运行合成示例，将 `OC_SERVER_URL` 与 `OC_API_KEY` 设置在环境中。给 `OC_EXAMPLE_TURN_ID` 设置一个新 UUID，重试时保留。示例显式归档所示合成问答及反馈。只在明确请求 Improve 时设置 `OC_EXAMPLE_IMPROVE=1`，此时需配置抽取模型。这项任务只可使用本地合成模型夹具。模型关闭时 Improve 返回 503，没有受理改进任务，不计成功。

```sh
export OC_SERVER_URL=http://127.0.0.1:8080
export OC_EXAMPLE_TURN_ID=33333333-3333-4333-8333-333333333333
# OC_API_KEY is supplied securely by your environment, not pasted into this file.
python3 - <<'PY'
import json, os, urllib.request, urllib.error, uuid
base = os.environ['OC_SERVER_URL'].rstrip('/')
turn = uuid.UUID(os.environ['OC_EXAMPLE_TURN_ID'])
def call(path, body=None, key=None):
    headers = {'Authorization': 'Bearer ' + os.environ['OC_API_KEY']}
    if body is not None:
        headers['Content-Type'] = 'application/json'
    if key:
        headers['Idempotency-Key'] = key
    req = urllib.request.Request(base + path, headers=headers,
        data=None if body is None else json.dumps(body).encode())
    # No redirects: never forward the credential to another origin.
    class NoRedirect(urllib.request.HTTPRedirectHandler):
        def redirect_request(self, *args, **kwargs):
            return None
    try:
        with urllib.request.build_opener(NoRedirect).open(req, timeout=60) as r:
            return json.load(r)
    except urllib.error.HTTPError as error:
        raise SystemExit(f'HTTP {error.code}; no success claimed; read back or retry identical IDs') from None
session = call('/v1/session')['session_id']
path = f'/v1/sessions/{session}/turns'
call(path, {'turn_id': str(turn), 'question': 'Synthetic example question',
    'answer': 'Synthetic example answer', 'used_evidence': []}, f'example-turn-{turn}')
feedback_id = str(uuid.uuid5(turn, 'feedback'))
feedback_path = f'{path}/{turn}/feedback'
call(feedback_path, {'feedback_id': feedback_id, 'target': {'type': 'turn'},
    'rating': 'helpful', 'comment': 'Synthetic explicit feedback'}, f'example-feedback-{turn}')
print('Archived turn and feedback readback:', call(feedback_path))
if os.environ.get('OC_EXAMPLE_IMPROVE') == '1':
    improve_id = str(uuid.uuid5(turn, 'improve'))
    print('Improve acceptance:', call(f'/v1/sessions/{session}/improve',
        {'improve_id': improve_id, 'conditions': ['synthetic example only'],
         'max_feedback': 1}, f'example-improve-{turn}'))
    print('Improve status:', call(f'/v1/improvements/{improve_id}'))
PY
```

用同一 `/v1/session` 查找 MCP 归档轮次的会话，用已有 `turn_id` 提交反馈，避免以不同内容再次归档。对归档证据评价时，使用服务端分配的 `evidence_ref_id` 和 `target.type="evidence"`。重试保留 ID 和完整负载，修改返回冲突。Improve 已受理不代表完成，须查状态与现有任务接口。未增加自动重试、本地会话采集或后台 Improve。来源撤回和当前所有权约束所有读取和重放。缺失用量或结果默认为**未知**，适配器不实现或认证 #89 的独立核验。
