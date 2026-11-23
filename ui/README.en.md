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

`npm start` reads the `.env` in this directory and accepts configuration from the deployment environment. For external access, set the exact HTTPS `ORIGENCE_UI_ORIGIN` and have the reverse proxy preserve that Host. Only loopback Hosts are allowed by default. The proxy takes an HTTP(S) origin for the upstream and does not follow redirects.

### Local accounts and workspaces

Build the UI, then generate a password hash from standard input. Use at least 15 characters.

```bash
read -rsp 'Password: ' console_password
printf '%s\n' "$console_password" | node scripts/hash-console-password.mjs
unset console_password
```

In PowerShell, use `Read-Host -MaskInput | node scripts/hash-console-password.mjs`.

Use the existing [Origence key management API](../docs/API.en.md#endpoints) to issue a separate key with the appropriate role for each account and workspace grant. Inject each key through a private environment variable. Store workspace IDs, labels, variable names, and password hashes in a private file such as `../.data/console-accounts.json`.

```json
{
  "accounts": [
    {
      "username": "alice",
      "password_hash": "<generated scrypt-v1 hash>",
      "workspaces": [
        {
          "workspace_id": "11111111-1111-4111-8111-111111111111",
          "name": "Engineering",
          "key_env": "ORIGENCE_ALICE_ENGINEERING_KEY"
        }
      ]
    }
  ]
}
```

Set `ORIGENCE_UI_ACCOUNTS_FILE=../.data/console-accounts.json` in `.env` before starting the console. Operators manage accounts and grants in the file. Restart the console to apply configuration or key rotation. The upstream checks each key's workspace and role. An account with multiple active grants can switch workspaces. Revoked keys are denied by the upstream on subsequent requests and refreshes. Private sessions and Runs retain logical-key isolation. To preserve access to an account holder's existing private records, grant their own original logical key.

An eight-hour in-memory console session survives page reloads. The browser receives a random HttpOnly, SameSite=Strict cookie. HTTPS also enables Secure. The tab stores only the selected workspace ID in sessionStorage. Sign-out, expiry, or Node restart requires login again. Without an account file, loopback development can still connect with a personal API key.

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
