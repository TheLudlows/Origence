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

在 `ui` 中构建后，运行以下命令生成密码哈希。密码从标准输入读取，至少 15 个字符。

```bash
read -rsp 'Password: ' console_password
printf '%s\n' "$console_password" | node scripts/hash-console-password.mjs
unset console_password
```

PowerShell 可用 `Read-Host -MaskInput | node scripts/hash-console-password.mjs`。

通过已有的 [Origence 凭据管理接口](../docs/API.md#接口)，为每个账号在每个已授权工作区签发一把独立且权限适当的 key。用私有环境变量注入 key，把工作区 ID、显示名、环境变量名及密码哈希写进例如 `../.data/console-accounts.json` 的私有文件。每个账号可以多配几项工作区。

```json
{
  "accounts": [
    {
      "username": "alice",
      "password_hash": "<生成的 scrypt-v1 密码哈希>",
      "workspaces": [
        {
          "workspace_id": "11111111-1111-4111-8111-111111111111",
          "name": "研发",
          "key_env": "ORIGENCE_ALICE_ENGINEERING_KEY"
        }
      ]
    }
  ]
}
```

在 `.env` 设置 `ORIGENCE_UI_ACCOUNTS_FILE=../.data/console-accounts.json` 后启动控制台。配置由运营者维护，修改账号、工作区授权或轮换 key 后重启控制台即可生效。后台会核验每把 key 绑定的工作区和权限。账号有多个仍有效的工作区时，界面提供切换菜单。已撤销的 key 在下次请求及刷新时由上游拒绝。独立 key 的私有会话与 Run 继续按逻辑 key 隔离，如需延续自己的私有记录，应绑定该账号本人原有的同一逻辑 key。

控制台通过 8 小时有效的服务端内存会话恢复普通页面刷新。浏览器使用 HttpOnly、SameSite=Strict 的随机 Cookie，HTTPS 入口额外启用 Secure。同一标签页只在 sessionStorage 保存所选工作区 ID。主动退出、会话到期或 Node 服务重启后需重新登录。单工作区开发仍可在未配置账号文件时，用个人 API key 从 loopback 连接。

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
