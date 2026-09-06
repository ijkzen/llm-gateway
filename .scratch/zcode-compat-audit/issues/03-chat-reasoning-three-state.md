# 03: chat_reasoning 三态化——区分「未指定」与「明确关闭」

**What to build:** 客户端显式关闭思考（`reasoning_effort:"none"` 或 `reasoning:{enabled:false}`）时，四个上游方向的请求都真正不思考。现状是「关闭」被归一成 None 后下游一律当作「没提思考参数」：Gemini 不写 thinkingConfig 时默认动态思考仍开（照计费不回显），Responses 不带 reasoning 字段时按默认档位思考。改造方式：`chat_reasoning` 返回值从 Option 改为三态（未指定 / 明确关闭 / 开启+档位），三个转换器接新语义——明确关闭时 Gemini 写 `thinkingConfig:{thinkingBudget:0}`（2.5 Pro 不支持关闭的场景给最小预算并记录）、Responses 写 `reasoning:{effort:"none"}`、Anthropic 不写 thinking（现状即正确）、OpenAI 直通维持字节透传。背景见 `../FINDINGS.md` P5。

**Blocked by:** None (can start immediately)

**Status:** ready-for-agent

- [ ] `reasoning_effort:"none"` / `reasoning:{enabled:false}` 与「完全不带思考参数」在归一层可区分
- [ ] 明确关闭时：Gemini 请求体含关闭语义的 thinkingConfig；Responses 含 effort:"none"（或上游支持的最近似档位）
- [ ] 未指定时四方向行为与现状一致（回归不破）
- [ ] OpenAI 直通路径行为不变（字节透传）
- [ ] cargo fmt / clippy -D warnings / cargo test 全绿
