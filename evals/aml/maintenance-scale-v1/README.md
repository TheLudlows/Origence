# 全库与重复压实验证

本轮基于 cc4ee55 的维护实现，容器输入修复单独在 PR #34 合入 5b4bd94。不改变生产 Search、维护算法、依赖、来源或 parser。单模型真实 HTTP 测量与多 profile 确定性库回归分别记录，不把后者当作第二个真实模型的成绩。

## 预登记范围

[计划](plan.json)、[模型预检](preflight.json)、[受测实现](implementation.json)。沿用固定公开 LongMemEval 已观察的 32 个用户、1524 批、15685 条消息；按 question ID 升序，每阶段每题一次 top-100。基线 → 停机压实 → 重启 → 追加其他四个用户 256 批 → 再压实。追加期间另查前 8 个原用户，共计划 168 次 Search。

所有阶段使用同一隔离库、同一二进制和本地 BGE-M3。结果必须完整 data 内容、ID、元数据、顺序完全一致；独立审计重新对原消息 UTF-8 范围和 user scope 逐条核对。没有 Answer、重排或聊天裁判调用，也不重新发布质量分数。

执行器只在 target/aml-drills 创建自身临时目录；输出必须是新文件。宿主在已完成请求后停止（既有 Host.stop 使用进程 kill，不当作优雅关闭验收），维护时宿主已退出。没有在压实中强退，没有硬取消/断电/磁盘不足保证。二进制来源可由上一轮 manifest 和本轮计划交叉核对。

## 多 profile 回归

新增 repeated_compaction_preserves_multiple_profiles_after_mutations：三个独立 profile（维度 2/3/5）、两个 tenant × 两个同名 workspace ID、generation 1/2、相同 artifact ID 跨 scope/profile 重用。每个 profile 初始 64 行，共 192 行。三轮维护之间更新 embedding、按来源版本定向删除、追加数据；每轮都按独立预期集合检查 ID/source/score/排序及候选 ID 子集，再比较压实和重新打开后的完整 NativeHit。总共 288 次库查询、9 次逐表压实，不调用模型。

这覆盖多表和持续变更后的数据正确性，不是多 profile 容量基准。默认仍只显式停机维护，不据此启用自动调度或设定维护频率。

## 本地验证

Rust 1.98.0 fmt、严格 Clippy 通过（8.41 秒）；lib 32 passed（1.19 秒），local 111 passed（137.63 秒）。新增测试单独运行 1 passed（129.83 秒）；不把测试运行时间当产品查询延迟。Python 3.13.11 设置 AML_TEST_BINARY 后 49 passed / 0 skipped（4.727 秒）；新增审计反例覆盖漏项、重复/乱序、行数变化、额外表、预览/重启变化和未追加数据。

完整 HTTP 实验结果将在本轮结束后与原始响应、离线审计一起记录。入库阶段与本地回归执行存在重叠，不将入库时间当独占机器吞吐测试。
