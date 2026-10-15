# #60 候选二设计审计：index_entries → artifact_ledger

结论：本轮保留两表。当前模型不存在无损的一对一替换；若只求少表，需要复制独立状态、产生空键/哨兵行，或引入跨来源×profile展开，违背本 issue 的减复杂度目标。并不是逻辑上永远不能统一，而是必须先批准“每个精确外部 vector 写入只有一个发布状态”的新契约；这不是等价 schema 精简。

基线：TheLudlows/Origence main `4c0862e80ccfb8b403268170c06d391239790d2b`，tree `7e73c2e3`。这是设计审计，不是实现或测试通过记录。PR #59 的 `publication.receipt_metadata` 已纳入，未使用已移除旧 API 作证据。

## 1. 完整键与事实来源

|对象|唯一键|非键事实|约束/含义|
|---|---|---|---|
|index|tenant, workspace, artifact, field, model, generation|dimension, state, created_at|dimension 不在主键；pending/ready/removed；不含source/version；无指向ledger的FK|
|ledger|tenant, workspace, source, version, artifact, surface, generation|artifact_type, idempotency_key, state, attempt, last_error, next_retry_at, timestamps|artifact_type不在主键；vector/graph/full_text；pending/committed/retry_wait/orphan；没有field/model/dimension|
|native vector|每个profile一个物理表；表内tenant, workspace, source, version, artifact, generation|vector，dimension固定于表schema|不同profile/generation/source/version可保留不同物理行；当前native格式无field列|
|owner|tenant, workspace, source, version, artifact_type, artifact|chunk_id, created_at|独立来源归属，不能由ledger替代|

索引ready只证明指定artifact/field/profile/generation的索引登记；ledger committed只证明指定source/version/surface/generation的写入已提交。检索还需精确来源、当前版本、active event、权限及Learning支持校验；二者任何一个都不是完整可见性证明。

合法组合需区分schema可表示与生产writer目前产生：
- 多profile/多field/多generation：index键允许并存；ledger在同source/version/artifact/vector/generation下只有一行，因此一个ledger行可被多个index profile匹配。生产worker当前只写content、每个publication一个profile，不能据此删除schema/API的独立维度。
- 多source/version：ledger/native可并存；index同artifact/field/model/generation共用一行。生产chunk ID由job/generation/ordinal等生成，通常不同发布不共享chunk；不能将graph明确支持多来源误说成worker正常共享chunk。底层schema没有强制“artifact唯一source/version”，读路径依靠chunk→version的精确绑定排除不一致组合。
- 同profile不同dimension：index同完整键不允许两行；index_ready冲突只更新state，不更新dimension；Lance同profile固定维度，异维度写入拒绝。不能将dimension塞入新唯一键而允许同profile异维度共存。
- 同artifact不同generation必须并存；generation不是scope全局唯一的模型标识，不能以generation替代model/source/version。
- 不同scope完全隔离；artifact_type与surface不可据artifact_id猜测。graph/full_text不应被迫填写虚构vector字段。

## 2. 全状态乘积矩阵

下表为同一待校验vector对象：∅表示缺行。schema无跨表约束，所有组合都可独立表示；“正常”仅表示当前worker或维护路径的常见来源，不声称其余一定是正常业务。

|index \ ledger|∅|pending|retry_wait|orphan|committed|
|---|---|---|---|---|---|
|∅|未准备/无记录|正常准备、native写入中/失败后|账本重试状态；尚无索引|准备超时或未完成产物残留|提交登记不完整/缺索引；不可检索|
|pending|独立/未完成登记|双未完成|未完成+待重试|未完成+已孤立|账本已提交但索引未ready；不可检索|
|ready|独立索引/缺账本；不可检索|索引仍在、账本待确认|索引仍在、账本待重试|索引仍在、此精确来源已孤立|仅此格通过两项索引发布条件，仍需其余可见性校验|
|removed|移除索引残留|重新准备但未ready|已移除且重试|常见清理终态|账本已提交但索引被移除；不可检索|

重要区别：ready+orphan不等于removed+orphan，pending+∅不等于∅+pending。以单一状态枚举编码全部乘积只是把两个状态藏进名称，不是统一事实来源。index pending在schema存在，当前生产代码没有显式写pending的路径；ledger retry_wait存在确认/恢复读取，但未发现生产路径将ledger写成retry_wait。它们是可单独审议的契约/字段精简项，不能在合表里顺手删除。

## 3. 生产时序、重试、撤回与清理

|阶段|index|ledger|持久性/恢复条件|
|---|---|---|---|
|准备事务|通常无新index|register_pending，冲突保留旧行；resume将orphan/retry_wait→pending|同时保存完整publication；commit后才能native IO|
|native vector/full_text/graph写入|不变|pending|IO在事务外；部分失败可能已有native残留但不可检索|
|最终发布事务|插入/恢复ready|pending/retry_wait→committed；已committed幂等|guard、版本期望、版本/chunk/owner、index、ledger、job完成同事务；最终权限重验保留|
|崩溃在最终commit之前|新index不可见|pending或恢复后orphan|同generation saved publication保留向量等重放输入；重试相同native完整键，不能只凭receipt_metadata宣布成功|
|重试恢复|保留既有index状态至最终提交|register幂等；resume orphan/retry_wait→pending|不得把orphan当永久不可恢复；confirm自身不会orphan→committed|
|reconcile|不变|pending超5分钟→orphan；到期retry_wait→pending|不证明native实际不存在；重试字段不会被confirm清空|
|来源撤回/asset删除|可暂仍ready|可暂仍committed|active source/current asset权限链先阻断可见性；不等待物理删除|
|cleanup第一事务|尚未统一改状态|尚未统一改状态|移除失效owner，形成native清理计划；可在重开后重试|
|native删除|关系状态不作成功猜测|同左|单worker串行，native IO不进关系事务|
|cleanup最终事务|仅当chunk不存在才removed|仅当精确source/version/artifact owner不存在才orphan|两个谓词不同；历史有效owner保留，graph共享来源独立处理；失败重开可重复|

索引生命周期按artifact/profile；账本生命周期按来源发布。删除一个来源账本不能无条件使共用index removed；反过来保留一个index也不能让已撤回source的账本committed。以新增nullable index_state+field/model/dimension拼入ledger，只是复制两种状态到每个来源，且更新一次index须传播到多个账本。

## 4. 缺行、异常状态与新metadata

- message_batch_status：job缺行NotFound；receipt身份不符Conflict；来源/asset缺失或已撤回Invisible；job pending/processing/retry_wait→Pending，failed→Failed，cancelled/superseded相应状态；未知job state报Conflict。completed仍逐项核对exact chunk数、generation/profile、index ready+dimension、两个surface committed，缺项Invisible。
- malformed receipt_metadata/原生ID token不是成功缓存，验证失败Unavailable；metadata仅持久保存轻量chunk ID与dimension计划，不能替代真实index/ledger和来源检查。
- 低层候选/ready匹配：缺行或错键不匹配；不能据此说所有调用均抛错。高层已参与native top-k的候选在最终验证缺失/变更时Unavailable，防止静默少结果。须保留这两层区别。
- 启动：先分页committed ledger，再批量精确检查chunk/version/asset/event，再找ready content index以发现profile；没有匹配index不宣称需要该profile。入选profile非法类型/非法dimension报错；被SQL谓词排除的坏行不应提前解码导致原本有效读取失败。表缺失即使前一步为空也必须prepare并暴露错误。
- ledger register冲突DO NOTHING；confirm缺行、无active source、错误generation或orphan是no-op。改成异常、重置dimension、删除旧pending等均是需独立评审的行为变化。

## 5. 为何本轮保留；如另立契约，最小统一方向

当前查询逻辑是I(scope,artifact,field,model,generation)与L(scope,source,version,artifact,surface,generation)两个独立关系的交集。等价展开到一张表需要来源×profile行，并保留尚无对方的记录；也要同步传播共享index的状态/维度。用空source或空profile的哨兵记录、JSON profile清单、独立index_state都是“少表但没少事实/状态”。仅删除index并以committed替代则放松profile/field/dimension验证。

若用户愿意改变契约，可以另案采用“精确vector写入即唯一发布单位”：
1. vector唯一键为(scope,source,version,artifact,surface,generation,field,model)，dimension为非键且必须匹配profile固定维度；非vector继续旧唯一键。使用surface条件CHECK和分别的partial UNIQUE约束，不能依赖含NULL的复合唯一键，也不使用任意sentinel字符串。
2. 一行仅state pending/committed/retry_wait/orphan；准备时已存不可变field/model/dimension，最终commit同时表示该精确写入已验证完成；不增加ready/removed影子状态。retirement由来源/owner/native清理协议统一控制。此定义是新的状态契约，不能由历史ready=committed推导。
3. 为真正支持不同field，还必须补native identity/routing中的field。目前Lance无field且按profile分表，不能光改关系主键宣称多field正确。
4. 保存publication仍是恢复输入；每个精确键独立pending/commit，重试及幂等key纳入field/model且碰撞安全编码。不同profile不共享一个commit证据；同generation重放不覆盖其它profile/source。
5. 所有读路径单表参数化定长批读后在同事务应用层关联；保留当前4/8-slot边界和启动32-row seek分页，不写JOIN/跨表嵌套。不应将目前scope全量maintenance扫描误称已有分页；若要求全量路径也内存有界需另定批处理且保留事务一致性。
6. 此方案可真实删除一次index_ready写、一次独立index匹配；但会增加profile级ledger行数，且当前1-profile publication没有证明明显净收益。需测Q/W/行数/时间/内存而不是以表数代替证据。

需用户先定的边界：是否接受将原source无关index能力改成source/profile精确发布；是否引入native field支持；是否收紧现有no-op/错误返回；是否删除schema允许但当前writer未产生的状态；是否把不一致状态从Invisible改成错误。未批准前不实施。

建议#60候选二验收记录为“已设计验证，本轮不合并”，继续优先身份一对一合表和逐字段证据删除。无需迁移；未来实施统一模型也必须保留同版本重开/重试/崩溃恢复、明确拒绝不兼容库、不自动转换删除已有数据。

## 固定源码证据

以下所有链接固定在上述最新main：
- [DDL键与状态](https://github.com/TheLudlows/Origence/blob/4c0862e80ccfb8b403268170c06d391239790d2b/src/storage/sqlite-schema.sql#L151-L191)
- [worker准备、native IO、最终发布](https://github.com/TheLudlows/Origence/blob/4c0862e80ccfb8b403268170c06d391239790d2b/src/worker.rs#L643-L880)
- [账本转移及no-op](https://github.com/TheLudlows/Origence/blob/4c0862e80ccfb8b403268170c06d391239790d2b/src/storage/sqlite/ledger.rs#L5-L97)
- [index_ready及候选](https://github.com/TheLudlows/Origence/blob/4c0862e80ccfb8b403268170c06d391239790d2b/src/storage/sqlite/app.rs#L176-L216)
- [恢复pending](https://github.com/TheLudlows/Origence/blob/4c0862e80ccfb8b403268170c06d391239790d2b/src/storage/sqlite/app.rs#L412-L420)
- [清理的不同谓词](https://github.com/TheLudlows/Origence/blob/4c0862e80ccfb8b403268170c06d391239790d2b/src/storage/sqlite/app/maintenance.rs#L246-L313)
- [新receipt_metadata与活体发布检查](https://github.com/TheLudlows/Origence/blob/4c0862e80ccfb8b403268170c06d391239790d2b/src/storage/sqlite/publication.rs#L118-L235)
- [独立精确index/ledger条件](https://github.com/TheLudlows/Origence/blob/4c0862e80ccfb8b403268170c06d391239790d2b/src/storage/sqlite/publication.rs#L580-L583)
- [精确ready匹配](https://github.com/TheLudlows/Origence/blob/4c0862e80ccfb8b403268170c06d391239790d2b/src/storage/sqlite/read_model.rs#L1339-L1409)
- [启动分页及profile校验](https://github.com/TheLudlows/Origence/blob/4c0862e80ccfb8b403268170c06d391239790d2b/src/storage/sqlite/full_text/startup.rs#L117-L326)
- [native vector主键和固定profile维度](https://github.com/TheLudlows/Origence/blob/4c0862e80ccfb8b403268170c06d391239790d2b/src/storage/lancedb.rs#L1-L7)
- [native最终校验回归](https://github.com/TheLudlows/Origence/blob/4c0862e80ccfb8b403268170c06d391239790d2b/src/retrieval/publication_tests.rs#L154-L190)
- [历史及共享owner重开清理回归](https://github.com/TheLudlows/Origence/blob/4c0862e80ccfb8b403268170c06d391239790d2b/src/storage/sqlite/app/maintenance/tests.rs#L130-L225)
