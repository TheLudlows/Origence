# Skill 审阅、发布与分发

**简体中文** · [English](skills.md)

本契约定义公开库与 SQLite 工作流。上报成功本身不能证明方法有效。客户端基于有权访问的精确资产版本或不可变私有 Run 前缀提供草稿。所有操作都不会自动发布。

## 契约

- `save_skill_candidate(id, expected_revision, input)` 以调用方稳定逻辑 key 创建或追加不可变修订。私有备注和来源引用只对所有者可见，工作空间管理员也不能读取。
- 所有者显式提交审阅 ID、精确修订、指定审阅者凭据、工作空间/Skill/版本目标及选定的脱敏依据。仅指定审阅者及所有者可读审阅包。审阅者不会隐式获得读取原始归档的权限。
- `decide_skill_review(review, packet_digest, approve)` 要求获指定且当前有效的 Publish 角色。决定不可变，变更须新建审阅 ID。凭据轮换要求重新指定审阅者。获授权的逻辑 key 轮换保持候选所有权。
- `publish_skill(review, packet_digest)` 是独立显式操作。重新核对当前 Publish 权限、审批凭据、当前候选修订、支持来源权威和确切生成包。候选的任何改动都会使旧批准失效，包括修改来源或私有备注。
- 发布通过现有 SQLite 权威事务保存有界、不可变的包字节及来源。不执行外部 I/O，不启动 Worker，不产生半安装包。版本单调递增。当前包保存在 SQLite 中，原始文档字节保存在 Blob 存储。
- `skill_package(skill, version)` 重新检查来源和撤回状态后，只返回获准的清单与文件。分发包不含审阅依据、原始来源 ID、私有备注和批准记录。在来源和支持仍有效时，可以使用精确历史版本。向 Run 追加事件不会改变已批准的不可变前缀。撤销或撤回后拒绝读取。

`SkillPackage` 包含 `manifest`、`files` 和 `digest`。格式 1 清单仅含租户与工作空间、Skill ID 和版本、名称、描述及工件哈希。文件由服务端生成，包含 `SKILL.md` 及最多十六份有界 `references/<lowercase-slug>.md|.txt` 文件。摘要是规范 serde JSON 元组 `(manifest, files)` 的 SHA-256，map 按序编码。`SkillPackage::validate()` 核对精确生成的 frontmatter、允许名单中的文件及全部哈希。实际分发仍须执行实时授权判断。

格式不允许任意 frontmatter、可执行或脚本文件、hook、动态命令、工具预授权或凭据元数据。拒绝未知字段和不支持路径，包括 Windows 设备名。按[客户端契约](https://code.claude.com/docs/en/skills#inject-dynamic-context)，校验器拒绝描述、正文和参考文件中 Claude 的 shell 预处理标记，包括内联和代码块形式。自然语言可建议动作，但不授予执行权限。它也不能通用检测秘密，审阅者仍须保证选定正文和参考内容获授权且已脱敏。离线副本无法立即撤回，批准不授予业务工具权限。

## 客户端边界

导出获准包时重新核对作用域、版本和来源，再写入 Claude Code 或 Codex 项目布局。见 [CLIENTS](../CLIENTS.md)。导出只是显式文件复制。受管加载、回收既有离线副本及候选自动挖掘尚未实现。当前结果见 [VALIDATION](../VALIDATION.md)。私有 Run 引用遵循 [Run 契约](runs.zh-CN.md) 的限制。

## 基于选定经历的起草入口

独立的 [Skill CLI](../SKILL-CANDIDATE-CLI.zh-CN.md) 使用明确选定的一个固定 Run 前缀或精确资产/Learning 版本，以及所有者确认可披露的文本，确定性预填可编辑的私有候选。它复用原候选、审阅及发布接口。选定依据绑定私有备注对象中的修订，在所有者追加候选修订前，提交不同依据会冲突。不自动复制私有支持，不要求模型，保存也不会提交、批准或发布。
