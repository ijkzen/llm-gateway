# 14 · 供应商数据管理域审查

Type: task
Status: open
Blocked by: 01

## Question

对供应商数据管理域做全量审查：`provider_model/`、`provider_template/`（模板默认头/接口类型）、`provider_repo.rs`、`availability.rs`（disabled_reason 状态机）、`app_settings.rs`（设置缓存热生效）及域内测试。按四轴 + 专门性能轮产出分级清单（不改代码）：

- 逻辑正确性：模板种子与版本迁移、可用性状态机迁移历史对齐（09-06 ADR 0003 后有无漂移）、设置缓存失效面；
- 实现简洁：模板/模型/repo 三层的关系与重复（seed 模板迭代器访问改造后有无残余）、谓词拷贝；
- 测试覆盖：provider_repo/provider_template/availability 单测之外缺什么；
- 模块间调用：与 11 CRUD、usage 门控、proxy headers/convert 的口径一致性（此域被多方消费，重点查反向依赖）。

产出 `.scratch/code-quality-map-2026-09-09/findings/14-provider-data-domain.md`，Answer 给摘要与需拍板问题。
