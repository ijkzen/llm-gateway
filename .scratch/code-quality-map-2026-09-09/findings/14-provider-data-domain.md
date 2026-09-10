# FINDINGS · 14 供应商数据管理域审查（2026-09-10）

范围：`availability.rs`（803 全读）、`app_settings.rs`（303 全读）、`provider_model/`（catalog 423 + refresh 203 全读，models.json 结构扫读）、`provider_template/`（mod 409 全读 + seed 27 + 四 chunk 抽查 + tests 699 盘点）、`provider_repo.rs`（200 全读）+ 交叉核对（lib.rs 启动序列、tests/common harness、lb/convert/backup 消费方、dev 库 schema）。方法：两个子代理分域全读 + 主代理对所有保留发现行号磁盘复核。清单模式：不改代码。F1（disabled_reason 字面量）与 F5（协议编号分类法）主审在本票，F8（URL 版本段双实现）顺带核验——**F8 升级为真实功能 bug（14-01）**。

## 汇总

| 编号 | 严重度 | 维度 | 一句话 |
| --- | --- | --- | --- |
| 14-01 | P2【已修复 2026-09-10】 | 逻辑/F8 | `build_models_url` 版本段白名单（仅 v1/v1beta/v1alpha）窄于 `build_upstream_url`（v+纯数字）：9 个 v3/v4 种子供应商（火山×2/腾讯×2/Z.AI×2/智谱×2/Eden）的「刷新模型」拼出 `.../v3/v1/models` 必 404——火山 v3 修复只修了推理侧，刷新侧漏改 |
| 14-02 | P3【已修复 2026-09-10】 | 逻辑/时区 | 首启空库时区分叉：种子行在解析之后才插入，运行期 `inner.timezone=None`（cron 走 chrono::Local）与 `timezone_sync()`（回退 Asia/Shanghai，stats/usage 用）分叉；重启自愈，生产容器 TZ=Asia/Shanghai 掩盖；叠加「同值 PUT 不触发 reload」无法自助修正 |
| 14-03 | P3【已修复 2026-09-10】 | 逻辑/校验 | `max_consecutive_failures` 校验按 i64≥1 无上限（settings.rs:112-121），缓存/加载按 u32 解析（app_settings.rs:131/240）——越界值落库后被静默忽略，库与运行期不一致 |
| 14-04 | P3【观察级保持现状 2026-09-10】 | 测试基建 | 测试 harness 给 scheduler 与 app 各建一个 `AppSettings::default()`（tests/common/mod.rs:60/63/77/176）——时区类行为在测试里对调度器不可见（11-01 未被抓到的根因）；i18n 时区测试只断言 `next_run_at !=` 旧值（变错方向也算过） |
| 14-05 | P3 | 简洁/注释 | convert/mod.rs:246 注释「沿用 build_models_url 的版本段规则」方向说反（build_upstream_url 才是超集），误导后人删 vN 分支 |
| 14-06 | P3【已修复 2026-09-10】 | 死码/F5 | DeriveActiveEnum 死码 ×4：provider_template.rs 的 ProtocolType+BillingMode、virtual_model.rs 的 LoadBalancingStrategy+FallbackStrategy 全仓零消费；SettingType 是唯一真用的 |
| 14-07 | P3【已修复 2026-09-10】 | 健壮/F5 | 协议/接口编号裸范围 `0..=3`/`0..=4`/`!=4` 散落 12+ 处（backup 7 处/providers 2/virtual_models 2/provider_models 1 等）未绑常量；同仓 virtual_models.rs:124 已有常量区间正面样板 |
| 14-08 | P3【已修复 2026-09-10】 | 简洁 | 模板 extra 回填五个 host 专属函数近重复（mod.rs:230-335）；通用回填只在首次插入分支调用（:53），更新分支不调——种子给某 host 的 extra 新增键永远到不了既有 provider |
| 14-09 | P3【已修复 2026-09-10】 | 性能/简洁 | models.json（293KB include_str!）被 CATALOG/RAW 两个 OnceLock 各解析一次各持一份（catalog.rs:81/84/106/127），双份常驻可合一 |
| 14-10 | P3【已修复 2026-09-10】 | 逻辑/数据 | 模板 upsert 仅按 name 匹配且无删除路径：种子改名/删除留孤儿行，`/match` 同 host 返回重复模板；「尊重用户修改」的注释理由不成立（模板无编辑入口，只有 POST /match） |
| 14-11 | P3【已修复 2026-09-10】 | 健壮 | `is_krill_host`/`is_sensenova_host` 用 matches!（大小写敏感），其余五个 is_*_host 用 eq_ignore_ascii_case；当前安全仅靠调用方都过了 `host_of`（已 lowercase） |
| 14-12 | P3【已随 14-01 修复】 | 测试覆盖 | build_models_url 只有 v1/v1beta 用例（refresh.rs:136-167），无 vN/非版本末段——14-01 无回归网；集成刷新测试 base_url 一律 `{mock}/v1` |
| 14-13 | P3【已修复（口径注释+测试）2026-09-10】 | 口径观察 | `recover_quota` 不清 FailureCounter（availability.rs:182-211）与模块头「恢复清零」措辞歧义（enable_manual:267/recover_probe:328 都清）；行为可辩护（额度停用非失败累积）但无注释无测试钉死 |
| 14-14 | P3【观察级保持现状 2026-09-10】 | 简洁/性能 | availability 条件更新样板 ×4（148-177/182-211/215-245/302-336）；`set_items_enabled` 逐行 UPDATE（N 往返，可一次 update_many+is_in）；锁 expect/unwrap 毒化即 panic（临界区极小，观察） |
| 14-15 | P3【已修复 2026-09-10】 | F1/单源 | disabled_reason 字面量仍在 failure_recovery.rs:10/33/235 + persist.rs:246（无 09-08 后新增）；枚举只有 as_str 无 parse/PartialEq，消费方无法回流单源；providers.rs:375-379 已用枚举=正面样板 |
| 14-16 | P3【已随 11-01 修复】 | 注释微 | settings.rs:211 注释「失败时数据库已是新值」措辞误导（settings.update 返回 () 永不失败，指代不明） |

**重复登记（他票已记，本票独立重发现/补充视角，不另编号）**：11-01 时区 reload 顺序（本票补：生产侧 scheduler 与 AppState 同 Arc 实证 lib.rs:145/154 + 测试不可见根因=14-04）；11-09 delete_setting 保护面（本票补：allowlist 键涉出站头透传，删除后旧名单继续生效至重启）。

## 各条证据

### 14-01 build_models_url 版本段判定窄于 build_upstream_url（P2，F8 升级）【已修复 2026-09-10】

refresh.rs:29 `matches!(last, "v1" | "v1beta" | "v1alpha")`，否则一律补 `{default_version}/models`；convert/mod.rs:252-266 的 `is_version_segment` 额外认 `v`+纯数字（注释自带火山 v3 教训）。后果链：provider_models.rs:681 → refresh.rs:48 → 末段 v3/v4 的 base_url 拼出 `.../v3/v1/models` → 404，管理端刷新模型对该供应商**必失败**。受影响种子 9 条（磁盘 grep 实证）：Eden AI（chunk_1.rs:322 `/v3`）、腾讯 Coding/Token Plan（chunk_4.rs:69/76 `/coding|plan/v3`）、火山 Ark ×2（chunk_4.rs:182/189 `/api/v3`、`/api/coding/v3`）、Z.AI ×2 与智谱 ×2（chunk_4.rs:252/259/287/294 `/paas/v4` 系）。推理链路走 build_upstream_url 正常，只有刷新坏；火山 v3 修复（build_upstream_url 认 vN）未回填刷新侧。默认解：版本段判定抽共享 helper，refresh 复用 convert 侧判定，9 家随批修复+补测试（14-12）。附带观察：末段为普通路径段的种子（Upstage `/v1/solar`、Opper `/v3/compat` 等）两个函数都会再叠一层 `/v1`，属种子数据与判定规则的共同错位，图后单独评估。

### 14-02 首启时区分叉（P3，时区）【已修复 2026-09-10】

app_settings.rs:109-156：`load_from_db` 先遍历 setting 表解析（空库 timezone=None）→ 写 TIMEZONE_SYNC=None（:144）→ 构造 Self → **之后** ensure_seed_rows（:154）插入 Asia/Shanghai 种子行。于是首启进程内：cron 链路 `settings.timezone()`=None → chrono::Local（scheduler.rs:583-587），stats/用量链路 `timezone_sync()`=None→回退 DEFAULT_TIMEZONE=Asia/Shanghai（app_settings.rs:262-266）。两口径分叉一个首启生命周期；重启读到种子行即自愈；生产容器 TZ=Asia/Shanghai 掩盖。叠加 settings.rs:190 `timezone_changed = model.value != req.value`：首启后 DB 已是 Asia/Shanghai，同值 PUT 不触发 reload，本地时区解释无法自助修正。默认解：ensure_seed_rows 提到解析之前，或 None 回退统一走 DEFAULT_TIMEZONE。

### 14-03 max_consecutive_failures 越界静默（P3，校验）【已修复 2026-09-10】

settings.rs:112-121 校验 `i64 ≥ 1` 无上限；app_settings.rs:131（加载）与 :240（update）按 `u32` 解析且失败静默保留旧值。DB 存 `5000000000` → 运行期阈值仍是旧值/默认 5，设置页与运行时不一致，重启也不收敛。默认解：校验侧加上限（≤u32::MAX）或与解析统一类型。

### 14-04 测试 harness 双 AppSettings 实例（P3，测试基建）【观察级保持现状 2026-09-10】

tests/common/mod.rs:60（JobWorker::new_with_settings 一个 default）:63（SchedulerRuntime::new_with_settings 另一个）:77/176（build_*_with_settings 又一个）——scheduler 与 HTTP 应用各持独立 AppSettings。生产是同 Arc clone（lib.rs:145/154）。后果：设置 PUT 只更新 app 侧实例，调度器侧永远 default（timezone=None→Local）——时区 reload 类行为测试不可见，11-01 因此未被抓到；i18n_integration.rs:137-184 的时区测试只断言 `next_run_at != before`（方向错误也算变）。默认解：harness 收成一个共享实例（与生产同形），时区测试补「按新时区重建」断言。

### 14-05 convert 注释方向说反（P3，注释）【已随 14-01 修复】

convert/mod.rs:246「沿用 `build_models_url` 的版本段规则」——实际 build_upstream_url 是超集、refresh.rs:21-32 是子集。注释会误导以后者为准删 vN 分支。默认解：14-01 收敛单实现后删注释，或改述为「版本段判定基准」。

### 14-06 死码枚举 ×4（P3，F5）【已修复 2026-09-10】

entity/provider_template.rs:5-16 ProtocolType、:19-26 BillingMode；entity/virtual_model.rs:5-17 LoadBalancingStrategy、:19-26 FallbackStrategy——grep src+tests 仅定义行，零消费。5 个 DeriveActiveEnum 里只有 setting.rs 的 SettingType 真用。默认解：删；或让校验点（providers.rs:140、backup.rs:310 等）改引用枚举 try_from，一举单源。

### 14-07 裸数字范围散落 12+ 处（P3，F5）【已修复 2026-09-10】

backup.rs:310/313/337/353/359/365/390（含 `!= 4` 魔术值）、providers.rs:140/146、virtual_models.rs:140/146、provider_models.rs:186 等用 `0..=3`/`0..=4` 裸范围；同仓 virtual_models.rs:124-125 已用 `INTERFACE_OPENAI_COMPAT..=INTERFACE_FULL_COMPATIBLE` 常量区间（正面样板）。新增编号时裸范围静默漏改（如 interface 5 被备份校验误拒）。默认解：收敛常量区间全量替换。

### 14-08 模板 extra 回填重复 + 更新分支不回填（P3，简洁）【已修复 2026-09-10】

provider_template/mod.rs:230-335 五个 host 专属回填函数（krill/sensenova/siliconflow/agentrouter/tokenrhythm）结构一致走同一 backfill_host_extras 管线；通用版 backfill_provider_extra（:340-370）只在 upsert 插入分支调用（:53），更新分支不调——种子模板新增 extra 键时既有 provider 拿不到（:244-249/:289-295 注释正是此坑的补丁史）。默认解：启动时对全部模板统一跑「按 host 补缺」通用回填，与插入分支解耦，五个专属函数被覆盖后删除。

### 14-09 models.json 双解析（P3，性能/简洁）【已修复 2026-09-10】

catalog.rs:81/84 两个 OnceLock、:106/:127 各 `serde_json::from_str(MODELS_JSON)`，293KB 解析两遍、常驻两份（尾段索引 + 全量 Vec）。懒加载非热路径，量微。默认解：解析一次，索引由 Vec 派生。

### 14-10 模板 upsert 孤儿行（P3，逻辑/数据）【已修复 2026-09-10】

provider_template/mod.rs:26-55 仅按 name 匹配 upsert，无删除路径：种子改名→旧行+新行并存（/match 同 host 返回重复模板），种子删除→行永不清理。:19 注释「不会删除用户手动添加或修改的记录」——但模板无用户编辑入口（routes/provider_templates.rs 只有 POST /match），理由不成立。默认解：upsert 后按种子名单清理不在名单内的行。

### 14-11 is_*_host 大小写不一致（P3，健壮）【已修复 2026-09-10】

provider_template/mod.rs:67-80 krill/sensenova 用 matches!（大小写敏感），:83-105 其余五家用 eq_ignore_ascii_case。当前生产调用方（usage/mod.rs、calls.rs）都先过 host_of（:386 lowercase）故安全；pub(crate) 谓词被直传原始 host 会静默漏判。默认解：统一 eq_ignore_ascii_case。

### 14-12 refresh URL 测试缺口（P3，测试覆盖）【已随 14-01 修复】

refresh.rs:136-167 只覆盖 v1/v1beta/裸 host 补默认；集成刷新测试 base_url 一律 `{mock}/v1`。14-01 的 9 家 vN 供应商无回归网。默认解：补 `build_models_url(".../api/v3") → .../api/v3/models` 断言 + 至少一家 vN 集成刷新。

### 14-13 recover_quota 不清失败计数（P3，口径观察）【已修复（口径注释+测试）2026-09-10】

availability.rs:182-211 recover_quota 不接收 FailureCounter；enable_manual（:267）与 recover_probe（:328）清零。模块头「恢复…必须清零」可被读作含额度恢复；行为可辩护（额度停用期间无流量，失败计数是停用前真实累积，保留=不掩盖供应商质量问题）但无注释无测试钉死。默认解：模块头/ADR-0003 补一句口径 + 一条锁定测试。

### 14-14 availability 样板与逐行 UPDATE（P3，简洁/性能）【观察级保持现状 2026-09-10】

条件更新样板 ×4（update_many + rows_affected 判断 + set_items_enabled + 日志：148-177/182-211/215-245/302-336）；set_items_enabled（:89-143）逐行 active.update，N 条目 N 往返（SQLite 本地影响小）；FailureCounter/静态锁 expect/unwrap 毒化即 panic（临界区无 await 极小，观察级）。默认解：样板抽小助手；set_items_enabled 改 update_many+is_in；锁处理保持现状。

### 14-15 F1：disabled_reason 单源无反向接口（P3，F1 定案）【已修复 2026-09-10】

单源 availability.rs:39-47 as_str 属实；手写点与 01 盘点一致无新增：failure_recovery.rs:10（eq("failure")）/:33（== Some("failure")）/:235（测试种子）、persist.rs:246（matches! None|Some("quota")）；db.rs:463-464 迁移 SQL 字面量合理；providers.rs:375-379 已用枚举（正面样板）。根因：枚举只有 as_str 没有 parse/PartialEq<str>，消费方只能手写。默认解：补 `DisabledReason::parse(&str)` 或 PartialEq<str>，替换 4 处生产点。

### 14-16 settings.rs:211 注释措辞（P3，微）【已随 11-01 修复】

注释「失败时数据库已是新值，缓存以数据库为准，不阻塞响应」位于 Ok 分支内、且 `settings.update` 返回 () 永不失败——指代不明易误读。默认解：重写注释或删后半句。

## P3 实施批（2026-09-10）

**分类法单源（F1/F5 家族落地）**：
- **14-06 已修复**：删除 `ProtocolType` / `BillingMode`（provider_template）与 `LoadBalancingStrategy`（virtual_model）三个零消费枚举，改为同处声明的编号常量（`BILLING_MODE_*` / `LB_*`）；`FallbackStrategy` 保留（02-10 批已由 route.rs 真实消费）。
- **14-07 已修复**：12 处裸范围（`0..=3` / `0..=4` / `!= 4`）改引常量区间——backup.rs 4 处、providers.rs、provider_models.rs、virtual_models.rs；新增编号时不再静默漏改。
- **14-15 已修复**：新增 `DisabledReason::parse` 与 `PartialEq<str>` 反向接口；4 处生产手写字面量（failure_recovery 两处、usage/persist 的候选过滤、边界探活谓词）改为经枚举解析。

**模板与回填**：
- **14-08 已修复**：模板 extra 回填与插入/更新分支解耦——`upsert_templates` 对每个种子模板都跑 `backfill_provider_extra`，存量 provider 也能拿到模板新增的 extra 键（原先只在首次插入分支回填）。测试 `sensenova_history_backfill_*` 的「非 SenseNova host 不动」断言随批改述为「不串入 SenseNova 专属键」（该 provider 落在 DeepSeek 模板 host 上，按新语义会补缺）。
- **14-10 已修复**：`upsert_templates` 末尾按种子名单清理残留行（种子改名/删除不再新旧并存）；清理条数记 info 日志。
- **14-11 已修复**：`is_krill_host` / `is_sensenova_host` 由 `matches!`（大小写敏感）改为 `eq_ignore_ascii_case`，与其余五家谓词一致。

**其他**：
- **14-02 已修复**：种子行插入提到解析之前（`load_from_db` 首步调 `seed_rows`）——首启进程内不再出现「cron 链路用本地时区 / 统计链路用默认时区」的分叉。
- **14-03 已修复**：`max_consecutive_failures` 校验补上界（1..=u32::MAX）；backup 集成测试的文案断言随批更新（0 仍被拒，语义不变）。
- **14-09 已修复**：models.json 只解析一次——`catalog()` 复用 `raw()` 的结果（`RawModelFull` 为超集），删除已无用的 `RawModel` 结构；293KB 嵌入 JSON 不再解析两遍。
- **14-12 已随 14-01 修复**：vN 版本段回归（含 `/api/v3`、`/api/coding/v3`、`/api/paas/v4`、`/v2/` 与非版本段对照）已随 14-01 落地。
- **14-13 已修复（口径注释 + 测试）**：模块头明确「`recover_quota` 不清零失败连击计数」的理由（停用期间无流量、计数反映停用前真实累积），`recover_quota` 文档同步；新增两条对照测试（`recover_quota_keeps_failure_counter` / `enable_manual_clears_failure_counter`）。
- **14-16 已随 11-01 修复**：该处注释已在时区重载顺序修复时改写清楚。

**观察级保持现状**：14-04（测试 harness 双 AppSettings 实例——测试基建观察项，未影响生产）、14-14（availability 样板与逐行 UPDATE——SQLite 本地量微）。

## F1/F5/F8 家族定案

- **F1**：单源存在但无反向接口 → 绕行依旧（4 生产点+1 测试种子，无新增）；定案=补 parse/PartialEq 后全量替换（14-15）。
- **F5**：死码枚举 ×4 确认（14-06）；裸数字范围 12+ 处确认且有同仓常量区间正面样板（14-07）；编号语义对齐无错位（协议 0..3==接口 0..3、4=Full Compatible 在 backup/virtual_models/迁移 23 三处一致）。
- **F8**：从「双实现+注释误导」升级为真实功能 bug（14-01，9 家已上线种子刷新必 404）+ 注释反向（14-05）；定案=版本段判定收敛单实现。

## 已核验无问题区（避免后续票重复审查）

- **可用性状态机与 ADR-0003 对齐无漂移**：四值语义/镜像不变式由动作单点写入（availability.rs:148-336）；五动作条件守卫正确（disable_for_quota 仅 enable=true、recover_quota 仅 quota、disable_for_failures 仅 NULL、recover_probe 事务内 CAS、manual 双动作幂等不覆盖来源）；并发 disable 经 SQL 条件原子化互不覆盖。
- **级联分层 cascade_disabled**：停用只动启用项并打标、恢复只取带标项、手动关闭成员不动（:104-132）。
- **读侧谓词唯一**：traffic_available（:54-56）全仓无绕过，lb.rs:152/forward.rs:169 统一经谓词。
- **AppSettings 写序**：路由先落库后刷缓存（settings.rs:209-213），backup 导入顺序正确；ensure_seed_rows find+insert 幂等；全仓 setting::Entity::find 无业务热路径绕过缓存直读。
- **模板种子与回填**：upsert_templates 幂等（插/更/回种子值测试在）；backfill_host_extras 只补缺不覆盖（entry.or_insert，五 host 测试逐条锁定：未知键保留/已填凭据不覆盖/子域不误伤/坏 JSON 跳过）；加密迁移先于种子（lib.rs:108→:113）；provider_repo 加密迁移幂等（3 测试）。
- **host_of/模板匹配**：去协议/路径/端口/占位符正确（测试在）；find_by_domain_all 大小写+路径+多命中覆盖。
- **catalog**：尾段匹配含 models/ 前缀；相似度长度剪枝数学安全；Levenshtein 双行滚动实现正确。
- **无反向依赖**：provider_model/template/repo 只依赖 entity/crypto/db，被 proxy/usage/routes/lib 单向消费。
- **锁安全**：FailureCounter std Mutex 临界区无 await；AppSettings RwLock 读路径短。

## 测试覆盖盘点

已有：availability 13 单测（含阈值熔断/CAS/五态矩阵）+ provider_template tests.rs 21 用例（upsert/回填五家/host_of/匹配/默认头）+ catalog 15 + refresh 6 + provider_repo 3 + 交叉集成（quota_gate/boundary_probe/failure_recovery/settings/i18n）。缺口：14-04（harness 双实例+时区断言方向）/14-12（vN URL）/availability 并发竞态与 threshold=1/app_settings update·load_from_db·sync 双写无单测（仅 parse_header_allow_list 3 例）/pi_user_agent 平台映射与 kernel_release 回退/is_*_host 大小写/种子改名孤儿（14-10）/通用回填仅一处间接覆盖。

## 性能/内存轮结论

无 P1/P2。AppSettings 读锁短路径（lang/timezone/allowlist 每请求级频率可接受）；FailureCounter 无热点；模板匹配 186 行全扫+逐行 host_of 仅限管理端 /match；models.json 编译期内嵌懒解析不进 /v1 热路径（14-09 双份常驻量微）；启动 upsert 186 模板约 372 次查询+五回填各一次全表扫为一次性成本可接受。唯一动作项=14-14 的 set_items_enabled 批量化（可选）。

## 实施进度

- **14-01 已修复**：版本段判定收敛为单实现 `provider_model::refresh::is_version_segment`（`v1`/`v1beta`/`v1alpha` 或 `v`+纯数字），`build_models_url` 与 `convert::build_upstream_url` 共用；9 个 v3/v4 种子供应商（火山×2/腾讯×2/Z.AI×2/智谱×2/Eden）刷新模型不再拼出 `.../v3/v1/models`。测试 `test_build_models_url_recognizes_vn_version_segments`（火山 `/api/v3`、`/api/coding/v3`、智谱 `/paas/v4`、`/v2/`、非版本段 `/v1/solar` 反向用例）。
- **14-05 已随批修复**：`convert/mod.rs` 的注释改为指向共享判定（不再自称「沿用 build_models_url 的规则」，方向反了的问题消除）。
