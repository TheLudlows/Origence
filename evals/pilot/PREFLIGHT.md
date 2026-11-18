# 公开合成样例预检

目的：在正式准备/未见样本外，用一条公开合成样例检查能否取得所需证据。
Windows 原生 Skill 加载预检使用已安装客户端、已授权模型和独立新库。正式试点的冻结表和分母单独保留。缺证据记 UNKNOWN，未尝试记 NOT_RUN。

## 复跑原生 Skill 加载

使用 Python 3.11 或更高版本。从仓库根目录指定 Origence 和 AnySeek 服务端二进制、两端模型选择器及一个尚不存在的报告文件。Codex 使用已有的 OpenAI 登录。Claude Code 从环境或可选的 `--claude-settings PATH` 读取供应商连接配置。命令会调用真实模型。

```sh
python tools/anyseek_smoke.py --anyseek-binary ANYSEEK_BINARY --run python evals/pilot/live_skill_preflight.py --binary ORIGENCE_BINARY --claude-model CLAUDE_MODEL --codex-model CODEX_MODEL --report NEW_REPORT.json
```

脚本创建临时 workspace，测试作者和审阅账户按精确来源发布一版 Skill，在独立项目目录分别执行 `skill-export`。校验串仅在包正文出现。两端使用各自的 Run 和新会话，报告同时记录原生加载信号、回答是否匹配，以及成功读取后由测试程序上报的 Run 前缀。客户端共享 100 秒运行时限。之后撤回来源，核对在线包和新的导出请求，再核对两份已有文件。任一必要检查缺失，命令以非零状态退出。报告仅保存标识、摘要及观察结果，临时目录内的原始会话和请求内容随测试清理。实测记录见[验证记录](VALIDATION.md)。

## 正式试点所需证据

样例：公开合成 TEST 币申请 110，v1 上限 150、v2 上限 100，有凭证、非重复；
制度负责人预期复核建议按 v2 截顶 100，不执行付款。该样例和答案允许公开，禁止纳入未见评分集。
真实双向加载需分别记录 Codex、Claude Code；一个宿主预检不能替代另一个。

| 检查项 | 待填记录 / 判读 |
| --- | --- |
| 身份与隔离 | tenant/workspace、逻辑 key 别名（不记密钥）、公开样例 ID、独立项目目录；仅合成测试资源 |
| 精确环境 | 整合 SHA、服务/客户端二进制版本与 SHA256、模型精确修订/配置摘要；未知不填 latest |
| 输入与来源 | 样例/制度 bytes SHA256、asset UUID + version、source/locator（PDF 页与 UTF-8 范围如适用）；v1/v2 分别记录 |
| 会话与 Run | 客户端 session ID、服务器 Run UUID、精确不可变 prefix digest、事件 ID/序号/UTC；原生会话不等于平台归档 |
| 审批与包 | candidate revision、review packet digest、批准人/时间、skill UUID/version、canonical package digest、SKILL.md/reference 文件 SHA256；单文件 hash 不代替包 digest |
| 在线获取/导出 | 请求关联 ID、服务器当时 scope/version/source 结果、输出字节/路径与哈希；仅记录授权获取结果，不填写为原生已加载 |
| 原生加载 | 真客户端原生事件/诊断的证据路径+哈希，能否绑定上述 session/Run/Skill/version；无原生证据 UNKNOWN，模型自述不能替代 |
| 模型上下文 | 实际调用可观测的输入/内容版本绑定，采集者、覆盖与遗漏；prepared 或 report 单列，不能推断实际进入模型。未执行/无法观测分别记 NOT_RUN/UNKNOWN |
| 业务核验 | 输出工件 hash、适用制度版本、审阅人/时间/独立依据；缺独立人不得填 PASS，本预检不产出评分 |
| 撤回后新获取 | 记录撤回的确切来源/版本和时间，再次在线下载/导出的拒绝证据；若未执行则 NOT_RUN |
| 残留与会话 | 单独登记本地文件/缓存、旧上下文、新会话各自实际状态与清理责任人；拒绝新获取不等于回收副本 |
| 受管在线重载 | 独立记录机制、再授权/切换/撤回证据；当前未验证，不用手工重导出冒充，不降低原要求 |
| 收尾 | 证据只在受限测试目录，保留/清理清单及遗漏；没有授权的新动作停留为待执行 |

每条证据记录相对路径、SHA256、UTC、采集者、证据层级；以 `prd-v0.3.A*` 和 `pilot.P*`
分别关联，不按数字互换。原始 seed、隐藏答案不进入 Agent 输入，本样例没有隐藏种子。

## 手工启动 Windows 服务的前提

必须显式指定本轮全新的 `-DataDir`、独立的 `-AnySeekDataDir` 和专用测试 workspace，
不能沿用环境变量中旧的 `OC_DATA_DIR` 或已有业务 workspace。按当前整合版本在这些新路径
完成所需初始化后再启动；`tools/start-dev.ps1` 要求目录/已初始化 context.db 等前置条件，
“新库”不是把空目录直接交给脚本即可启动，也不是删除或覆盖旧数据。
记录两目录的测试资源别名、初始化二进制 SHA、schema 与 workspace UUID，不记录凭据。

[#95](https://github.com/TheLudlows/Origence/pull/95) 增加 `oc_run_verifications`，采用
fresh-only 精确 schema admission。旧库缺该表的失败属于**启动 schema 不兼容**，
不能误报为 Skill 加载、模型、MCP 或业务任务失败；停止该次启动并保留原库，改用本轮新库。
不手工补表、不迁移、不用清空旧库绕过。此处仅预检要求，不声称 Windows 已验收。

## 独立检索预检（未评分）

现有 `pilot.py` 报销题及上面的公开样例直接给出两版 cap/rule，只能支持规则应用与证据流程准备，
不能证明模型实际检索了制度。独立检索预检另建公开合成样例，不改生成器，也不进入正式题数或分母。

- Agent 题面只含申报事实（TEST 币金额、凭证/重复标记等）和可检索的制度标识；不给 cap、rule、
  正确金额、版本答案或本预检文档。操作员与 Agent 输入分离，禁止将下面的评判资料复制入上下文。
- 在专用测试 workspace 的真实授权知识资产 v2 中存放适用规则；v1/v2 内容、原始来源、版本、
  locator 和哈希由独立保管人记录。期待结果只由独立人持有，不通过题面泄漏。
- 获权新会话必须实际 search/get，保留服务端精确版本引用、准备回执、客户端可观测接收/使用迹象和
  输出工件。仅答案恰好正确不能证明检索；缺事件为 UNKNOWN，不补造模型输入证据。
- 另开无权读取该资产的独立干净会话作对照，不共享已检索文本、Skill 副本或旧会话上下文。
  记录服务端拒绝/不可见结果；该会话即便猜中金额也不算获取证据成功。不得向其泄露规则作为“对照”。
- v2 来源撤回后分别核对新的在线获取被拒与本地残留，不能把前者写成副本已回收。
  全部预检只报告证据可见性/缺口，不计正式业务成功率或核验覆盖率，不代签。

已有获权 v2 读取的实测结果见[主干验证记录](../../docs/VALIDATION.md)。无权模型会话对照和正式双场景的独立核验仍待执行。
