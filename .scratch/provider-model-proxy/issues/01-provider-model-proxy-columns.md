# 01: provider_model 表新增模型级代理字段（迁移 19 + 实体）

**What to build:** 供应商模型（provider_model）获得与供应商同构的模型级网络代理字段 `proxyEnabled`/`proxyAddr`：数据库可存、实体可读写。存量模型行默认关闭（回落供应商代理），新库建表直接带这两列，历史库兜底 ALTER。这是后端所有后续改动（路由读写、转发管线装配）的数据基础。

**Blocked by:** None (can start immediately)

**Status:** ready-for-agent

- [ ] `provider_model` 表增加 `proxy_enabled`（boolean NOT NULL DEFAULT '0'）与 `proxy_addr`（varchar NOT NULL DEFAULT ''）两列；迁移从版本 19 起编，沿用 column_exists 逐列检测 + 单次 ensure_migration 的既有写法
- [ ] 新库 create_table_from_entity 建出的表带这两列；历史库 migrate() 兜底补齐（有回归测试覆盖列存在）
- [ ] provider_model 实体结构体含两个新字段，与 provider 实体同构（serde camelCase）
