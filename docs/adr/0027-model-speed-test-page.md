# 0027 — 模型测速页（TPS 基准）

## Status

accepted

## Context

供应商详情的「模型测速」弹窗（`ProviderSpeedTestDialog`）本质是**连通性检查**：提示词固定「你好」、非流式、每行手动点、前端 60 秒超时，只回一个总耗时。它回答不了「这个模型吐字有多快」，也无法横向比较——要比较就得一排一排手点，而且结果只活在前端组件里，切页面即消失。

需要一个独立的基准测试入口：长文请求、流式采样、批量执行、结果在服务端可回看。设计时撞上四个真正需要解释的取舍。

## Decision

### 1. 结果落库，只留最近一次运行

新建 `speed_test_run`（运行元信息）与 `speed_test_result`（每模型一行）两表（迁移 29）。**单行语义**：开始新运行即删除旧运行与其结果行，页面永远只有两种形态——「正在跑」或「上一次的成绩」，不存在「新一次在跑但旧成绩还挂着」的叠加态。

否决的备选：内存单槽位（仿飞书扫码的 `RegistrationSlot`）。逐行原始数据确实已在 `request` 表里，但 `request` 有保留期清理，且「上一次结果」的界面便利不该依赖它的存活。

结果行**冗余供应商名与模型名快照**（而非只存 `request_id` 外键）：页面读一张表即可渲染，供应商改名/删除后旧成绩仍可读；`request_id` 仍保留，用于跳转请求日志审计。

`request` 表本身不再承载「这次跑得怎么样」——行级 tps 无法表达运行状态、候选总数与失败归类。

### 2. 复用 `api_key_name='test'` 标记，不入统计、不计熔断

测速流量与自动恢复探测、边界实测共用同一标记，因此**零改动**就继承了 ADR-0024 的排除口径（12 个 stats 端点 + 快照统一排除）。若改用新标记（如 `speedtest`），必须改 `base_predicate` 并递增 `PREDICATE_VERSION` 触发快照全量重算，而测速的长文请求（2048 tokens 输出）会把成功率/图表/排行彻底带偏。

为在请求日志里区分来源，`request_id` 用 `speedtest-` 前缀（探测是 `test-`、后台聊天是 `chat-`）。

测速**不触碰可用性状态机**：不调 `note_member_failure`、不发飞书通知。一次批量测速会把多个上游打一遍，若计入熔断，一个配置错误的模型就可能把整批供应商打成「连续失败禁用」并连发通知——测速是诊断动作，不该改变生产状态。

### 3. 开始即写全部 pending 占位行

运行开始时就把所有候选模型写成 `status='pending'` 的结果行，逐行转 `done`/`failed`，取消时剩余行转 `cancelled`。

好处有两个：行数从第一刻就是候选总数，「已完成 7 / 23」的分母天然正确；「没参与本次过滤」与「参与但失败」在界面上可区分（前者根本没有行，后者是 `failed` 行）。取消时也能一眼看到哪些没跑。

### 4. 四态运行 + 启动恢复

状态词表：`running` / `completed` / `cancelled` / `interrupted`。四态而非三态，是为了把「管理员主动取消」与「服务重启打断」分开——排障时这两种情况含义完全不同。启动时把残留的 `running` 改判 `interrupted`（与 cron 执行日志的启动恢复同款做法），已完成的结果行保留。

同时 `GET /api/speed-test/run` 额外返回内存槽位的 `running` 标志：进程刚重启、库还没改判的瞬间，前端以内存真值为准，不会看到一个进度永远不动的「测速中」。

## Consequences

- 测速结果与生产流量统计彻底隔离；请求日志仍可审计每一条测速请求（含失败原因）。
- 单实例 + 可取消，**无并发上限、无同供应商串行、不设 `max_tokens` 上限、不设单模型硬超时**（见下方补记）。
- TPS 口径与 `request.tps` **完全同源**（`RequestRecord::tps_value`），不引入第三套定义：流式 `output_tokens / (ttft + 输出耗时)`，含首字延迟。TTFT 单列一列供想看纯生成速度的人自行推算。
- 失败判定只看传输层（非 2xx / 网络错误 / 超时）。输出 token 偏短不算失败，只在表格里标「样本不足」——把「模型话少」和「模型坏了」混为一谈会让诊断信息失效。
- 汇总条的平均 TPS 用**算术平均**（不是数据面板的 token 加权）：这里回答「这批模型平均多快」，与表格主排序一致；加权平均会被单个超快模型拉高，与看到的中间值对不上。
- 现有弹窗保留并改名「连通性测试（时延）」，与新页形成两个明确定义：短 prompt 时延 vs 长 prompt TPS 基准。

## 补记（上线后实测修正）

初版把测速输出预算夹到 `[512, 2048]`、单模型硬超时 180 秒，并只把 `content` / `reasoning_content` 计入首 token。上线后在生产发现 OpenDesign 的 `deepseek-v4.1-flash` 测速行 `output_tokens=2048`（正好等于上限）但 `ttft` 与 `tps` 全空。

直接抓该上游原始 SSE 得到三个事实：

1. 它用 **`reasoning`** 作为思考增量字段名（不是 `reasoning_content`），而我们只认后者；
2. 面对测速的长文题面，它把整个 2048 预算烧在思考上，**正文 `content` 一个 token 都不出**，`finish_reason` 也不出现（被预算硬截断）；
3. 4096 预算下 240 秒仍无正文；而短问题 + 4096 预算 11 秒就能出正文并正常收尾。

即：**预算上限本身反而抑制了正文产出**，而硬超时会把「能出数」变成「超时失败」。据此修正两条：

- **首 token 口径**：`chunk_has_content` 与 OpenAI 旁路扫描器同时接受 `reasoning`，`strip_reasoning_delta` 两种字段名都剥。这使直通上游与三种转换协议（产出 `reasoning_content`）在思考字段上统一——代价是这些模型的 TTFT 从「首个正文 token」变为「首个思考 token」，数值变小但语义更准。
- **预算**：`max_tokens` 直接用 `provider_model.max_output_tokens`（未登记时兜底 2048），取消单模型硬超时，由上游客户端自身的连接/读超时兜底。代价是推理模型单次可能跑数分钟。
- **并发**：初版有全局 `Semaphore(4)` 与「同一供应商串行」两道闸，按用户要求**全部取消**——所有候选模型同时开测。代价必须认清：同一把 API Key 的多个长请求会并发打上游，容易撞到厂商限流，此时测到的是限流后的速度而非模型真实吐字速度；批量几十个模型时也会瞬时对上游造成压力。相应地，取消语义从「把排队中的行标为已取消」退化为「把尚未开始的行标掉（通常一行都没有），在飞的自然结束」，运行本身仍按 `cancelled` 收尾。

回归：`proxy::tests::chunk_has_content_counts_reasoning_alias`、`proxy::tests::strip_reasoning_delta_removes_both_field_names`、`proxy::convert::openai::tests::scanner_detects_reasoning_and_tool_content`、`speedtest::tests::target_max_tokens_uses_registered_value_without_cap`。

实现：`src/speedtest/`（`candidate` 候选与过滤单一事实源、`bench` 流式采样、`runner` 运行控制、`store` 读写）、`src/routes/speed_test.rs`、`src/entity/speed_test_{run,result}.rs`；前端 `web/src/pages/speed-test.tsx` + `web/src/components/speed-test/`。回归：`tests/speed_test_integration.rs`。

### 补记二：候选集先行展示

初版只在「点开始测速」后才把候选变成结果行，用户选完筛选只能看到一个数字，得先点按钮才知道会测谁。改为候选接口直接返回完整模型清单（供应商 + 上游模型 + 引用它的虚拟模型），页面三态渲染：

1. **测速中** → 进度与逐行结果（改筛选也不打断，优先展示）；
2. **结果属于当前筛选** → 展示成绩。判定依据是运行行里的筛选快照（`provider_ids`/`vm_ids`/`keyword`）与当前 URL 筛选逐项比对，id 按集合比较；
3. **其余** → 展示候选清单。

第 2 条的比对是这次改动的关键：没有它，用户改完筛选仍会看到上一批与当前筛选无关的成绩，误以为那就是新筛选的结果。
