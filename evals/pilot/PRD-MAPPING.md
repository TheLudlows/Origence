# PRD v0.3 与试点执行清单

**简体中文** · [English](PRD-MAPPING.en.md)

依据 PRD v0.3 第 5、11、12 章（印刷页 6、13、14）。Library 原件版本为 0，文件 ID 和 SHA-256 见[冻结表](freeze.template.json)。以下映射待联合审阅。

## 逐条对应与缺口

签认表用 `prd_matrix` 登记 PRD 的 A1–A7，用 `matrix` 登记试点的 P1–P7。两者按下表关联。旧试点记录中的 A* 按当时的执行项解读，P7 同版本恢复是另列的签认条件。正式双场景试点为 NOT_RUN。[执行矩阵](RUNBOOK.md#执行矩阵) · [预检结果](VALIDATION.md)

| PRD 条款与原文要求摘要 | Issue / pilot 关联 | 尚待真实证据 |
| --- | --- | --- |
| prd-v0.3.A1 私有会话：同逻辑 key 双端读取同一归档问答；轮换接续，独立 key 拒绝 | [#87](https://github.com/TheLudlows/Origence/issues/87) · P1/P5。Windows 接入 readiness 已在 `4160fd5` 通过，覆盖双端互读、独立 Run、权限隔离、轮换与撤销。[验证记录](../../docs/VALIDATION.md#2026-11-18--87-双客户端归档互读与独立-run) | 正式试点须在最终整合提交和新库上按冻结配置复核，并由独立验收人签认。这些正式试点证据仍待采集。 |
| prd-v0.3.A2 长期记忆：同条件已发布事实更正后新任务用更新值；不同条件仍分别定位来源 | [#90](https://github.com/TheLudlows/Origence/issues/90)；P4/P5（须补专门事实步骤） | 两环境/条件事实，只改一项；另一获权 key 实查新值、未改值与各自精确来源。现有报销金额题不能代替事实写入/筛选/分页验证 |
| prd-v0.3.A3 知识与文档：新任务查 v2，旧 Run 读有效 v1，撤回 v1 来源后旧链接拒绝正文 | #90；P3/P4 | 原始 PDF 页与引用核对、真实入库/发布、新旧 Run 精确引用、撤回后旧链接阻断及 v2 独立有效；补更新失败旧版仍可用、并发冲突证据 |
| prd-v0.3.A4 反馈与 Learning：归档问答、反馈、显式 Improve；已发布经验带条件，成员可召回正文，私有支持指针只属 owner | [#88](https://github.com/TheLudlows/Origence/issues/88)，#87 的显式反馈链；P2/P3/P5/P6（须补完整 Learning 步骤） | 同一真实经历→反馈→显式 Improve→发布 Learning 的来源/条件链；另一成员召回正文及私有支持指针拒绝。直接创建 Skill 候选不能替代此链 |
| prd-v0.3.A5 Skill 发布与采用：审批后修改包或依据需重审；另一客户端加载获批版本，提交可核对操作与独立结果 | #88、[#91](https://github.com/TheLudlows/Origence/issues/91)；P2/P3/P4/P6 | 精确 revision/packet/package digest、修改后旧批准失效、重审与显式发布、跨客户端原生加载、操作和独立签认；仅下载/导出不足 |
| prd-v0.3.A6 上下文与运行：同 Run 区分服务准备、客户端使用报告、业务核验；缺事件未知，重试去重、冲突和准备失败可见 | #87、[#89](https://github.com/TheLudlows/Origence/issues/89)；P1/P6 | 同一 Run 展示准备 v2、自报成功、独立失败、实际使用版本未知；精确 prefix 与独立签认身份/时间/依据；重放/冲突/失败真实记录。缺事件不是业务失败或成功 |
| prd-v0.3.A7 权限与反查：跨 key 反查同资产版本各见获权 Run，跨 workspace 拒绝，撤凭据不能访问 | #90、#87；P5/P6 | 两 key 对同一资产精确版本的反查正例及交叉拒绝、Run 正文另行核权、跨空间与撤权负例。P7 的恢复成功不能满足本条 |

## 计量差异必须披露

PRD 业务成功率为独立通过数 / 已独立判定数，核验覆盖率为已独立判定数 / 到期应核验数，另列待定数。例如三项到期、通过一项、失败一项、待定一项，对应 1/2 和 2/3。零分母记 null。
离线评分表按全部预登记题数计算 `task_success_rate`，按冻结的必要检查总数计算 `verification_coverage`。缺独立核验的检查保留在分母中。正式签收时另报 PRD 分母，并附独立判定依据。
