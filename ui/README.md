# Origence 管理界面

**简体中文** · [English](README.en.md)

Node.js 24 + TypeScript，使用浏览器原生 ES modules 和 DOM。
没有运行时 npm 依赖；Node 内置 HTTP 服务提供静态资源和受限的同源 API 转发。
业务、权限、持久化与 Worker 继续由 Origence Web API 提供。

## 启动

先按[操作文档](../docs/OPERATIONS.md#anyseek-全文与向量)配置 AnySeek 服务并启动 Origence，再执行：

```sh
cd ui
npm ci --ignore-scripts
npm run build
cp .env.example .env
npm start
```

`npm start` 读取本目录的 `.env`，也支持由部署系统注入环境变量。对外提供服务时将 `ORIGENCE_UI_ORIGIN` 设为准确的 HTTPS origin，并让反向代理保留该 Host。默认仅允许 loopback Host。上游 origin 只接受 HTTP(S) 域名及端口，且转发不跟随重定向。

### 本地账号与工作区

使用私有 SQLite 配置库，例如 `../.data/console-accounts.db`。其中 `oc_console_accounts` 保存用户名和 scrypt 密码哈希，`oc_console_workspace_grants` 保存账号对应的工作区 ID、显示名和后端 key 的环境变量名。配置库由 Node 控制台维护，独立于 Origence 的业务库 `context.db`。

构建后在 `ui` 目录执行以下命令。密码从标准输入读取，至少 15 个字符。

```bash
mkdir -p ../.data
node scripts/console-accounts.mjs --db ../.data/console-accounts.db init
read -rsp 'Password: ' console_password
printf '%s\n' "$console_password" | node scripts/console-accounts.mjs --db ../.data/console-accounts.db set-password alice
unset console_password
node scripts/console-accounts.mjs --db ../.data/console-accounts.db grant alice 11111111-1111-4111-8111-111111111111 "研发" ORIGENCE_ALICE_ENGINEERING_KEY
```

PowerShell 可以把 `Read-Host -MaskInput` 的输出传给同一个 `set-password` 命令。

使用已有的 [Origence 凭据管理接口](../docs/API.md#接口)，为每个账号及工作区授权签发一把独立且权限适当的 key，并由服务端环境注入，如 `ORIGENCE_ALICE_ENGINEERING_KEY`。在 `.env` 设置 `ORIGENCE_UI_ACCOUNTS_DB=../.data/console-accounts.db` 后启动控制台。再次运行 `set-password` 可改密，`grant` 可更新已有授权，`revoke USERNAME WORKSPACE_ID` 可撤销一项授权，`remove-account USERNAME` 可删除账号及其授权。重启控制台应用配置或 key 轮换。

用户用账号登录后可切换获授权的工作区。后台核验每把 key 的工作区和权限，上游会拒绝已撤销的 key。Agent 继续用自己的 API token 接入。私有会话和 Run 按逻辑 key 隔离。要延续自己的私有记录，应绑定该账号本人原有的同一逻辑 key。

控制台通过 8 小时有效的服务端内存会话恢复普通页面刷新。浏览器使用 HttpOnly、SameSite=Strict 的随机 Cookie，HTTPS 入口额外启用 Secure。同一标签页只在 sessionStorage 保存所选工作区 ID。主动退出、会话到期或 Node 服务重启后需重新登录。单工作区开发仍可在未配置账号数据库时，用个人 API key 从 loopback 连接。

### 新建子工作区

使用当前工作区的管理员凭据执行 `origence key-create <工作区 ID> --role admin --allow-owned-workspaces`，将得到的 key 作为受信控制台服务凭据配置给获授权的本地账号。该授权涵盖根工作区及其所有直接子工作区。

登录后，在有此授权的根工作区点击 **新建工作区**，填写名称并提交。成功后，控制台更新列表并切换至新工作区。在子工作区内，该凭据具有 writer 权限，创建入口显示在有创建授权的根工作区。新的根工作区仍由操作员离线初始化。

开发时修改源代码后运行 `npm run build`；`npm run dev` 会监听编译产物并重启服务。
服务也支持 `--host`、`--port` 参数，便于接入已有预览工具。

## 页面

- **运行总览**显示服务就绪状态、当前逻辑 key 的 Run 创建量与趋势。未采集的延迟和成本显示为未知。
- **调用链路**按 Run 查看私有 Trace，包括事件顺序、报告方、准备回执、精确引用、PDF 页内定位和外部核验记录。可在准备事件查看模型服务上报的查询向量输入 Token；没有可用上报时显示未知，不代表整轮 Agent 用量或成本。
- **记忆管理**：可见记忆目录、按页筛选、检索、显式/语义写入、文本提取、来源与固定版本比较。
- **知识管理**：Text/Markdown/PDF 上传、异步发布、更新、任务查询/重试/取消及获准的恢复/撤回。
- **上下文工作台**：新查询预览、来源明细、预算与省略计数；预算单位始终为 UTF-8 字节。

目录只显示当前已发布且有权读取的记忆与知识；不是撤回/未发布内容的管理清单。
检索预览展示 工作区检索命中中与当前页面对应的记忆或知识结果，不冒充文档内检索或完整目录。
历史查看使用显式版本；来源撤回后由服务端阻断，不用前端缓存复活历史内容。
语义记忆通过新的语义写入修订，不能使用通用 restore。
原始来源/文件与任务读取按 writer 以上权限提供；删除记忆或知识、来源撤回还需要 admin。

## 代码结构

```text
src/server.ts              固定上游、路径白名单、请求大小/超时、静态资源
src/console-accounts.ts    本地账号配置与密码哈希
src/console-session.ts     控制台会话与工作区授权转发
src/client/api.ts          授权、取消、错误与身份失效
src/client/app.ts          应用框架、导航、登录与主题
src/client/assets.ts       记忆/知识目录与详情
src/client/asset-editor.ts 写入与提取表单，保持重试幂等身份
src/client/jobs.ts         有界任务轮询与操作
src/client/context.ts      上下文预览
src/client/runs.ts         运行与准备证据
src/client/source.ts       来源定位与固定版本差异
```

## 验证

```sh
npm run check
npm test
```

测试包括实际 HTTP 转发、凭据隔离、二进制上传、错误/超时、UTF-8 定位及五页 DOM 流程。
DOM 测试不验证浏览器排版。真实后端测试从仓库根目录执行，不调用外部模型：

```sh
python3 tools/anyseek_smoke.py --anyseek-binary /path/to/anyseek_server \
  --run python3 tools/ui_smoke.py --binary target/debug/origence --ui-entry ui/dist/server.js
```

该命令使用新建临时库与测试凭据，验证 Node UI → Origence → AnySeek 的完整 HTTP 路径。
更多结果与未验证范围见 `docs/VALIDATION.md`，设计见 GitHub Issue #76。
