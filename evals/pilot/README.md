# #91 双场景试点验收

**简体中文** · [English](README.en.md)

数据库排障由 Claude Code 首做、Codex 复做。报销复核由 Codex 首做、Claude Code 复做。两个方向已完成公开合成演练，正式未见题试点及独立签认仍为 **NOT_RUN**。[Issue #91](https://github.com/TheLudlows/Origence/issues/91)

| 要做什么 | 入口 |
| --- | --- |
| 核对本轮已登记的冻结字段、缺项与合成预检 | [冻结准备记录](FREEZE-READINESS.md) |
| 生成开发题或封存未见题、填写和核对评分表 | [执行手册](RUNBOOK.md#题集与评分表) |
| 运行真实客户端的公开预检 | [复跑命令](PREFLIGHT.md) |
| 冻结条件、执行 P1–P7、恢复和清理 | [执行手册](RUNBOOK.md) · [冻结表](freeze.template.json) · [签认表](signoff.template.json) |
| 核对 PRD v0.3 条款和正式指标分母 | [A1–A7 对照](PRD-MAPPING.md) |
| 查看逐次结果、失败尝试和覆盖范围 | [验证记录](VALIDATION.md) · [原生加载 JSON](native-skill.windows.json) · [跨客户端复做 JSON](public-handoff.windows.json) |
