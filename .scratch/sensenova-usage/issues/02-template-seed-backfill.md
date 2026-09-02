# 02: SenseNova 模板种子 + 历史回填

**What to build:** 新建商汤供应商时按 base_url 自动匹配到 SenseNova 模板（推理地址、订阅制、用量开启、extra 含 refresh_token 推荐空字段）；功能上线前已手动创建的商汤供应商，在模板首次插入时自动补齐 extra 中缺失的键（只补缺、不覆盖用户已设值），无需手动编辑 JSON。

**Blocked by:** None (can start immediately)

**Status:** done

- [x] 模板种子新增 SenseNova 条目（usage 开启、refresh_token 为推荐空字段）
- [x] 模板首次插入时向 base_url host 匹配的既有 provider 合并 extra 缺失键（通用机制，不限商汤）
- [x] 已有值的键不被覆盖；非首次插入（更新分支）不触发回填
- [x] 模板匹配/回填单测（先例：provider_template tests）
- [x] 全量质量门绿
