# 01: 顶栏「页面刷新」组件（清全部非布局级缓存 + 重取 + 反馈）

**What to build:** 顶栏右侧的刷新按钮升级为「页面刷新」：点击后清空除登录态（`auth/me`）与版本号（`health`）之外的全部前端查询缓存，并立即重新取数当前页面。点击后当前页数据先清空（短暂骨架屏）再呈现新数据；其它页面、其它过滤条件/时间窗口的缓存一并清掉，切页时重新请求（已拍板接受）。重取进行中按钮禁用、图标旋转，让用户知道刷新正在进行。

按钮与刷新逻辑抽成一个小组件（唯一消费方，不抽多余 hook 文件），`layout.tsx` 里原内联按钮块替换为该组件并清理不再使用的 import。

**Blocked by:** None（can start immediately）

**Status:** done (2026-09-10)

- [x] 新组件点击后：非全局的 inactive 缓存数据被清空（`getQueryData` 变 `undefined`）
- [x] active 查询被重新取数（queryFn 再次调用、新数据上屏），忽略全局 5 分钟 `staleTime`
- [x] `["auth","me"]` 与 `["health"]` 数据原样保留、queryFn 未被再次调用（刷新不会踢登录、不重取版本号）
- [x] 重取期间按钮 `disabled` 且图标带旋转类；完成后恢复可点击
- [x] `layout.tsx` 使用该组件；`RefreshCw` 等不再使用的 import 已清理，无 lint 警告
- [x] 行为测试（真实 QueryClient + 探针查询，不 mock react-query）全绿；`cd web && pnpm lint && pnpm vitest run` 通过
- [x] 实现原语为 `queryClient.resetQueries({ predicate })`（predicate 排除顶层键 `auth`/`health`），`useIsFetching` 用同一 predicate 驱动按钮状态
- [x] 评审修复：谓词提为单一 `isRefreshableQuery` 常量（两调用点共用，防漂移）；「保留 auth/health」用例改为挂载布局级查询并在断言前先证明刷新确实执行（probe 重取计数），并做过变异验证（predicate 放开为全刷 → 该用例转红）
