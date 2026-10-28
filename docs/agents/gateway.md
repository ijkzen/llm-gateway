# `/v1` 转发子系统

转发链路：路由解析 → 成员加载与 LB 排序 → 上游调用组装 → 成员尝试循环（failover）→ 流式转运/响应分派 → 指标落库。代码在 `src/proxy/`（模块划分见 `architecture.md` 结构树）。

## 对外端点与转发语义

对外提供 `POST /v1/chat/completions`（Bearer API Key 鉴权），另有两个原生透传端点（ADR-0005）。按虚拟模型 LB 策略选成员，转发到 OpenAI Compatible / OpenAI Responses / Anthropic / Gemini 四种上游协议（转换逻辑参考 nyro 与 LiteLLM），支持流式与非流式。

**failover**：成员失败——本地错误或上游 HTTP >=400——即降级尝试下一成员；降级策略开启时直至成功或全部成员试完，**无状态码白名单**。

## 出站头四层组装

优先级（`src/proxy/headers.rs`）：

1. 协议鉴权头（网关生成，不可覆盖）
2. 下游 allowlist 透传：allowlist 由设置项 `downstream_request_header_allow_list`（Json 字符串数组）动态提供、`AppSettings` 缓存热生效，种子默认 `traceparent`/`tracestate`/`x-opencode-session`/`user-agent`；同名只补缺，剥离清单始终兜底且写入时拒绝黑名单头
3. provider `custom_header`
4. 模板默认头：`provider_template::template_default_headers` 按 base_url host 查漏补缺——`opencode.ai` 兜底 pi 同款动态 UA、`api.kimi.com` 兜底 `KimiCLI/1.50.0`；模板匹配接口同步返回供前端预填

另对 `opencode.ai` 上游注入 `x-opencode-session` 会话头（OpenCode Go 会话亲和）：缺失时按 API Key + 本地日期（设置表时区，缺省 Asia/Shanghai）派生当日稳定 UUID 回退，当日内恒定、跨日零点轮换。

## 思考块无损透传（ADR-0006）

三个转换协议把上游签名类思考载体装进 `message/delta.reasoning_details`，采用 OpenRouter 兼容格式（`reasoning.text`/`reasoning.encrypted` + `format`）：

| 上游协议 | 载体 | `format` |
| --- | --- | --- |
| Anthropic | `thinking` + `signature` / `redacted_thinking` | `anthropic-claude-v1` |
| OpenAI Responses | reasoning item 的 `encrypted_content`（请求侧恒带 `include: ["reasoning.encrypted_content"]`） | `openai-responses-v1` |
| Gemini | functionCall 的 `thoughtSignature` | `google-gemini-v1` |

客户端在 assistant 消息上原样回传后，请求方向按 `format` 匹配注入（不匹配或 failover 换了厂商即 debug 丢弃；OpenAI Compat 直通上游剥离该字段）。Anthropic 工具轮有块回传时不再禁用 thinking。

## 请求指标（ADR-0014）

每次转发成功/失败各落一行 `request` 表（ttft、tps、缓存命中率等 19 个字段，实体见 `src/entity/request.rs`）。

- **ttft 起点** = 建连开始（新建连接）或请求发出（复用连接）；建连耗时并入 ttft，无独立 network_latency 字段。
- **tps** = `output_tokens / (ttft + 输出耗时)`（流式）或 `output_tokens / (end_time − 请求发出)`（非流式）。
- `request_id` 来源可辨：普通转发是裸 UUID，探测是 `test-` 前缀（`src/proxy/probe.rs`），测速是 `speedtest-` 前缀（`SPEED_TEST_REQUEST_PREFIX`）。

探测/测速流量的统计排除口径见 `stats.md`。

## 上游连接

自研 `src/proxy/upstream.rs` + `src/proxy/pool.rs`（hyper 1 + tokio-rustls + webpki-roots，仅 HTTP/1.1）：连接按 `scheme://host:port` 池化复用（响应体读完归还，空闲超 10 分钟由后台任务释放）。响应头超时只守到首字节，流式体不设总超时。

## 虚拟模型 LB 用量感知排序（ADR-0011）

策略 0/1（订阅制优先/按量优先）先分组，组内排序（`src/proxy/usage_rank.rs`）：

- **订阅制：截止时间优先（FEFO）**。从 5h 层起逐层检查双方该层是否都有额度（剩余 > 0）：有则跳过剩余百分比，改比更上层（日/周/月）截止时间（早重置者优先、缺截止时间者排后）；截止链全平则回退剩余百分比逐层比较（5h→日→周→月，同层打平比该层重置时间）；层内无额度或窗口不可用判平进下一层；全平随机选一。
- **按量付费**：按 fetcher 标记的主余额字段（`BalanceItem.primary`，各家指定单字段不做合计）降序。

**剔除口径**：订阅制任一已提供窗口剩余为 0（`UsageData::subscription_usable()` 为 false）即从候选剔除；按量付费查得到余额且合计为 0（`UsageData::balance_usable()` 为 false）即从候选剔除，查不到余额的按量成员**不剔除**（无法判定视为可用）。

用量优先取 10 分钟数据库缓存，缺失/过期才真实抓取（见 `providers.md`）。剔除后的顺序即 failover 尝试顺序；降级策略=1 时每次失败在剩余成员中按同策略重新选路。
