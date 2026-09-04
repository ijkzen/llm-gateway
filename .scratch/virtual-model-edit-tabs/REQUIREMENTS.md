# REQUIREMENTS — 虚拟模型编辑弹窗：双 Tab 成员管理 + 固定头底布局 + 方向键折叠

> Feature slug: `virtual-model-edit-tabs`
> Source: 用户原始需求（2026-09-04）+ 两次 AskUserQuestion 澄清（LB 顺序取数 / 折叠交互语义）。

## 范围

修改 `web/src/components/virtual-models/VirtualModelEditDialog.tsx`（创建/编辑虚拟模型弹窗），三件事：

1. **滚动布局**：标题栏与底部按钮操作栏固定位置、固定高度、不参与滚动；仅中间内容区滚动。
   - 仓库既有约定（AGENTS.md「弹窗内容可滚动时，标题栏与底部操作区必须固定高度、固定位置且不参与滚动；仅中间内容区负责滚动」）。
   - 参照已有范例 `web/src/components/provider-models/AddProviderModelsDialog.tsx`：`DialogContent` 用
     `flex h-[...] flex-col gap-0 overflow-hidden p-0`，`DialogHeader` 为 `shrink-0`，中间滚动区
     `min-h-0 flex-1 overflow-y-auto`，`DialogFooter` 为 `shrink-0 border-t`。
   - 注意用户原话「标题栏和底部按钮操作栏参与滚动，固定位置和高度，中间内容区域滚动」中「参与滚动」为口语表述，真实意图是**不随内容滚动**（已按仓库约定与既有文档解释，不另行改动）。

2. **成员模型分两个 Tab 分组展示**（Tab 属于弹窗内容区，位于基本信息表单之后）：
   - **Tab「已使用」**：供应商名下至少有一个模型是当前虚拟模型成员（无论该成员是启用、停用，还是跟随供应商禁用——即无论 `item.enable`、`provider.enable` 为何值，只要 `item` 存在于当前虚拟模型）即进入此 Tab。
     - Tab 内先按成员启用/禁用状态分组：启用成员在前，停用成员（含「停用」「随供应商禁用」）在后。
     - 分组内部再按**负载均衡顺序**排序（用户拍板：与运行时后台 LB 顺序一致 = `virtualModelItemId` 升序，即 `src/proxy/mod.rs` `load_members` 的基础尝试顺序；策略 0/1 的用量感知排序是运行态动态值，静态弹窗不展示）。
   - **Tab「未使用」**：供应商名下没有任何一个模型是当前虚拟模型成员 → 该供应商进入此 Tab。
     - 供应商按 `providers` 传入数组顺序排列（该顺序即后台供应商列表 `sort_order`；供应商层面不存在成员级 LB 顺序，故沿用列表顺序 = 页面展示与后台一致的顺序）。
     - Tab 内容 = 该供应商下「可添加候选」（未被其他虚拟模型占用、未在当前成员中）的模型，可展开分组点「+」加入暂存（复用现有候选区交互）。
   - 用户已澄清折叠语义为「供应商分组头整组折叠」，因此分组头始终可折叠/展开，折叠时整组（含成员/候选列表）收起。

3. **方向键折叠/展开（新增）**：无论供应商处于启用还是禁用状态，都允许对分组执行折叠/展开。交互：
   - 供应商分组头为可聚焦按钮（`tabIndex` + `aria-expanded`）。
   - 键盘：`ArrowLeft` 折叠、`ArrowRight` 展开、`Enter`/`Space` 切换；鼠标点击切换。
   - 供应商 `enable=false` 不成为禁止折叠/展开的条件（用户显式要求）。
   - 「方向键」明确到 ←/→（左右），因为折叠/展开是左右语义；不引入 roving tabindex 等更复杂键盘模型。

## 非目标（YAGNI 裁剪）

- 不改后端：排序取数复用现有接口字段（`VirtualModelItem.virtualModelItemId`），无接口/后端改动。
- 不做「创建模式下的未使用 Tab 特判」——创建模式下无成员，全部供应商都进「未使用」Tab 是自然结果（见下「语义边界」）。
- 不引入第三方折叠/树组件，不用 Radix Collapsible——用现有 Button + `aria-expanded` + onKeyDown 足够。
- 不新增供应商列表 sort_order 到前端类型（Tab2 直接用 `providers` 数组顺序，页面已按后台顺序取数）。
- 不重排「暂存添加后立即生效」的交互模型（仍在弹窗内暂存、点保存一次性提交），仅改展示分组与排序。

## 语义边界（grill 澄清记录）

- 「负载均衡顺序」= 运行时 LB 顺序（用户选「和后台 LB 顺序一致」）。弹窗是静态管理界面，取 `virtualModelItemId` 升序
  作为 LB 顺序的静态代理，且排序方向：先按启用状态分两堆，堆内再按 `virtualModelItemId` 升序。
- 「已使用 Tab 内先按启用禁用分组」与后端 `sort_items`（启用优先→策略分组→字母序）分层不同：后端把「启用」当第一排序层；
  用户在此弹窗要求同样的「启用优先」语义做**视觉分组**，堆内顺序用户指定为 LB 顺序（id 升序）而不是字母序。
- 成员「停用 vs 随供应商禁用」两种状态都在「停用堆」内；停用堆内部仍按 id 升序（同一排序函数处理）。
- 未使用 Tab 的折叠分组只含候选区；已使用 Tab 的折叠分组含成员列表 + （可选的）添加候选区。
- 现有「添加候选区（openAddGroups）」的独立 chevron 与「整组折叠」融合：分组头点击 = 整组折叠/展开，折叠后隐藏组内全部内容
  （成员 + 添加区）。保留在分组内添加成员的能力（候选区放成员列表下方，需组为展开态）。

## 布局与结构

- 顶部：基本信息表单区（displayId / 策略 / 启停）—— 归入**中间滚动区**顶部（滚动区随内容滚动），或归入标题下固定区？
  决定：基本信息表单是「内容」的一部分，随中间区滚动；标题栏与底部操作栏固定。Tab 切换条紧贴标题栏下方固定，
  中间滚动区内仅渲染当前 Tab 的分组列表。此结构与 `AddProviderModelsDialog`（标题 → Tab 条 → 滚动内容 → 底部）一致。
- 底部操作栏保留现有提示（已选 N 个成员模型 / 至少保留一个）+ 取消 + 保存/创建。

## 涉及文件

- `web/src/components/virtual-models/VirtualModelEditDialog.tsx`（主体改动）
- `web/src/i18n/locales/zh-CN.ts`、`web/src/i18n/locales/en.ts`（新增 Tab/分组/折叠文案）
- `web/src/components/__tests__/virtual-models-dialogs.test.tsx`（补排序/分组/Tab/折叠测试）
- 既有测试可能因 DOM 结构调整需同步适配（分组内成员仍在、添加候选交互仍在，改动尽量向后兼容断言）。

## 验证

- `cd web && pnpm vitest run`（全量前端测试通过）
- `cd web && pnpm lint`（Biome：tab 缩进、双引号、100 列）
- 视觉冒烟：`pnpm dev` 打开虚拟模型页 → 编辑弹窗 → 双 Tab 分组、折叠、滚动、保存 payload。