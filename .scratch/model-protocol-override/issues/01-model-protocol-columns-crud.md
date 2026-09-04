# 01: 模型协议字段存储与 CRUD

**What to build:** 给供应商模型（provider_model）加一个可空协议字段并贯穿 CRUD：新建/批量导入的模型默认「跟随供应商协议」（字段为空）；详情、列表响应回显该值；编辑接口可把它改成任意一种具体协议或改回「跟随供应商」；提交非空但超出 0..=3 的协议值时接口返回 400。

**Blocked by:** None（可立即开始）

**Status:** ready-for-agent

- [ ] `provider_model` 表新增可空列 `protocol_type`（迁移编号 20，column_exists 守卫，历史库兜底 ALTER；新库建表自动带列）
- [ ] 实体 `provider_model::Model` 增加 `protocol_type: Option<i32>`；既有存量行读取为 `None`（跟随供应商）
- [ ] 模型详情/列表响应携带 `protocolType`（`null` = 跟随供应商）
- [ ] 创建/批量创建/更新请求接受 `protocolType`（缺省 `null`），ActiveModel 赋值正确落库
- [ ] 非空协议值超出 0..=3 时返回 400 中文错误
- [ ] CRUD 集成测试：创建带协议值→响应回显；更新改值/改回 null→回显变化；非法值 400 拒绝
