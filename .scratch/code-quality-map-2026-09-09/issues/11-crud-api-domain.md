# 11 · CRUD API 域审查

Type: task
Status: claimed
Blocked by: 01

## Question

对配置管理 CRUD API 域做全量审查：`routes/providers.rs` / `provider_models.rs` / `virtual_models.rs` / `cron_jobs.rs` / `settings.rs` / `openai_compat.rs` + `routes/mod.rs` 组装及域内测试。**01 盘点补漏：范围含 `api_keys.rs` / `request_logs.rs` / `provider_templates.rs` / `backup.rs` / `chat.rs` 五个路由文件**（request 表直查原始 SQL 家族 F9 主审在此，见 MODULES.md §2）。按四轴 + 专门性能轮产出分级清单（不改代码）：

- 逻辑正确性：级联删除/停用/刷新语义、用量预估读侧（645bca1 固化后的信任边界）、设置类型校验、路由守卫遗漏面、竞态（双请求同改）；
- 实现简洁：三个 800-970 行大文件的重复结构（详情/列表/刷新 handler 拷贝）、错误映射面；
- 测试覆盖：CRUD 集成之外缺什么（非法输入矩阵、级联边界）；
- 模块间调用：直写调度器/仓库层/availability 的面是否收敛（C4 已收口不复查）、openai_compat 与 proxy 门面的边界。

产出 `.scratch/code-quality-map-2026-09-09/findings/11-crud-api-domain.md`，Answer 给摘要与需拍板问题。

## Answer

**清单产出**：`findings/11-crud-api-domain.md`——27 条（3 P2 + 24 P3）+ 两项拍板 + F9 主审定案。方法：三个子代理分文件族逐行全读（providers+provider_models / virtual_models+cron_jobs+settings / 六个小路由+mod），所有保留发现的行号锚点经主代理磁盘复核。

**三张 P2（均无数据损坏、无即时拍板需要，随实施批）**：
- 11-01 时区变更时 `reload_all_jobs` 在 `settings.update` 之前执行，调度器用旧时区重建（settings.rs:190-213 vs scheduler.rs:321/582；代码与自身注释矛盾，backup.rs:73-84 顺序正确可对照）。**当前零可见影响**（四个种子任务全为 @every/@hourly 时区不敏感 + 无创建任务 API），一旦出现 tz 敏感定点 cron 即升 P1。默认解=顺序对齐 backup.rs。
- 11-02 用量缓存失效与在途抓取竞态无版本护栏：在途抓取可在失效后回写旧凭据用量，脏缓存最长活一个 TTL，自愈无损坏。
- 11-03 request_logs 分页 offset u32 相乘溢出（page>~4290 万：dev panic→500 / release 静默错页），一行修。

**F9 主审定案**：request 表直查全部无注入面（request_logs 全参数绑定+排序白名单；providers.rs:927-937 的 format! 形态已不存在、现为参数绑定——01 票登记的该实例自然消亡；stats 侧 format! 只插 i64 时间戳与常量）。「三套 WHERE 构造器并存无共享 helper」属实但属简洁收敛项；实质缺口已单列（11-03/11-15/11-16/11-26）。

**两项拍板（用户当场拍定，结论锁定现状）**：
- 拍板一：创建/更新虚拟模型**允许**加已停用供应商的模型为成员（成员关系≠可用性，选路 traffic_available 剔除兜底，创建侧不加校验）。
- 拍板二：全局 5MB body 上限**保持现状**（DoS 兜底，接受巨型多模态 /v1 请求 413，真实需求出现再议放宽）。

**P3 要点**（详见 findings）：多步写非原子×2（11-04 update_provider 字段已落库动作失败报 500 / 11-06 cron 先落库后改内存失败不回滚）；未加载 cron 任务无法软删除（11-07，行永远删不掉）；校验缺口族（11-12 custom_header 非对象可落库被静默忽略 / 11-13 备份导入绕过 apiKey 必填 / 11-14 备份直写 enable⇔disabled_reason 不变式无校验 / 11-08 Bool 不 trim）；缓存面（11-09 删非保护设置键不刷新缓存 / 11-21 备份导入不清用量缓存孤儿行）；i18n 遗漏（11-05）；错误码不一（11-10）；边界（11-11 嵌套列表 200 空数组有意注释 / 11-17 尾斜杠 401 / 11-18 未知 API 返 HTML 200）；简洁（11-19 死变量 / 11-20 not_found 样板×7）；性能（11-23 无分页列表 / 11-24 load_usage_map N+1 有现成批读可换 / 11-25 用量端点抓取无单飞=07-01 同解 / 11-26 modelId 过滤与两排序列无索引）；观察（11-27 ProviderResponse 缺 disabled_reason）。

**已核验无问题区**（11 项，详见 findings）：级联删除事务、ADR-0003 单一 owner、密钥面（掩码/无明文回写/extra 教训防线）、缓存失效成对、协议级联、645bca1 预估信任边界、cron 未加载 400 约定+日志端点 404+SSE 无丢窗、settings 三层校验+缓存热生效、路由守卫全貌（/v1 Bearer 在 auth/mod.rs 非 mod.rs）、openai_compat/chat/templates 薄而干净、批量去重/重排原子性/N+1 基本规避。

**测试缺口** 14 类（T1-T14，正对 11-01/11-02/11-03/11-07/11-08/11-09/11-12/11-13/11-14/11-15/11-16 与 SSE 后端零集成、i18n 英文分支裸面等，详见 findings）。

**需拍板问题**：无遗留（两项已在票内当场拍定）。

Status: resolved
