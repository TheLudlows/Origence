# 受管 Run 上报

**简体中文** · [English](MANAGED-RUNS.en.md)

应用直接驱动 Codex App Server 或 Claude Agent SDK 时，可把命令完成信息或取消过程记录到已有的私有 Run。需要 Node.js 24、Origence 工作区的写权限和该 Run 的所有权。在适配器进程设置 `OC_API_KEY`、`OC_WORKSPACE_ID` 和 `OC_SERVER_URL`（默认 `http://127.0.0.1:8080`）。远程地址使用 HTTPS。先用现有 HTTP `POST /v1/runs` 或 MCP `context_run_create` 创建 Run，再向适配器传入该 Run ID。

## Codex App Server

将下列函数接到已有 App Server 客户端的通知读取处。使用 `turn/start` 的真实回执绑定 thread 和 turn。在绑定时取一次观测时间；在重放同一批数据时使用原值。

```js
import { openRunReporter } from "./tools/managed-run/report.mjs";
import { codexCommandRecorder } from "./tools/managed-run/codex.mjs";

const report = await openRunReporter({ runId });
const record = codexCommandRecorder({
  threadId, turnId, observedAtMs: Date.now(), report,
});
for await (const notification of appServerNotifications) {
  await record(notification);
}
```

也可把该应用收到的 JSONL 通知送入独立适配器的 stdin。每条最多 4 MiB。适配器只处理绑定轮次的 `item/completed` 中的 `commandExecution`，追加 `codex_command_reported`，将命令状态和实际存在的整数退出码、耗时显示在 Run 的调用链路里。缺字段显示「未知」，显式零保留。命令文本、目录、参数、输出和外部事件 ID 不进入 Run。该适配器不需要安装 SDK。

```sh
node tools/managed-run/agent.mjs codex --run-id "$RUN_ID" \
  --thread-id "$THREAD_ID" --turn-id "$TURN_ID" --observed-at-ms "$OBSERVED_AT_MS"
```

## Claude Agent SDK

在 `tools/managed-run` 执行 `npm ci` 安装锁定的 Claude Agent SDK 0.3.332。应用可用 `runClaudeTurn({ query, prompt, report, cancelSignal, onMessage })` 启动一个独立提示，把自身的 `AbortController.signal` 传入。由应用调用 `abort()` 时，适配器调用 SDK 的 `interrupt()`。`onMessage` 供应用本地显示 SDK 消息。命令入口从 stdin 读取一个提示（最多 1 MiB），可向该适配器进程发送 SIGINT 请求取消。

```sh
node tools/managed-run/agent.mjs claude --run-id "$RUN_ID" < prompt.txt
```

时间线分开显示已发起取消、SDK 回执及随后得到的结果。仅在 SDK 返回有效回执、目标提示已经不在待处理队列，同时结果回显同一个提示 UUID 且明确显示流式输出或工具执行被中断时，才标为「确认中断」。普通完成、其他终止原因和未知分别显示。缺回执、旧版 SDK 缺字段、只匹配到其他提示或超时均保留相应缺口。取消后最多等 10 秒再中止 SDK 查询流。SDK 子进程环境排除 Origence 的 `OC_*` 变量。提示正文、回复、队列中其他提示的 ID 和 SDK 错误原文都不上传。

## Run 语义

上报通过现有私有 Run API 完成，重试沿用同一事件 ID 和载荷，每次请求失败最多重试两次。一次启动给 Claude 提示生成新 UUID，防止把重复执行混为同一轮取消。两类事件均为执行方上报，业务结果继续由现有结果上报或独立核验证据决定。普通终端会话仍按原有 [Hook 事件边界](CLIENTS.md#事件和关联边界) 采集。

[Codex App Server](https://learn.chatgpt.com/docs/app-server) · [Claude Agent SDK 回执](https://code.claude.com/docs/en/agent-sdk/typescript#sdkcontrolinterruptresponse)
