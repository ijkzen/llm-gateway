# 07 · usage 持久化与额度门控审查

Type: task
Status: claimed
Blocked by: 01

## Question

对 usage 持久化与门控域做全量审查：`types.rs` / `persist.rs`（缓存写读/全量刷新/apply_usage_gate/probe_boundary_providers/usage_refresh handler）/ `mem_cache.rs` 及域内测试。按四轴 + 专门性能轮产出分级清单（不改代码）：

- 逻辑正确性：10 分钟新鲜度单一判定是否真单一、原子 upsert 边界、单飞去重、停用/恢复级联、边界实测探活（09-08 E7 已修项不复查）；
- 实现简洁：判定谓词是否有第二份拷贝、缓存双写面；
- 测试覆盖：quota_gate/boundary_probe 集成之外缺什么（并发刷新、缓存过期竞态单测）；
- 模块间调用：与 06 抓取层、proxy usage_rank、cron seed、availability 停用域的口径是否一致。

产出 `.scratch/code-quality-map-2026-09-09/findings/07-usage-persist-gating.md`，Answer 给摘要与需拍板问题。

## Answer

**清单产出**：`findings/07-usage-persist-gating.md`——5 条全 P3 无 P1/P2。方法=types/persist/mem_cache 三文件全读 + availability.rs 门控动作面全读与测试盘点 + 消费方交叉核对（lb 三层读路径/routes ?refresh=1/virtual_models rank/failure_recovery/failure_recheck/lib.rs handler）+ 谓词与新鲜度判定全仓 grep 查拷贝。

**核心结论**：本域是图内最干净的域之一——**新鲜度判定真单一**（cache_age_fresh* 全仓唯一实现，DB 单读/批读/mem 读全复用）；**原子 upsert 无回归**（E7 单语句 ON CONFLICT + 并发单测）；**级联/幂等/manual-failure 守卫**在 availability 状态机完备（15 测试含 manual 不被额度刷新触碰、recover 只解 quota 态、recover_probe 乐观锁）；**谓词单源**（门控/LB 排序/边界探活三消费方共用 types.rs 访问器，无第二份推导）；恢复双通道与 ADR-0010 语义自洽。5 条新发现集中在三处：
- **抓取入口无跨调用单飞 + 失败无负缓存**（07-01）：cron/手动/LB/failure_recheck 四路可并发重复抓；持续故障期 LB 每请求真实厂商调用（DB 写失败期含已计费调用放大）——E7 修的是分类不涉放大。默认解=四路收敛 mem.fetch_shared 单飞入口或 mem 层失败负缓存。
- **缓存双写面不对称**（07-02）：cron 刷新只写 DB，mem 持旧数据到自身 TTL——「刚刷新完 mem 反而返回旧一轮」。默认解=刷新成功回填/失效 mem。
- **测试缺口**（07-04/05）：mem 失败/取消路径（waiter 不悬挂的 Cleanup guard）零测试、read_usage_cache_many 全仓零测试、probe_boundary_providers 零单测（候选过滤矩阵仅靠集成 4 场景）。
- 另有 07-03（探活顺序执行无总时限，多家边界拖垮 5 分钟周期，try_lock 自愈）。

**需拍板问题**：无。本票无 P1/P2、无行为口径分裂点（探活/恢复语义已拍板过不重问；5 条默认解均明确直接落清单）。

Status: resolved
