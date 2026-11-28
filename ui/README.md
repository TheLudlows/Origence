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
npm start
```

`npm start` 使用内置默认值。需要覆盖时，可使用本目录的 `.env` 或部署环境变量。对外提供服务时将 `ORIGENCE_UI_ORIGIN` 设为准确的 HTTPS origin，并让反向代理保留该 Host。默认仅允许 loopback Host。上游 origin 只接受 HTTP(S) 域名及端口，且转发不跟随重定向。

### 工作空间 Key 登录

宿主停机时运行 `origence --offline workspace-create --workspace-key <名称>` 创建空间，然后启动 API 和控制台。用签发的 Key 登录控制台或配置 Agent。控制台打开该 Key 对应的空间，支持读取归档问答和管理记忆。轮换使用 `origence --offline workspace-key-rotate <工作空间 ID>`，再用新 Key 登录。

已配置控制台账号时，登录页默认显示账号密码。点击表单下方的「用 Key 登录」可改用工作空间 Key 或部署管理员 Key。未配置账号时，页面直接显示 Key 输入框。

`origence-deploy install --init <名称>` 启动的控制台仅使用 Key 登录，无需创建账号库。手动启动时可用 `npm start -- --key-only`。

### 部署管理员

宿主停机时运行 `origence --offline deployment-admin-rotate` 签发或轮换部署管理员 Key。用该 Key 从控制台的「访问 Key」入口登录，选择任意工作空间，便可查看其原始会话、Run、已发布记忆和知识。轮换后旧 Key 及其登录会话失效。

### 全局知识

打开自己工作空间的知识详情，在指定版本点击「提交到全局知识审核」。部署管理员通过「全局知识审核」查看待审正文，批准、驳回或下架。已批准版本可从「全局知识」打开，也会出现在知识检索和上下文中。共享副本保留提交时的版本。后续修改原知识时，可提交新的版本审核。

### 现有账号授权入口

账号库默认位于仓库根目录的 `.data/console-accounts.db`，可用 `ORIGENCE_UI_ACCOUNTS_DB` 指定其他位置。配置账号后，控制台显示账号密码入口。

构建后在 `ui` 目录设置第一个账号的密码和工作区授权。`set-password` 会按需创建私有账号库。密码从标准输入读取，至少 15 个字符。

```bash
read -rsp 'Password: ' console_password
printf '%s\n' "$console_password" | node scripts/console-accounts.mjs set-password alice
unset console_password
node scripts/console-accounts.mjs grant alice 11111111-1111-4111-8111-111111111111 "研发" ORIGENCE_ALICE_ENGINEERING_KEY
```

PowerShell 可以把 `Read-Host -MaskInput` 的输出传给同一个 `set-password` 命令。

使用已有的 [Origence 凭据管理接口](../docs/API.md#接口)，为每个账号及工作区授权签发一把独立且权限适当的 key，并由服务端环境注入，如 `ORIGENCE_ALICE_ENGINEERING_KEY`。账号库保存 scrypt 密码哈希和工作区授权，后端 key 仅由服务端读取。再次运行 `set-password` 可改密，`grant` 可更新已有授权，`revoke USERNAME WORKSPACE_ID` 可撤销一项授权，`remove-account USERNAME` 可删除账号及其授权。若控制台已启动，重启后加载账号改动或轮换后的 key。

用户用账号登录后，侧栏直接列出该账号可以访问的全部工作区，标出当前项，点击即可切换。列表包含账号直接获授权的工作区及通过根工作区授权可访问的子工作区。后台核验每把 key 的工作区和权限，上游会拒绝已撤销的 key。Agent 继续用自己的 API token 接入。私有会话和 Run 写入按逻辑 Key 授权。要接续该账号已有的私有记录，应绑定原有逻辑 Key。

控制台通过 8 小时有效的服务端内存会话恢复普通页面刷新。浏览器使用 HttpOnly、SameSite=Strict 的随机 Cookie，HTTPS 入口额外启用 Secure。同一标签页只在 sessionStorage 保存所选工作区 ID。主动退出、会话到期或 Node 服务重启后需重新登录。

### API Key 管理

在现有成员授权工作区中，用管理员账号登录并选择工作区后，在侧栏进入 **API Key**。可查看本区签发的凭据、创建与调整权限、
轮换或撤销。为同租户的其他工作区签发的 Key 添加权限时，在目标工作区输入其逻辑 Key ID。
移除额外授权只撤销当前工作区的访问。轮换会立即使旧 token 失效，保留逻辑 Key ID 和原授权。

创建和轮换后请在结果弹窗复制 token。关闭弹窗或离开页面后需要再次轮换才能取得新的 token。
当前控制台会话所用凭据通过原有管理员流程维护。轮换其他 Key 时，当前凭据还须在该 Key 的额外授权工作区有管理员权限。

### 新建子工作区

登录后，根工作区的管理员可点击 **新建工作区**，填写名称并提交。成功后，控制台更新列表并切换到新工作区，创建者拥有该子工作区的 writer 权限。普通管理员通过显式授权访问其他子工作区。带有 `allow_owned_workspaces` 的受信根凭据可访问其所有直接子工作区。新的根工作区由操作员离线初始化。

开发时修改源代码后运行 `npm run build`；`npm run dev` 会监听编译产物并重启服务。
服务也支持 `--host`、`--port` 参数，便于接入已有预览工具。

## 页面

- **运行总览**显示服务就绪状态、Run 创建量与趋势。管理员查看当前工作区的 Run 列表、上报状态、统计和链路详情。其他角色查看当前 Key 的运行。
- **调用链路**按 Run 查看私有 Trace，包括事件顺序、报告方、准备回执、精确引用、PDF 页内定位和外部核验记录。可在准备事件查看模型服务上报的查询向量输入 Token；没有可用上报时显示未知，不代表整轮 Agent 用量或成本。
- **记忆管理**：可见记忆目录、按页筛选、检索、显式/语义写入、文本提取、来源与固定版本比较。
- **知识管理**：Text/Markdown/PDF 上传、异步发布、更新、任务查询/重试/取消及获准的恢复/撤回。
- **上下文工作台**：新查询预览、来源明细、预算与省略计数；预算单位始终为 UTF-8 字节。
- **私有归档**：具备写入权限的所属 Key 可查看问答、删除单轮或清空固定范围。删除未确认时可重试原范围。

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

测试包括实际 HTTP 转发、凭据隔离、二进制上传、错误/超时、UTF-8 定位及控制台 DOM 流程。
DOM 测试不验证浏览器排版。真实后端测试从仓库根目录执行，不调用外部模型：

```sh
python3 tools/anyseek_smoke.py --anyseek-binary /path/to/anyseek_server \
  --run python3 tools/ui_smoke.py --binary target/debug/origence --ui-entry ui/dist/server.js
```

该命令使用新建临时库与测试凭据，验证 Node UI → Origence → AnySeek 的完整 HTTP 路径。
更多结果与未验证范围见 `docs/VALIDATION.md`，设计见 GitHub Issue #76。
