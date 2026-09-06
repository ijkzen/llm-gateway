# llm-gateway 归一转换正确性 × ZCode 客户端适配审计

- 日期：2026-09-06
- 范围：`src/proxy/convert/`（openai 直通 / responses / anthropic / gemini 四个方向）+ `src/proxy/mod.rs` 管线 + `src/routes/openai_compat.rs`
- 对照客户端：本机 ZCode 桌面应用（agent 核心 `ZCode.app/Contents/Resources/glm/zcode.cjs`，内嵌 Vercel AI SDK 的 openai-compatible provider）
- 方法：网关侧逐文件读代码；ZCode 侧解包 app.asar + 逆向 zcode.cjs + 本机 `~/.zcode/v2/config.json` 与 CLI 日志实证

## TL;DR

转换层骨架（消息、工具调用、finish_reason、usage、流式 chunk 形状）没有方向性错误，质量很高。**真正的问题集中在"思考（reasoning）"这个维度**，分两类：

- **断链类（对 ZCode 严重）**：网关把上游的签名类思考载体（Gemini thoughtSignature、Responses encrypted_content、Anthropic signature）只以 OpenRouter 风格的 `reasoning_details` 字段回传给客户端。但 ZCode 的 AI SDK 层**不读 `reasoning_details`**——它只认 `reasoning_content`（纯文本）和 `tool_calls[].extra_content.google.thought_signature`。于是签名到了 ZCode 手里就被丢弃，下一轮对话回传历史时没有签名可带，Gemini 3 工具轮必 400、Responses 有 400 风险、Anthropic 被静默降级。
- **语义丢失类（普遍）**：`reasoning_effort:"none"` 在 Gemini/Responses 上游关不掉思考；`thinking:{type:...}`、`enable_thinking` 两种 ZCode 会发的形态转换侧不识别；usage 丢了 reasoning token 明细。

---

## 第一部分：读懂本报告需要的背景

### 1.1 网关的归一模型

网关对外只暴露 OpenAI Chat Completions 形态（`POST /v1/chat/completions`），对内按上游协议分四类处理：

| 上游协议 | 请求方向 | 响应方向 |
| --- | --- | --- |
| OpenAI Compatible | **字节直通**：只重写 `model`、注入 `stream_options.include_usage`、剥离 `reasoning` 对象和 `reasoning_details`，其余字段原样转发 | **字节直通**：只旁路扫描统计，不改字节 |
| OpenAI Responses | 重建请求体（input 数组、强制 `stream:true` + `store:false`） | 自建状态机把 SSE 事件流转成 OpenAI chunk |
| Anthropic Messages | 重建请求体（system 抽出、tool_use/tool_result 互转） | 同上 |
| Gemini generateContent | 重建请求体（contents/parts、generationConfig） | 同上 |

### 1.2 ZCode 发什么（实证，不是猜测）

ZCode 的模型请求由内嵌的 AI SDK openai-compatible provider 发出。通过逆向 `zcode.cjs` 确认：

**思考参数**：ZCode 按"模型家族"决定发哪种形态——

| 模型家族（按 modelId 字符串匹配） | 实际发到线上的字段 |
| --- | --- |
| kimi-k3 / glm-5.3 / gpt 系 | `reasoning_effort: "low"/"high"/"max"/"xhigh"`（顶层字符串，原样） |
| deepseek-v4 | `thinking: {"type":"enabled"}` + `reasoning_effort: "high"/"max"`；选 off 档发 `thinking:{"type":"disabled"}` |
| 阿里百炼 / qwen | `enable_thinking: true/false` |
| 完全不认识的模型（无 reasoning 配置的任意名字） | 默认发 `thinking: {"type":"enabled"}`（二档开关，默认开） |
| 认识但没有内置映射表的（如 longcat2.0） | **什么都不发** |

关键事实：ZCode 的档位字符串会原样上线，包括 `max`、`xhigh`、`none` 这些 OpenAI 官方枚举里没有的值。本机实证：CLI 日志里 `session.reasoning_effort.updated` 事件显示当前档位恒为 `max`。

**其他字段**：流式恒带 `stream_options:{include_usage:true}`；历史消息里 assistant 轮会回传 `reasoning_content`（纯文本思考）；工具调用若带签名会放在 `tool_calls[].extra_content.google.thought_signature`。

**ZCode 读什么（响应侧）**：

- 流式 delta：`content`、`reasoning_content`（或 `reasoning`）、`tool_calls`、`finish_reason`、usage 尾块
- 非流式 message：同上
- **`reasoning_details` 不在它的解析 schema 里**——zcode.cjs 里 18 处 `reasoning_details` 全部是 models.dev 模型目录的静态数据，运行时解析代码一处都没有
- Gemini 签名只从 `tool_calls[].extra_content.google.thought_signature` 读取，读到后会存进 providerMetadata 并在下一轮原样回传

### 1.3 网关的思考载体设计（v0.1.14 现状）

网关三协议捕获上游签名类载体（Anthropic `thinking.signature`/`redacted_thinking`、Responses `encrypted_content`、Gemini `thoughtSignature`），统一装进 OpenRouter 兼容的 `message/delta.reasoning_details` 回传给客户端；客户端原样回传后按 `format` 匹配注入回上游。这套设计对 **OpenRouter 风格客户端**（会原样搬运 reasoning_details 的）是闭环的。**ZCode 不是这种客户端。**

---

## 第二部分：断链类问题（ZCode 视角最严重）

### P1. Gemini thoughtSignature 断链——ZCode + Gemini 上游 + 工具调用 = 第二轮必 400

**背景**：Gemini 3 系列（gemini-3-pro 等）在工具调用对话里强制校验签名：模型在流式响应里给每个 functionCall part 附带一个 `thoughtSignature` 字段（一串不透明密文），客户端下一轮把对话历史原样发回时，这个签名必须原样挂在对应的 functionCall part 上，**缺了就直接 400**。这是 Google 的硬性校验，不是可选项。

**现在的数据流**：

```
第 1 轮  上游 Gemini → 网关：functionCall part 带 thoughtSignature: "sig-abc"
         网关 → ZCode：tool_calls[0]（无签名）+ delta.reasoning_details 里放签名
         ZCode：读 tool_calls ✓，读 reasoning_content ✓，reasoning_details 不认识 → 签名丢弃
第 2 轮  ZCode → 网关：历史里 assistant 消息的 tool_calls 干净无签名
         网关 → 上游 Gemini：gemini.rs 只从 reasoning_details 里找签名（找不到）
                → functionCall part 无 thoughtSignature → Gemini 3 返回 400
```

**代码位置**：签名捕获在 [gemini.rs 流式 855 行附近 / 非流式 702 行附近]（写成 `reasoning.encrypted` detail）；请求侧注入在 [gemini.rs:230-249]，只认 `reasoning_details` 里 `format=="google-gemini-v1"` 的项。

**影响面**：任何"ZCode → 网关 → Gemini 3 系上游"的工具调用对话，第二轮起必炸。普通对话（无工具）不受影响。

**修法**（已验证可行性）：ZCode 侧 AI SDK 原生支持 `tool_calls[i].extra_content.google.thought_signature`——它读到这个字段会存进 providerMetadata 并原样回传。所以网关只要：

1. 响应侧：往 `tool_calls[i]` 里**双写**一份 `extra_content.google.thought_signature`（reasoning_details 继续保留，不破坏 OpenRouter 风格客户端）；
2. 请求侧：gemini.rs 注入签名时，除了看 reasoning_details，也认 assistant 消息 `tool_calls[i].extra_content.google.thought_signature`。

这个修复同时也利好其他基于 AI SDK 的客户端（ Cherry Studio 系、各种 AI SDK 套壳）。

### P2. Responses `encrypted_content` 断链——gpt-5/o 系工具轮有 400 风险

**背景**：OpenAI Responses API 的推理模型（gpt-5、o 系列）在工具调用多轮里要求把模型产出的 reasoning item 原样回传。网关固定 `store:false`（不让 OpenAI 存会话），这种模式下 reasoning item 必须以 `encrypted_content`（加密密文）形式回传，且请求时要带 `include:["reasoning.encrypted_content"]`（网关已带，见 responses.rs:88）。

**断链方式与 P1 相同**：网关把 `encrypted_content` 装进 `reasoning_details`（format=`openai-responses-v1`）回传，ZCode 不读 → 不回传 → 下一轮 requests 侧 [responses.rs:178-197] 找不到可注入的 reasoning item → 工具轮历史缺 reasoning item，OpenAI 可能 400（此前调研结论：gpt-5/o 系有此硬性要求）。

**与 P1 的差别**：AI SDK 对 Responses 系没有像 `extra_content.google` 那样现成的字段约定，修复没有 P1 那么直接。

**2026-09-07 静态验证定论（替代生产实测）**：ZCode 不回传 `reasoning_details`。证据链（zcode.cjs）：① 响应解析 schema 顶层是 `looseObject`，但嵌套的 `message`/`delta` 对象是 strict `g.object`——未知字段在 zod parse 时就被剥离；② AI SDK 内部模型只有 text/reasoning（纯文本）/tool-call 三种 part，没有 reasoning_details 概念；③ 回传历史的组装函数（`convertToOpenAICompatibleChatMessages` 的 `case "assistant"`）逐字段重建消息：只写 `content`/`reasoning_content`/`tool_calls`（唯一例外是 tool_call 级 `extra_content.google.thought_signature`——它同时出现在解析 schema、内部 providerOptions、回传组装三处，所以能存活）。因此「ZCode 透传未知字段使链路自愈」的假设不成立：**ZCode + Responses 上游 + 多轮工具对话是事实上的断链组合**，且 AI SDK openai-compatible 线上格式里没有可双写的载体（不像 Gemini 有 extra_content 约定），短期只能文档化为不支持组合或等 ZCode 侧支持。

### P3. Anthropic 签名断链——不报错，但双重降智（已知问题，此处补齐 ZCode 视角）

**背景**：Anthropic 的 extended thinking 开启后，工具调用轮的历史里必须带签名的 thinking 块，否则 400。网关此前的设计（3ffa3c3）已解决"签名回传"——但还是放在 reasoning_details 里。

**ZCode 场景的实际走向**：ZCode 丢弃 reasoning_details → 下一轮历史里没有签名块 → 网关 [anthropic.rs:339-356] 的 `drop_thinking_without_history_blocks` 检测到"含 tool_calls 的 assistant 轮没有可回传的 thinking 块"，于是**把整个 thinking 参数丢掉**（打一条 warn 日志）→ 该轮请求以"思考关闭"发出 → 且此后每轮都关（历史里永远没有签名块）。不报错，但推理质量静默下降——这正是 memory 里记的"双重降智"。

**缓解现状**：ZCode 会回传 `reasoning_content` 纯文本。如果请求侧允许"无签名 thinking 块以 `thinking` 但无 signature 注入"——不可行，Anthropic 校验签名必填。现实修法只有两个：(a) 等 ZCode 支持 reasoning_details（上游修）；(b) 网关对"ZCode 类客户端 + Anthropic 上游 + 工具对话"场景接受降级，把 warn 日志升级为更可见的信号（如响应头或 request 表标记），让降智可观测。

### P4.（与断链同源但方向相反）ZCode 回传的 `reasoning_content`，Anthropic/Responses 请求侧不消费

ZCode 的 AI SDK 在组装历史消息时会带上 assistant 轮的 `reasoning_content` 纯文本。网关三个转换器里：

- **Gemini 消费了**：[gemini.rs:205-209] 把它转成 `{text, thought:true}` part 回传上游 ✓
- **Anthropic 不消费**：[anthropic.rs:66-99] 只从 reasoning_details 还原 thinking 块，纯文本被丢弃（丢了对 Anthropic 也无害，它本来就不要求思考文本回传，只要求签名块——所以这条不算缺陷，仅记录差异）
- **Responses 不消费**：同理无害

结论：这条不是 bug，是记录"各协议对纯文本思考的利用不一致"，Gemini 路径是对的。

---

## 第三部分：语义丢失类问题（所有客户端都受影响）

### P5. `reasoning_effort: "none"`（关思考）在 Gemini / Responses 上游关不掉

**背景**：`chat_reasoning`（[mod.rs:127-186]）把"关闭思考"（`reasoning_effort:"none"` 或 `reasoning:{enabled:false}`）归一成 `None`——即"本请求没有思考参数"。下游各转换器看到 `None` 的行为是"不写思考相关字段"：

| 上游 | 不写思考字段的实际语义 | 客户端意图（关）是否达成 |
| --- | --- | --- |
| OpenAI 直通 | `reasoning_effort:"none"` 原样转发给上游 | ✓（上游自己处理 none） |
| Anthropic | 不写 `thinking` = 思考关闭 | ✓ 巧合达成 |
| Gemini | 不写 `thinkingConfig` = **动态思考默认开启**（只思考不回显，照计费） | ✗ 反了 |
| Responses | 不写 `reasoning` = 模型按默认档位（medium）思考 | ✗ 没关掉 |

**修法**：`chat_reasoning` 需要区分"没提"和"明确要求关"两种 None（比如返回 enum：Unspecified / Disabled / Enabled(effort)）。Disabled 时 Gemini 侧写 `thinkingConfig:{thinkingBudget:0}`（注意 2.5 Pro 不支持关闭，只能给最小预算），Responses 侧写 `reasoning:{effort:"none"}`（o4-mini/gpt-5.1+ 支持）或最小档。

**现实触发概率**：ZCode 的 deepseek 家族 off 档走的是 `thinking:{type:"disabled"}`（见 P7），`reasoning_effort:"none"` 只有 glm-5.2 的 nothink 档和手写请求会发。中等优先级。

### P6. Responses 上游原样透传 effort，`"max"` 会被官方 OpenAI 拒绝

[responses.rs:108-110] 把归一后的 effort 字符串原样写进 `reasoning:{effort}`。OpenAI Responses 官方枚举是 `none/minimal/low/medium/high/xhigh`（gpt-5.1 起含 xhigh），**没有 `max`**。而 `max` 恰恰是 ZCode 对 kimi-k3/glm 家族的**默认档**（本机 config.json 实证 defaultVariant 全是 max）。

走向：ZCode 选默认档 → `reasoning_effort:"max"` → 网关归一 → 若虚拟模型成员是官方 OpenAI Responses 上游 → `reasoning:{effort:"max"}` → 400。

**注意**：Anthropic/Gemini 方向不受影响（走 `reasoning_budget` 档位表，max→16384 有映射）；OpenAI 直通也不受影响（字节透传，上游是 kimi/智谱这些认 max 的）。只有"官方 OpenAI Responses 上游"这个组合会炸。用户当前没有这类 provider，属于**埋着的雷**。

**修法**：Responses 方向加档位钳制/映射（max→xhigh 或 high），或按上游能力表白名单。

### P7. ZCode 的另外两种思考形态：`thinking:{type}` 和 `enable_thinking`，转换侧完全不识别

`chat_reasoning` 只认两个字段：`reasoning` 对象、`reasoning_effort` 字符串。但 ZCode 实际会发三种形态（见 1.1 表格）：

- `thinking:{"type":"enabled"|"disabled"}`：deepseek 家族**和一切 ZCode 不认识的模型**（默认二档开关，默认 enabled！）
- `enable_thinking: true/false`：阿里百炼/qwen 家族

**现状**：

- **OpenAI 直通**：字节透传 ✓ 没问题（上游是同协议，认识这些字段）
- **Anthropic / Responses / Gemini 转换**：字段被静默忽略。其中 `enabled` 丢成"不写"= 思考没开（用户想要开）；`disabled` 丢到 Gemini = 思考反而开着（P5 同款反转）

**触发场景**：用户在网关上建一个名字不带家族特征的虚拟模型（比如 `my-claude`）指向 Anthropic 上游，ZCode 会默认发 `thinking:{type:"enabled"}` 但思考永远不会开。用户界面里明明显示"思考：开"。

**修法**：`chat_reasoning` 补认两种形态：`thinking.type=="enabled"` → 等价 effort "high"（或 medium），`=="disabled"` → 等价 none；`enable_thinking:true/false` 同理。几行代码的事。

### P8. usage 归一丢弃 reasoning token 明细

**背景**：OpenAI 官方口径里 `completion_tokens` 是含推理 token 的总数，另在 `completion_tokens_details.reasoning_tokens` 里单列推理量。ZCode 会读这个明细做展示（CLI 日志里 reasoningTokens 统计天天在打）。

**现状**：网关归一结构 `Usage`（metrics.rs）只有 input/cache/output 三个字段；客户端可见 usage 由 `cached_client_usage_json` 生成（[mod.rs:246-254]），只有 prompt/completion/total + cached_tokens。

- OpenAI 直通：上游 usage 字节透传，reasoning_tokens 明细**保留** ✓
- Anthropic 转换：output_tokens 含思考 ✓（口径对），无明细 ✗
- Gemini 转换：candidates+thoughts 加进 output ✓（[gemini.rs:592-616]，口径对），thoughtsTokenCount 明细丢 ✗
- Responses 转换：output_tokens 含推理 ✓，`output_tokens_details.reasoning_tokens` 明细丢 ✗

**影响**：ZCode 面板上走转换链路的请求，reasoningTokens 恒为 0；总账（completion_tokens）是对的。低优先级，纯展示层缺失。

### P9. Anthropic `tool_result` 空内容无兜底，可能 400

[anthropic.rs:101-114] 把 tool 消息的文本原样写进 `tool_result.content`。工具输出为空字符串时（shell 命令无输出、文件为空，ZCode 场景常见），content 就是 `""`。Anthropic 对空 text 内容有校验（"text content blocks must be non-empty"），会 400。

对比：user 消息空内容已有 `" "` 兜底（[anthropic.rs:61-63]），tool_result 漏了同款处理。一行修复：`content` 为空时填 `" "` 或 `"(empty)"`（LiteLLM 用 `"."`）。

### P10. 次要项（记录备查，不建议专门修）

- **Anthropic 流式多个 `message_delta` 会重复发 finish chunk**（[anthropic.rs:802]）：正规 Anthropic 流只有一个带 stop_reason 的 message_delta，但有些兼容上游（如某些代理商）会发多个（比如只更新 usage 的）。届时客户端会收到多个 finish_reason 块。AI SDK 对此宽容，风险低。
- **文件/PDF 输入静默丢弃**：三个转换器的 user 内容解析都只认 `text` 和 `image_url`，AI SDK 发 PDF 时用 `{"type":"file","file":{...}}` 形态，会被 filter_map 丢掉。ZCode 对 pdf 模态的模型发附件时内容缺失且无报错。当前用户没有向网关发 PDF 的场景，先记录。
- **Anthropic 默认 max_tokens=4096**：客户端不传 max_tokens/max_completion_tokens 时兜底 4096。ZCode 的模型条目都带 `limit.output`（128k 量级）且 AI SDK 会下发，正常不会触发；裸 curl 调试时会感到"回答被砍"。

---

## 第四部分：ZCode 适配度核对清单（哪些是好的）

为了防止只看到问题，以下核对过的**正常项**也留档：

| ZCode 的需求 | 网关现状 |
| --- | --- |
| `reasoning_effort` 顶层字符串（含 max/xhigh 非标值） | 直通原样转发 ✓；Anthropic/Gemini 按档位表换算预算（max→16384 有档）✓ |
| 流式 chunk 形状（role 首块、content/reasoning_content 增量、tool_calls index 增量、finish chunk、`[DONE]`） | 三个转换器 + 管线补发逻辑全覆盖 ✓；ZCode 读 `delta.reasoning_content ?? delta.reasoning`，网关发前者 ✓ |
| 流式 usage 尾块（ZCode 恒发 `include_usage:true`） | 客户端请求了就回 usage chunk ✓；直通时上游尾块原样过 ✓ |
| 非流式 `message.reasoning_content` | 三协议非流式都写 ✓ |
| 错误体 `{error:{message,type,code}}`（含流式中途错误帧） | 形状匹配 ✓（openai_error / 1875-1879 的错误帧） |
| 入口宽松（只要求 model 字段；`store`/`metadata`/`prediction` 等多余字段不 400） | ✓ 直通转发、转换忽略 |
| `/v1/models` 列表 | 极简形状够用：ZCode 靠**模型名字符串**匹配内置能力表，不读网关元数据 ✓（所以虚拟模型命名保持家族名 = 正确姿势，用户现网就是这么命名的） |
| assistant 历史回传 `reasoning_content` | Gemini 消费 ✓；直通保留 ✓（DeepSeek 工具轮必需）；Anthropic/Responses 不需要 |
| failover 重试（408/429/5xx） | 对 ZCode 透明 ✓ |

两个"不是网关问题但要知道"的事实：

1. **`longcat2.0` 这类 ZCode 无内置映射的模型，ZCode 端根本不发送思考参数**（网关侧看到"无思考字段"≠用户关了思考）。这是 ZCode 的行为，网关无从干预。
2. 本机 ZCode 对网关 provider（gateway.ijkzen.cn）没配任何 options 覆盖，行为干净；所有思考参数都是 ZCode 按模型家族自动决定的。

---

## 第五部分：修复优先级建议

| 优先级 | 项 | 理由 |
| --- | --- | --- |
| 1 | P1 Gemini 签名双写（`extra_content.google.thought_signature`） | 唯一"必炸"项：ZCode+Gemini3+工具 = 第二轮 400；修法明确且双向验证过 |
| 2 | P9 tool_result 空串兜底 | 一行代码，消除一类 400 |
| 3 | P7 chat_reasoning 补认 thinking/enable_thinking | 几行代码，补上 ZCode 三形态中的两种 |
| 4 | P5 区分"未指定"与"明确关闭" | 语义正确性，依赖 P7 的形态归一 |
| 5 | P6 Responses effort 钳制 | 埋雷型，官方 OpenAI Responses 上游接入前必须做 |
| 6 | P2 Responses 密文回传 | 先实测 ZCode 是否透传未知 message 字段，再定方案 |
| 7 | P3 Anthropic 降智可观测化 | 短期无法根治，先让降级可见 |
| 8 | P8 reasoning_tokens 明细 | 展示层锦上添花 |

---

## 附一：四协议 × ZCode（带思考 × 带工具调用）支持矩阵

回答「llm-gateway 代理四种上游协议时，ZCode 能否正常工作」的完整定论。判断基于：ZCode 的行为（2026-09-06/07 逆向实证）＋ 网关各转换链路的当前实现。核心结论一句话：**ZCode 的核心循环只依赖 `content` / `tool_calls` / `finish_reason` / `usage` / `reasoning_content` 五个要素，这五样在四条链路上都已吻合；断的只是「签名类思考载体」这一层**——它只在「推理思考 + 多轮工具调用」这个交叉点上构成问题，普通对话（含思考）四条链路全部正常。

| 上游协议 | 普通对话（含思考、无工具） | 多轮工具调用（ZCode 主场景，含思考） | 卡点说明 |
| --- | --- | --- | --- |
| **OpenAI Compatible（直通）** | ✅ 完全正常 | ✅ 完全正常 | 字节透传：`reasoning_effort`/`thinking`/`enable_thinking` 原样到上游；`reasoning_content` 回传保留（DeepSeek 工具轮必需）。本表其余问题与它无关 |
| **Anthropic Messages** | ✅ 正常 | ⚠️ **能用但静默降智** | 思考开关/档位生效，思考文本照常返回。但 ZCode 不回传签名块 → 网关触发 `drop_thinking_without_history_blocks` 每轮关掉 thinking（不报错、工具循环正常）。修复后带 `x-llm-gateway-thinking-dropped: history` 响应头，降级可观测（工单 08） |
| **Gemini generateContent** | ✅ 正常 | ❌→✅ 视模型与是否已部署工单 01 | 思考经 thinkingConfig 生效、`reasoning_content` 双向通。Gemini 3 系工具轮**强制** thoughtSignature：工单 01 部署前 ZCode 签名丢失 → 第二轮必 400；部署后双写到 `extra_content.google.thought_signature`，链闭合（待真机验证）。Gemini 2.5 及更早不强制签名，原本就可用 |
| **OpenAI Responses** | ✅ 正常 | ❌ 推理模型结构性不行 | 网关强制流式 + 客户端非流式聚合，普通对话正常。但推理模型（gpt-5/o 系）工具轮要求回传 `encrypted_content` 密文：ZCode 存不下也回传不了（唯一三环不贯通的载体），后续工具轮可能 400。**网关侧无解**（AI SDK 线上格式没有可双写的密文载体，不像 Gemini 的 extra_content），规避法 = 这类模型挂 OpenAI Compatible 形态的上游（如代理商直通），或等 ZCode 侧支持 `reasoning_details` |

补充边界（容易误读的点）：

- 「四协议代理下 ZCode 不能正常工作」的说法不准确——只有上表标 ❌/⚠️ 的交叉点受影响，且 ⚠️ 只是降智不是不可用。
- 用户当前生产（deepseek-v4-flash / kimi-k3 / longcat 等）全是 OpenAI Compatible 直通链路，本表问题不影响现网。
- 矩阵列的是 ZCode 这一种客户端。OpenRouter 风格客户端（原样搬运 `reasoning_details`）在 Anthropic/Gemini/Responses 三条链上是完整闭环的——这正是网关双写方案「两种客户端都保」的原因。

---

## 整改状态（2026-09-06，feat/zcode-compat-fixes 分支）

已实现并全量测试通过：工单 01（Gemini 签名双写 `tool_calls[i].extra_content.google.thought_signature`，请求侧双来源注入、extra_content 优先）、02（tool_result 空内容兜底 `" "`）、03+04（`chat_reasoning` 三态化 `ChatReasoning::{Unspecified, Disabled, Enabled}`，补认 `thinking:{type}` 与 `enable_thinking` 形态，优先级 reasoning 对象 > reasoning_effort > thinking > enable_thinking；明确关闭时 Gemini 写 `thinkingBudget:0`、Responses 写 `effort:"none"`）、05（Responses effort 钳制：max→xhigh，未知→high 并 debug 日志）、06（`Usage` 增 reasoning_tokens，Responses/Gemini 提取，客户端 usage 补 `completion_tokens_details.reasoning_tokens`；不落 request 表）、08（降级标记响应头）。

**工单 08 标记口径**：Anthropic 方向思考参数因工具轮历史缺签名块被丢弃时，响应（流式与非流式均）带 `x-llm-gateway-thinking-dropped: history` 头；未触发时无此头。落库侧未做（fail_reason 语义不兼容非失败标记，扩列收益低）。

工单 07 已静态定论（见 P2 的 2026-09-07 补记）：ZCode 不回传 reasoning_details，Responses 密文链对 ZCode 确认断开，且 AI SDK 线上格式无可双写载体——结论为「不支持组合」，无需生产实测。工单 01/04 的端到端真机验收项留待生产验证。

已知留档：Gemini 2.5 Pro 不支持 thinkingBudget=0 的场景由上游自行钳制（网关不维护模型能力表）；OpenAI 直通路径对 `reasoning:{enabled:false}` 维持「剥离对象、不注入」的既有行为（Disabled 不新注入 `reasoning_effort:"none"`，避免触达不支持该值的上游）。

---

## 附：审计方法备注（可复现）

- ZCode 侧证据链：`~/.zcode/v2/config.json`（各模型 reasoning.variants/defaultVariant/limit）→ 解包 `app.asar`（host 层连通性探针代码）→ agent 核心 `Resources/glm/zcode.cjs`（档位→providerOptions 映射表 `providerOptionsByLevel`、AI SDK openai-compatible 的 body 组装与响应解析 schema）→ CLI 日志 `~/.zcode/cli/log/zcode-2026-09-06.jsonl`（`session.reasoning_effort.updated` 实证档位为 max）。
- 网关侧：convert 五文件 + proxy/mod.rs + routes/openai_compat.rs 全量通读（2026-09-06，main @ c178016）。
