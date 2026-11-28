# 从选定经历生成 Skill 的 CLI（#88）

**简体中文** · [English](SKILL-CANDIDATE-CLI.md)

`origence-skill` 是基于认证 HTTP API 的独立入口。起草不会打开数据库、调用模型、挖掘 Run、提交审阅或自动发布。宿主须已运行。使用所有者的 `OC_API_KEY` 与 `OC_SERVER_URL`。选定输入文件和仅所有者可读的候选输出须保密。默认二进制仍为常规 `origence` CLI。

## 在控制台从 Run 起草

使用 Run 所属工作区的 Key 登录，打开「调用链路」中的 Run。选中截止事件，再点「选取依据并起草」。草稿绑定截至该事件的固定前缀。可切换时间线事件继续核对，草稿的前缀保持固定。

手填方法和可披露的脱敏依据，确认两者的披露权限后保存。记录候选 ID，可在同一 Run 的「Skill 候选」区重新打开。保存修改会追加修订，并使旧批准失效。填写目标 Skill ID 和版本，再提交已保存的修订。管理员用账号登录，选中该工作区，打开「Skill 审核」。查看选定依据与包文件后，可以批准或拒绝。批准后可单独点「发布 Skill」。

## 使用 CLI 选择依据并编辑

运行 `cargo build --bin origence-skill` 构建，使用 `target/debug/origence-skill`，也可将 `target/debug` 加入 PATH。以下示例使用占位符。选择一个且仅一个固定来源。

```sh
origence-skill run-source RUN_UUID 3
# 或选择已发布的 Improve → Learning 结果的固定版本
origence-skill asset-source LEARNING_ASSET_UUID 1
```

将该 JSON 对象复制为私有 `selection.json` 的 `source`。Run 序号指向不可变前缀端点，不能隐式使用最新或完整 Run。`asset-source` 也接受已获授权的精确知识或记忆版本。它只返回地址和哈希，来源正文及私有 Learning 依据不复制进候选。现有来源可见性和 Learning 依据检查继续生效。CLI 不运行 Improve 或提取私有轮次，应选择已存在的 Run 前缀或已发布 Learning。

```json
{
  "source": {
    "kind": "run",
    "run_id": "11111111-1111-4111-8111-111111111111",
    "through_sequence": 3,
    "digest": "REPLACE_WITH_EXACT_PREFIX_DIGEST"
  },
  "name": "verify-result",
  "description": "Independently verify a result before reuse",
  "steps": "1. Record inputs and assumptions.\n2. Check the output independently.\n3. Record limitations and unresolved failures.",
  "selected_basis": "A redacted observation the owner explicitly permits this reviewer to see; not proof of general reliability.",
  "disclosure_confirmed": true,
  "private_notes": "Private supporting notes, excluded from review and package."
}
```

所有者须确认公开**方法和选定依据两者**。该标志是用户显式声明，无法检测秘密或代替独立核验。标志为 false 或缺失时，保存前就会失败。字段严格且有上限，文件实际读取最多 1 MiB。名称与步骤使用既有纯文本包校验。不会新增可执行元数据、动态 shell 展开或模型。

```sh
origence-skill save CANDIDATE_UUID --expected-revision 0 --input selection.json
origence-skill candidate CANDIDATE_UUID
# Edit selection.json manually, then append an exact revision:
origence-skill save CANDIDATE_UUID --expected-revision 1 --input selection.json
```

`SelectedExperience::candidate()` 是公开、与宿主无关的确定性预填函数。选定步骤保留为可编辑的 Skill 正文，并绑定一个精确来源。既有 `save_skill_candidate` 服务与事务会先核验实时权限、逻辑所有权和来源权威再持久化。单独调用预填函数不会授权或创建服务端对象。相同 ID、预期修订号与内容重试时返回同一修订。更改负载会冲突，较旧的编辑器不能覆盖新版本。

选定依据保存在仅所有者可读的版本化备注对象中。步骤、依据、来源或备注的任何修改都会追加修订，使旧批准失效。使用该对象时，若审阅中的依据不同于已保存的选择，服务会拒绝。更改依据须编辑候选，直接调用 HTTP 也一样。未使用该对象的手工候选保留原显式审阅输入约定。原始备注、源 ID 与私有支持材料不会自动进入选定审阅包或分发包。

## 审阅、批准和显式发布

在单 Key 工作区，将精确的已保存修订和发布目标提交给管理员审核。

```sh
origence-skill submit CANDIDATE_UUID REVIEW_UUID --revision 2 \
  --tenant-id TENANT_UUID \
  --workspace-id WORKSPACE_UUID --skill-id SKILL_UUID --version 1
origence-skill review REVIEW_UUID
# 管理员批准后，候选所有者也可以发布。
origence-skill publish REVIEW_UUID EXACT_PACKET_DIGEST
```

尚在使用多 Key 的旧工作区可在提交时指定 `--reviewer-id REVIEWER_CREDENTIAL_UUID`。获指定的有效审阅者使用自己的凭据运行 `origence-skill review REVIEW_UUID`，再运行 `origence-skill decide REVIEW_UUID EXACT_PACKET_DIGEST --approve` 或 `--reject`。决定不可变，改变决定须新建审核 ID。

提交时，服务端在读取后再次检查当前修订。并发编辑会阻止提交。审阅包只包含生成的包、选定依据以及审阅和目标标识。审阅不授予访问原始归档的权限。发布是独立步骤，再次检查当前修订、凭据、来源与摘要权威。已审阅方法在显式发布前仍是提议，其指令不授予工具执行权限，也不能证明自报结果真实。

```sh
origence-skill package SKILL_UUID 1
origence-skill withdraw SKILL_UUID 1
# Subsequent online package fetch now fails.
```

撤回来源也会拒绝在线获取。现有 `origence skill-export` 仍可为 Claude Code/Codex 显式导出。离线副本无法远程清理或立即收回。此次没有新增原生客户端加载、自动发布、共享导航、结构迁移或跨 Run 挖掘。

测试结果及验收范围见 [Issue 88 历史验证](https://github.com/TheLudlows/Origence/blob/a56246406a0688f646a45a85cc2954fb14a8d28a/docs/validation/issue-88-skill.md)。
