# Origence management UI

**English** · [简体中文](README.md)

Built with Node.js 24 and TypeScript, using native browser ES modules and DOM. There are no runtime npm dependencies. Node's built-in HTTP server serves static files and forwards a limited API over the same origin. Origence's web API still provides business logic, permissions, persistence, and the Worker.

## Startup

Configure AnySeek and start Origence per the [operations guide](../docs/OPERATIONS.en.md#anyseek-full-text-and-vector-retrieval), then run the following.

```sh
cd ui
npm ci --ignore-scripts
npm run build
npm start
```

`npm start` uses built-in defaults. To override them, use the `.env` in this directory or deployment environment variables. For external access, set the exact HTTPS `ORIGENCE_UI_ORIGIN` and have the reverse proxy preserve that Host. Only loopback Hosts are allowed by default. The proxy takes an HTTP(S) origin for the upstream and does not follow redirects.

### Local accounts and workspaces

The console uses account and password sign-in by default. Its account database is `.data/console-accounts.db` in the repository root. Set `ORIGENCE_UI_ACCOUNTS_DB` only to use a different location. Until the first account is created, the sign-in page directs users to their administrator.

After building, run these commands from `ui` to set the first account's password and grant workspace access. `set-password` creates the private account database if needed. It reads the password from standard input. Use at least 15 characters.

```bash
read -rsp 'Password: ' console_password
printf '%s\n' "$console_password" | node scripts/console-accounts.mjs set-password alice
unset console_password
node scripts/console-accounts.mjs grant alice 11111111-1111-4111-8111-111111111111 "Engineering" ORIGENCE_ALICE_ENGINEERING_KEY
```

In PowerShell, pipe `Read-Host -MaskInput` into the same `set-password` command.

Use the [Origence key management API](../docs/API.en.md#endpoints) to issue a separate key with the appropriate role for each account and workspace grant. Inject each key into the console server environment, for example as `ORIGENCE_ALICE_ENGINEERING_KEY`. The account database stores scrypt password hashes and workspace grants. Run `set-password` again to change a password, `grant` to update a grant, `revoke USERNAME WORKSPACE_ID` to remove a grant, or `remove-account USERNAME` to remove an account and all its grants. If the console is running, restart it to load account changes or rotated keys.

After sign-in, the sidebar lists every workspace the account can access and highlights the current one. Select a workspace to switch to it. The list includes direct account grants and child workspaces accessible through a granted root. The upstream checks each key's workspace and role and denies revoked keys. Agents continue to use their own API tokens. Private sessions and Run writes require the owning logical key. To preserve access to an account holder's existing private records, grant their own original logical key.

An eight-hour in-memory console session survives page reloads. The browser receives a random HttpOnly, SameSite=Strict cookie. HTTPS also enables Secure. The tab stores only the selected workspace ID in sessionStorage. Sign-out, expiry, or Node restart requires login again.

### Create a child workspace

Sign in as a root workspace admin, select the root, and use **新建工作区** to enter a name. The console refreshes the list and switches to the new workspace, where the creator has writer access. Ordinary administrators access other children through explicit grants. Trusted root credentials with `allow_owned_workspaces` can access all immediate children. Operators initialize new root workspaces offline.

After making development changes, run `npm run build`. `npm run dev` watches build output and restarts the service. The server also accepts `--host` and `--port` to work with existing preview tools.

### API key management

Sign in as an administrator, select a workspace, and open **API Key** in the sidebar. The page lists
credentials issued there and lets administrators create them, change roles, rotate, or revoke them.
To grant access using a key issued in another workspace in the same tenant, enter its logical key ID
in the target workspace. Removing an additional grant removes access to that workspace. Rotation
immediately invalidates the old token and keeps the logical key ID and its grants.

Copy the new token from the creation or rotation dialog. To obtain another token after leaving the
dialog, rotate the credential again. Maintain the current console session's backing credential through
the existing administrator workflow. Rotating another key requires the caller's credential to have
administrator rights in that key's additional workspaces.

## Pages

- **Overview** shows service readiness and Run creation counts and trends. Admins can read the selected workspace's Run list, reported outcomes, counts, and full recorded traces. Other roles see their own Runs.
- **Tracing** displays each Run's private Trace, including event order, reporters, preparation receipts, precise references, PDF page locators, and external verification records. The preparation event can show query embedding input tokens reported by the model service. Without a report it shows unknown. It does not represent the full Agent turn's usage or cost.
- **Memory** displays the visible asset catalog and supports pagination, filtering, search, explicit and semantic writes, text extraction, provenance, and fixed-version comparison.
- **Knowledge** supports Text/Markdown/PDF uploads, asynchronous publication, updates, job lookup, retry and cancellation, and authorized restore or withdrawal.
- **Context workbench** displays new query previews, source details, budgets, and omission counts. Budgets are always in UTF-8 bytes.
- **Private archive** lets an owning writer view turns, delete one turn, or clear a fixed range. Unconfirmed deletions can be retried with the same range.

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

Tests cover actual HTTP forwarding, credential isolation, binary uploads, errors and timeouts, UTF-8 locators, and console DOM workflows. DOM tests do not verify browser layout. Run the real-backend test at the repository root. It uses no external model.

```sh
python3 tools/anyseek_smoke.py --anyseek-binary /path/to/anyseek_server \
  --run python3 tools/ui_smoke.py --binary target/debug/origence --ui-entry ui/dist/server.js
```

The command creates a temporary database and uses test credentials to verify the full Node UI → Origence → AnySeek HTTP path. See `docs/VALIDATION.md` for more results and unverified scope, and GitHub Issue #76 for the design.
