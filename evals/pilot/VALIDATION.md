# #91 本地验证记录

日期：2026-11-17。基线为 `293e2bf6af3b180d43c8050731047b552ac07519`，
`git ls-remote origin refs/heads/main` 已核实一致；原检出 HEAD 为
`c70a1aaa0b3cb75872109406dc17b1b461d1f771`，未在其上编辑。
独立工作树 `/workspace/Origence-pilot`，分支 `eval/p0-independent-pilot`。
最终提交 SHA 由提交后根线程报告/`git rev-parse HEAD` 记录，避免文件自引用哈希。

已阅读根 AGENTS.md、当前 STATUS、架构设计、Skill/Run/存储合同、CLIENTS、
OPERATIONS、评估标准和相关 client_skill/runs 实现。基线与 `/workspace/.agents`
未发现 `.agents/skills`，未声称加载任何缺失 Skill。

- `python3 -m unittest discover -s evals/pilot -p 'test_*.py' -v`：10 tests PASS。
- `python3 evals/pilot/pilot.py generate --seed public-development-1 --output /tmp/origence-pilot-91-smoke`：成功生成 16 题和 32 条 baseline/reuse 空观察记录。
- `python3 evals/pilot/pilot.py score /tmp/origence-pilot-91-smoke/observations.json`：4 组各 8 题、32 必要检查，成功/核验均 0；返回 NOT_CERTIFIED / NOT_CHECKED。这是未跑登记，不是产品失败率或模型成绩。
- `cargo fmt --all -- --check`：未执行成功，环境 `cargo: command not found`。Rust toolchain 声明为 1.98.0；Clippy/Rust suite 同样未运行。本路不改 Rust、生产 SQL、服务/DTO、UI、CI 或共享状态。
- SQL 手工范围审查：本路 Python 没有 SQL 执行/生成接口，无数据库连接、网络或模型调用；未触及 JOIN-free 事务、权限或发布合同。

因本路明确限定新验收目录，记录放此处，不改共享 `docs/VALIDATION.md` / `docs/STATUS.md`。
合成 smoke 输出保留在上述 /tmp 路径，目录由本次生成器创建；没有创建产品数据库、集合、凭据或客户端副本。

## 未验收 / 发布边界

A1–A7 联合编号/合同仍待协调确认。四路功能须落在同一整合 SHA 后重新验收。
真实 Claude Code/Codex、服务/模型冻结、原生 Skill 加载与撤回、浏览器/Windows、
实际 SQLite/AnySeek/Blob 同版本恢复、DBA/制度负责人/独立签认全部 NOT_RUN。
未运行的收费/生产访问不构成阻塞的绕过理由。无迁移工作。
仅本地提交；不推送、不开 PR、不更新 Issue、不合入、不部署。

## #92 草稿复审修复（2026-11-17）

修复有效超限样例缺口：报销正常、有效超限、重复、缺凭证各两个不同实例，
准备/未见分组保持同一覆盖。超限非重复且有凭证，独立断言 v2 截顶、低于申请额，
且不等于按 v1 可全额报销的结果。总题数 16、观察数 32、必要检查分母均不变。
每个场景/分组用种子稳定重排后重新编号，原始种子、分组/类别标签和 oracle 答案
不进入 tasks.json。公开测试种子不是真实封存未见种子。

13 项 Python 测试通过，包括 102 个种子 × prepare/held-out 两组覆盖检查、
相同种子重现/不同种子改变类别映射、分组隔离与 Agent 输入不含 oracle。
prepare/held-out 两组生成及评分 smoke 通过，均 NOT_CERTIFIED。
主干检查仍为 293e2bf6af3b180d43c8050731047b552ac07519，修复前远端 PR head
为 045843a970944eec502b3c415805cf64b69b869b，与本地父提交一致。
未运行模型或真实试点；原有 NOT_RUN 与独立签认门槛保持。
