# 公开合成样例预检

**简体中文** · [English](PREFLIGHT.en.md)

从仓库根目录运行，使用 Python 3.11 或更高版本、已安装并获授权的两个客户端、新建的测试 workspace 和尚不存在的报告文件。以下命令会调用真实模型。Codex 使用已有的 OpenAI 登录；Claude Code 从环境或可选的 `--claude-settings PATH` 读取供应商配置。

## 复跑原生 Skill 加载

```sh
python tools/anyseek_smoke.py --anyseek-binary ANYSEEK_BINARY --run python evals/pilot/live_skill_preflight.py --binary ORIGENCE_BINARY --claude-model CLAUDE_MODEL --codex-model CODEX_MODEL --report NEW_REPORT.json
```

两个客户端共享 100 秒，各自导出同一获批 Skill 并返回包内随机串。脚本核验原生加载信号与 Run，再撤回支持来源，核对新导出被拒和原有文件残留。必要检查缺失时返回非零状态。[实测记录](native-skill.windows.json)

## 复跑双场景跨客户端交接

把上述命令中的脚本替换为 `evals/pilot/public_handoff.py`，使用另一份新报告。两个方向各用独立新库，并行执行，四个会话共享 85 秒。首做者提交决策表，测试账户审阅发布；复做者用独立凭据在新题上原生加载，核对答案和包内随机串，同时检查对首做 Run 和私有候选的访问。原始转录随临时目录清理，报告保留标识、摘要和核对结果。[实测记录](public-handoff.windows.json)

## 手工启动 Windows 服务的前提

为本轮指定全新的 `-DataDir` 和 workspace。先按当前版本初始化，再用 `tools/start-dev.ps1` 启动，嵌入式 AnySeek 使用同一数据根目录。保留已有数据库。旧库缺少 `oc_run_verifications` 时记录为 schema 不兼容，改用新库。登记初始化二进制摘要、schema 和 workspace UUID。

## 独立检索预检（未评分）

报销交接演练的首做题面直接给出制度；检索预检另用只含申报事实及制度标识的题面。独立保管人保存授权知识资产 v1/v2、原文、定位、哈希和答案。获权新会话实际 search/get，登记精确版本、准备回执、客户端可观察的接收证据和输出。无权对照使用独立干净会话，检查拒绝或不可见结果，避免传入旧正文或 Skill 副本。撤回 v2 来源后分别观察服务端新获取与本地残留。[既有 v2 检索实测](../../docs/VALIDATION.md)
