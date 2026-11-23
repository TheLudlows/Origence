# Origence management UI

**English** · [简体中文](README.md)

Built with Node.js 24 and TypeScript, using native browser ES modules and DOM. There are no runtime npm dependencies. Node's built-in HTTP server serves static files and forwards a limited API over the same origin. Origence's web API still provides business logic, permissions, persistence, and the Worker.

## Startup

Configure AnySeek and start Origence per the [operations guide](../docs/OPERATIONS.en.md#anyseek-full-text-and-vector-retrieval), then run the following.

```sh
cd ui
npm ci --ignore-scripts
npm run build
cp .env.example .env
npm start
```

Open `http://127.0.0.1:3000` and enter the current user's Origence API key. The credential stays in page memory and is never written to localStorage, sessionStorage, the URL, build artifacts, or logs. Reloading requires reconnection. Sign-out or a 401 clears the page and its private data. Do not use a shared admin key to authorize all visitors.

`.env` configures the upstream HTTP origin, UI listen address, and port only. No business token is required. `npm start` reads the `.env` in this directory, and the deployment system may also inject environment variables. For external access, set the exact `ORIGENCE_UI_ORIGIN`, for example an internal HTTPS domain. The reverse proxy must preserve that Host and expose it over TLS. Only loopback Hosts are allowed by default. Upstream origins must not contain paths, usernames, or passwords. The proxy does not follow redirects.

After making development changes, run `npm run build`. `npm run dev` watches build output and restarts the service. The server also accepts `--host` and `--port` to work with existing preview tools.

## Pages

- **Overview** shows service readiness and counts and trends of Run creations for the current logical key. Uncollected latency and cost are shown as unknown.
- **Tracing** displays each Run's private Trace, including event order, reporters, preparation receipts, precise references, PDF page locators, and external verification records. The preparation event can show query embedding input tokens reported by the model service. Without a report it shows unknown. It does not represent the full Agent turn's usage or cost.
- **Memory** displays the visible asset catalog and supports pagination, filtering, search, explicit and semantic writes, text extraction, provenance, and fixed-version comparison.
- **Knowledge** supports Text/Markdown/PDF uploads, asynchronous publication, updates, job lookup, retry and cancellation, and authorized restore or withdrawal.
- **Context workbench** displays new query previews, source details, budgets, and omission counts. Budgets are always in UTF-8 bytes.

The catalog shows only currently published assets visible to normal reads. It does not list withdrawn or unpublished material. Search preview displays the relevant asset type from the workspace hybrid hits. It does not perform in-document search or represent the complete catalog. Historical views request an explicit version. After source withdrawal the server blocks access, and frontend caches cannot restore the content. Update semantic memory with a new semantic write. Generic restore does not apply. Reading original sources, files, and jobs requires writer or above. Asset deletion and source withdrawal also require admin.

## Code layout

```text
src/server.ts              固定上游、路径白名单、请求大小/超时、静态资源
src/client/api.ts          授权、取消、错误与身份失效
src/client/app.ts          应用框架、导航、登录与主题
src/client/assets.ts       记忆/知识目录与详情
src/client/asset-editor.ts 写入与提取表单，保持重试幂等身份
src/client/jobs.ts         有界任务轮询与操作
src/client/context.ts      上下文预览
src/client/runs.ts         运行与准备证据
src/client/source.ts       来源定位与固定版本差异
```

## Verification

```sh
npm run check
npm test
```

Tests cover actual HTTP forwarding, credential isolation, binary uploads, errors and timeouts, UTF-8 locators, and DOM workflows across all five pages. DOM tests do not verify browser layout. Run the real-backend test at the repository root. It uses no external model.

```sh
python3 tools/anyseek_smoke.py --anyseek-binary /path/to/anyseek_server \
  --run python3 tools/ui_smoke.py --binary target/debug/origence --ui-entry ui/dist/server.js
```

The command creates a temporary database and uses test credentials to verify the full Node UI → Origence → AnySeek HTTP path. See `docs/VALIDATION.md` for more results and unverified scope, and GitHub Issue #76 for the design.
