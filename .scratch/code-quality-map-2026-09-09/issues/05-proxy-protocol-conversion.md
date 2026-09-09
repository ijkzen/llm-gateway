# 05 · proxy 协议转换审查

Type: task
Status: claimed
Blocked by: 01

## Question

对 proxy 协议转换域做全量审查：`convert/` 全目录（openai/responses/anthropic/gemini × request/response/stream/images + 思考块载体推理 details 搬运 + 域内测试）。按四轴 + 专门性能轮产出分级清单（不改代码）：

- 逻辑正确性：跨协议字段映射遗漏、思考块 format 匹配/注入、工具轮回传、畸形上游事件处理、非流式全缓冲路径（历次审计已修项不复查）；
- 实现简洁：四协议转换的重复结构与可收敛点（对照 nyro/LiteLLM 参照系）；
- 测试覆盖：mock 回归之外缺什么（字段级等价、边界畸形输入）；
- 模块间调用：Converter 门面与 relay 泵、原生透传旁路的关系是否合适。

产出 `.scratch/code-quality-map-2026-09-09/findings/05-proxy-protocol-conversion.md`，Answer 给摘要与需拍板问题。

## Answer

**清单产出**：`findings/05-proxy-protocol-conversion.md`——10 条（1 P3·安全观察 + 1 P3·口径 + 4 逻辑/简洁 + 3 测试 + 2 性能观察），无 P1/P2。方法=convert 全目录 ~3800 行逐行通读 + relay 泵/TailSpec/RequestFlags/native 旁路交叉核对 + 历次审计边界圈定（protocol-conversion-audit 整改状态/openrouter 01-08 批/zcode 8 工单/架构轮 6 候选均不复查，仅核验未回归）。

- **核心结论**：转换域是历次审计反复打磨的成熟面——reasoning_details 载体全链路（构造/校验/钳制/三格式过滤/注入）、思考关闭三态×四协议出站差异、工具轮签名回传（Anthropic 签名必带/Responses encrypted_content 双路径去重/Gemini extra_content 双写）、finish 全表+工具收尾优先、usage 归一与旁路 scanner 等价——均核验无问题（见「已核验无问题区」，含逐条锚点）。新发现集中在三处：① 复杂清洗/下载函数零测试（sanitize_gemini_schema 无 schema 用例、images.rs 全路径零测试、reasoning_details 上限截断无测试）；② 死代码残渣（collect_tool_call_names 两处计算即弃、GeminiStreamConverter.model 只写不读）；③ 边缘逻辑（Anthropic json 模式 text 丢弃+多缓冲乱序、Responses 回放全文保留内存随输出增长）。
- **两项拍板（2026-09-10 AskUserQuestion）**：
  - **05-01 Gemini 远程图片下载 SSRF + 无界读取** → 保持现状记观察（信任 /v1 Bearer 门内 key 持有者），登记触发再评估条件（key 分发面扩大/网关网络内出现高价值内网服务/下载放大被利用），届时默认解=私有段/元数据地址阻断 + Content-Length 预检 + 流式截断。
  - **05-03 OpenAI 直通思考开关双写归一 effort** → 保持现状（双写覆盖「只认 effort」与「只认开关」两类上游理解，删除注入反而使 OpenAI 官方类上游收不到思考意图），实施批登记实证条目（DeepSeek 类上游 thinking.type+reasoning_effort 共存行为验证，冲突再改）。
- **历次遗留衔接**：A2/O3/O4/05/06 代际立项图外不重记；A7/C4/C5/B8/D3 旧账确认仍在（跟踪点在 protocol-conversion-audit FINDINGS 09-05 整改状态段，行号已在本票文件逐条锚定），随实施批统一处理，不开新账。
- **需拍板问题**：已全部当场拍板（上述两项），无遗留。

Status: resolved
