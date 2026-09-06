# 02: 共享窗口类型 + statsQuery 数据层收敛

**What to build:** 面板数据层有了单一实现：统一窗口/过滤类型替换现有三种参数写法；八个数据 hook 的参数序列化、query key 构建、keepPreviousData 收进一个内部查询模块；对外八个 typed facade 原名不变，所有调用方零改动。窗口状态结构与组件层的窗口控件语义对齐，为后续区块收敛铺路。

**Blocked by:** None (can start immediately).

**Status:** ready-for-agent

- [ ] 参数序列化与 key builder 纯函数直测（含缺省参数、过滤维度组合）
- [ ] 经任一 typed facade 验证：查询行为、缓存键、窗口切换保留旧数据均与升级前一致
- [ ] 八个 hook 签名统一为共享窗口/过滤类型，调用方零改动
- [ ] 新增过滤维度只需改一处（以一个演练维度或代码走查确认）
- [ ] pnpm lint + vitest 全绿
