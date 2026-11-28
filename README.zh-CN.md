# Origence

**简体中文** · [English](README.md)

**智能体统一数据底座。**

Origence 管理记忆、知识、Skill 和运行轨迹，提供 HTTP、MCP 接口和管理界面。首轮试点面向使用 Claude Code 和 Codex 的内部工程和业务团队。

## 核心数据

- **记忆**按作用域记录事实、偏好和决策，并保留版本和来源引用。
- **知识**管理文档及其已发布版本，支持来源引用和撤回。
- **技能（Skill）**将可复用方法整理为私有候选，经过对精确版本的人工审批后不可变地发布，并提供已批准包的在线下载。
- **运行轨迹**保存私有 Run 记录，包括服务端准备的上下文、客户端上报的 Skill 加载、工具与结果、外部评估记录和采集缺口。

工作区成员可以按权限复用已发布的记忆、知识和技能。

上下文准备为任务选择授权范围内的证据。AnySeek 提供全文、向量和一跳图检索，Origence 核验授权与来源。
模型推理、任务编排和业务工具执行由智能体宿主负责。[当前产品边界](docs/design/current-implementation.md#1-产品边界)

## 快速开始

[安装指南](docs/INSTALLER.md)分别说明 Claude Code / Codex 接入和本地控制台部署。

需要 Rust **1.98.0**。默认构建链接 AnySeek Rust SDK，用于本地存储。快速开始请使用全新的数据目录。

```sh
cargo build --locked
export OC_DATA_DIR="$(mktemp -d)"
./target/debug/origence serve --local
```

首次启动会在 `$OC_DATA_DIR/deployment-admin.key` 写入随机部署管理员 Key。用它登录控制台并创建工作空间，取得工作空间 Key 后即可接入 Agent。嵌入式 AnySeek Catalog 位于 `$OC_DATA_DIR/anyseek`。

## 云上部署

Docker 构建使用远端 AnySeek 客户端。用 `OC_ANYSEEK_URL` 和 `OC_ANYSEEK_API_KEY` 配置已部署的服务。用容器默认命令启动，或在云端主机执行 `origence serve`。[云上配置](docs/OPERATIONS.md#云上部署与独立-anyseek)

## 管理界面

轻量级 [Node.js 管理界面](ui/README.md)通过 Origence HTTP API 访问记忆、知识、上下文预览和私有 Run 证据。需要 Node.js 24。

```sh
cd ui
npm ci --ignore-scripts
npm run build
npm start
```

打开 `http://127.0.0.1:3000`，使用[工作空间 Key](ui/README.md#工作空间-key-登录)或[现有控制台账号](ui/README.md#现有账号授权入口)登录。Origence 地址与 `http://127.0.0.1:8080` 不同时，设置 `ORIGENCE_API_BASE_URL`。

## Rust 库与存储

SQLite 保存权限、版本化事实、来源、持久任务和全局知识副本。AnySeek 保存工作空间已发布的分块正文、摘要和检索索引，并在独立私有集合保存完整问答及轮次目录。Blob 保存上传的原始文件。[存储分工](docs/design/current-implementation.md#storage-design)

库调用方可使用解析、身份和值契约、模型、Blob 存储和 AnySeek 适配器，无需元数据引擎。

```sh
cargo test --no-default-features
```

单独使用权威元数据适配器可启用 `sqlite`，构建仅连接远端服务的宿主可启用 `local-storage`，默认原生构建通过 `embedded-anyseek` 启用本地 SDK。空读取集跳过元数据查询。宿主事务在入口和提交时分别核验所需权限。

## 文档

[产品目标与路线图](docs/ROADMAP.md) · [文档入口](docs/README.md) · [API 与库接口](docs/API.md) · [运维](docs/OPERATIONS.md) · [项目进度](docs/STATUS.md)

会话记录、Run 轨迹和未发布候选归属逻辑 API Key。已发布的记忆、知识和 Skill 归属工作空间，由获授权成员访问。见[架构](docs/design/agent-data-foundation.md)。

选定经验 CLI 将拟定的 Skill 方法绑定到一个精确来源，例如 Run 前缀或已发布的 Learning 版本。真实客户端试点验收尚未完成。见 [CLI 指南](docs/SKILL-CANDIDATE-CLI.zh-CN.md)、[客户端配置](docs/CLIENTS.md)和[试点验收议题](https://github.com/TheLudlows/Origence/issues/91)。

评估入口包括[检索](evals/retrieval/README.zh-CN.md)和 [Agent 对照](evals/agent/README.zh-CN.md)。

贡献者与编码智能体的实现规则见 [AGENTS.md](AGENTS.md)。SQL 禁用 JOIN 的约束继续生效，原自动检查和测试已移除。

Windows 原生开发（无需 WSL）见[本地启动和可选 UI 启动器](docs/WINDOWS-DEV.md)。
