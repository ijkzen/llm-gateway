# 04: 数据面板三张排行卡（该 key 用到的虚拟模型 / 供应商 / 模型）

**What to build:** 在 API Key 数据面板上展示「该 key 的请求覆盖了哪些虚拟模型 / 供应商 / 模型」三张排行表，
各带 6 指标与排序；点击某行跳到对应实体的数据面板，并携带当前区块时间窗。

**Blocked by:** 01（后端排行端点加 apiKey 过滤）、03（面板页骨架与区块时间窗机制已就绪）

**Status:** ready-for-agent

- [ ] 排行一：该 key 用到的虚拟模型（调 `virtual-model-rank?apiKey=…`），行点击跳 `/virtual-models/{id}/overview` 带窗口
- [ ] 排行二：该 key 用到的供应商（调 `provider-rank?apiKey=…`），行点击跳 `/providers/{id}/overview` 带窗口
- [ ] 排行三：该 key 用到的模型（调 `provider-model-rank?apiKey=…`），行点击跳 `/models/{providerId}/{modelId}/overview` 带窗口
- [ ] 时间窗随各排行区块独立；深链 query 拼装与现有 overview 赛马行一致（custom 带 start/endTime，否则 period/offset）
- [ ] 排行卡复用现有赛马表格视觉/排序交互（6 指标可排序），不引入新依赖
- [ ] 前端测试：断言三张卡各自渲染、过滤参数传给对应 hook、行点击触发正确导航路径
