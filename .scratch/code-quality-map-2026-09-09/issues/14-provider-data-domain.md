# 14 · 供应商数据管理域审查

Type: task
Status: claimed
Blocked by: 01

## Question

对供应商数据管理域做全量审查：`provider_model/`、`provider_template/`（模板默认头/接口类型）、`provider_repo.rs`、`availability.rs`（disabled_reason 状态机）、`app_settings.rs`（设置缓存热生效）及域内测试。按四轴 + 专门性能轮产出分级清单（不改代码）：

- 逻辑正确性：模板种子与版本迁移、可用性状态机迁移历史对齐（09-06 ADR 0003 后有无漂移）、设置缓存失效面；
- 实现简洁：模板/模型/repo 三层的关系与重复（seed 模板迭代器访问改造后有无残余）、谓词拷贝；
- 测试覆盖：provider_repo/provider_template/availability 单测之外缺什么；
- 模块间调用：与 11 CRUD、usage 门控、proxy headers/convert 的口径一致性（此域被多方消费，重点查反向依赖）。

产出 `.scratch/code-quality-map-2026-09-09/findings/14-provider-data-domain.md`，Answer 给摘要与需拍板问题。

## Answer

**清单产出**：`findings/14-provider-data-domain.md`——16 条（1 P2 + 15 P3）+ F1/F5/F8 三家族定案。双子代理分域全读（availability+app_settings / provider_model+template+repo），所有保留发现经主代理行号磁盘复核。

**唯一 P2 = 14-01（F8 升级）**：`build_models_url` 版本段白名单（仅 v1/v1beta/v1alpha，refresh.rs:29）窄于 `build_upstream_url`（v+纯数字，convert/mod.rs:252-266）——9 个 v3/v4 种子供应商（火山×2/腾讯×2/Z.AI×2/智谱×2/Eden）的「刷新模型」拼出 `.../v3/v1/models` 必 404；火山 v3 修复只修了推理侧没回填刷新侧。默认解=版本段判定抽共享 helper 收敛单实现 + 补 vN 回归测试（14-12 现无回归网）。属管理端功能必坏、无数据损坏，P2。

**三家族定案**：F1=枚举单源无 parse/PartialEq 反向接口致绕行依旧（failure_recovery×3+persist×1，无 09-08 后新增；默认解=补接口全量替换）；F5=死码枚举 ×4（ProtocolType/BillingMode/LoadBalancingStrategy/FallbackStrategy 零消费）+裸数字范围 12+ 处（同仓 virtual_models.rs:124 常量区间正面样板）；F8=升级为 14-01 真实 bug + convert 注释方向说反（14-05）。

**其余 P3 要点**：14-02 首启空库时区分叉（种子行在解析后插入→cron 走 Local vs timezone_sync 走 Asia/Shanghai，重启自愈、生产 TZ 掩盖）/14-03 max_consecutive_failures i64 校验 vs u32 解析越界静默/**14-04 测试 harness 给 scheduler 与 app 各建独立 AppSettings（11-01 测试不可见的根因）**/14-08 模板 extra 回填五份近重复+通用回填只在插入分支/14-10 模板 upsert 仅按 name 无删除路径留孤儿（「尊重用户修改」理由不成立——模板无编辑入口）/14-13 recover_quota 不清失败计数口径歧义待钉死/14-14 availability 样板×4+set_items_enabled 逐行 UPDATE/11-01 与 11-09 被独立重发现（不重复编号）。

**已核验无问题区**（9 项）：ADR-0003 状态机无漂移、五动作守卫+级联分层、读侧谓词唯一无绕行、AppSettings 写序与种子幂等、无热路径绕过缓存直读 setting 表、模板回填只补缺语义测试齐、加密迁移先于种子、无反向依赖、锁安全。

**需拍板问题**：无（14-01 修复方向明确无取舍）。

Status: resolved
