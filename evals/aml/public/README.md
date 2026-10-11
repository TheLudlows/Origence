# 公开 LongMemEval 本地准备（尚未评测）

已从[作者发布的清洗版数据](https://huggingface.co/datasets/xiaowu0162/longmemeval-cleaned)下载固定 revision 98d7416c24c778c2fee6e6f3006e7a073259d48f 的 longmemeval_s_cleaned.json。作者该版本 README 声明 MIT、英语；实际文件 277383467 bytes，SHA-256 已与发布元数据一致核对。完整数据保存在 target/aml-public，未加入 Git。

[准备收据](preparation.json)记录版本、哈希、许可和 32 个预选 question ID：按 SHA-256(Origence-public-test-v1: + question_id) 排序取前 32，不读取答案或运行结果来挑选。全部作为外部 test-only 子集，与自建开发调参分开。已查看字段结构与题型集合，没有模型调用或质量成绩。

后续先验证会话边界、时间格式、用户隔离、超长消息与公共评分接口，检查共享历史的分组统计，再冻结执行参数和语义判分。公开题不等于隐藏测试，不能保证模型未在训练中见过；不把 32 题子集称为完整 LongMemEval 或官方 AML。许可按作者数据卡记录，不替代其他上游材料的条款核对。
