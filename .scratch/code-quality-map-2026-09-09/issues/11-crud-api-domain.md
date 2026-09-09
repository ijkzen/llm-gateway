# 11 · CRUD API 域审查

Type: task
Status: open
Blocked by: 01

## Question

对配置管理 CRUD API 域做全量审查：`routes/providers.rs` / `provider_models.rs` / `virtual_models.rs` / `cron_jobs.rs` / `settings.rs` / `openai_compat.rs` + `routes/mod.rs` 组装及域内测试。按四轴 + 专门性能轮产出分级清单（不改代码）：

- 逻辑正确性：级联删除/停用/刷新语义、用量预估读侧（645bca1 固化后的信任边界）、设置类型校验、路由守卫遗漏面、竞态（双请求同改）；
- 实现简洁：三个 800-970 行大文件的重复结构（详情/列表/刷新 handler 拷贝）、错误映射面；
- 测试覆盖：CRUD 集成之外缺什么（非法输入矩阵、级联边界）；
- 模块间调用：直写调度器/仓库层/availability 的面是否收敛（C4 已收口不复查）、openai_compat 与 proxy 门面的边界。

产出 `.scratch/code-quality-map-2026-09-09/findings/11-crud-api-domain.md`，Answer 给摘要与需拍板问题。
