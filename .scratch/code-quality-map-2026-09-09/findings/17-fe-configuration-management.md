# FINDINGS · 17 FE 配置管理域审查（2026-09-10）

范围：providers 族 9 组件 + api-keys 族 4 组件 + provider-models 族 6 组件（AddProviderModelsDialog 824/ProviderModelDetailDialog 444）+ virtual-models 族 5 组件（VirtualModelEditDialog 685）+ 四页面 + CRUD hooks（use-providers/use-api-keys/use-provider-models/use-virtual-models/use-provider-usage/use-usage-estimate）+ 全部相关 `__tests__` 盘点。方法：两个子代理分族全读 + 主代理对全部 P2 与关键 P3 行号磁盘复核。清单模式：不改代码。

## 汇总

| 编号 | 严重度 | 维度 | 一句话 |
| --- | --- | --- | --- |
| 17-01 | P2 | 逻辑/竞态 | ProviderDetail 切换供应商时，在途明文 Key 请求 resolve 后把 A 的密钥写到 B 的详情上（渲染期重置挡不住在途 setState） |
| 17-02 | P2 | 逻辑/门控 | 编辑弹窗用量开关仅在 `extra.usage===true` 时渲染——关掉一次后开关整块消失，再也无法打开 |
| 17-03 | P2 | 逻辑 | `useMatchTemplate` 声称吞 404 但 `await` 在 try 外：未命中模板（输入 URL 的绝大多数情况）ky 抛错 → 每个键击 2 次请求+静默 error 态，且无防抖 |
| 17-04 | P2 | i18n | 使用了不存在的 key `apiKeys.showKeyFailed`（两 locale 均无）——取明文失败时 toast 标题显示原始 key 字面量 |
| 17-05 | P2 | 逻辑/健壮 | ProviderDetail 对 `extra`/`customHeader` 无保护 `JSON.parse`：密钥丢失时后端透传密文（非 JSON）→ 渲染抛错整页 ErrorBoundary |
| 17-06 | P2 | 逻辑/交互 | Add 弹窗「手动/待确认」候选卡整卡可点，卡内数字输入无 stopPropagation——点击/回车冒泡触发跳转卸载输入框，这两个态的数字字段根本填不进去（现有测试全用 fireEvent.change 故未暴露） |
| 17-07 | P3 | 逻辑/校验 | 前端代理地址校验缺 `@` 拒绝规则，与后端 validate_proxy 口径不齐（只能吃服务端报错） |
| 17-08 | P3 | 规范 | ProviderEditDialog:308 模板候选文案用 `truncate`，违反「单行截断一律 MidEllipsis」约定（全 scope 唯一一处） |
| 17-09 | P3 | 简洁 | ApiKeyCell 详情拉取失败静默显示掩码无提示（与 ProviderDetail 的 toast 不一致） |
| 17-10 | P3 | 安全/一致性 | ApiKeyCell 把明文 Key 写进 React Query 缓存（gcTime 5min），与 ProviderDetail「明文不进任何缓存」注释策略相悖；波及面：useApiKeyDetail 亦被 api-key-overview 消费 |
| 17-11 | P3 | 逻辑 | 删除当前选中供应商后详情面板空白，不回落列表首个 |
| 17-12 | P3 | 性能 | 供应商页一挂载即全局拉全部供应商模型（SpeedTestDialog 无条件挂载再客户端过滤）；后端有按 provider 作用域端点未用 |
| 17-13 | P3 | 逻辑 | ApiKeysTable 任何数据 identity 变化（如启停开关触发 invalidate）都把分页打回第 1 页 |
| 17-14 | P3 | 简洁 | ProviderDetail 手写协议/付费标签映射重复 ProtocolIcon 导出（`billingModeLabel` 全仓零消费=死导出） |
| 17-15 | P3 | 逻辑/文案 | 编辑模式 API Key 输入框标必填星号，实际留空=不修改——误导 |
| 17-16 | P3 | i18n | ProviderUsageCard 硬编码 zh-CN/中文串（:55/:64/:202），EN 下穿帮（01 盘点已归 20 票域，此记实例） |
| 17-17 | P3 | 简洁 | 创建模式应用模板时 extra 可编辑键默认值全置空——模板一旦带非空默认值即被丢弃（当前种子全空占位，无实害） |
| 17-18 | P3 | 健壮 | Add 弹窗 numberEdits 更新用渲染闭包旧 `edits` 展开（非 prev[id]），同批更新互相覆盖（窄） |
| 17-19 | P3 | 规范 | ProviderModelDetailDialog/VirtualModelItemDetailDialog 把 overflow-y-auto 放整个 DialogContent——标题/底部随内容滚动，违反固定头底约定（同族大弹窗做对了） |
| 17-20 | P3 | 失效面 | `useDeleteProvider` 漏失效 `providerModelKeys`——删供应商（后端级联删模型）后 RequestLogsTable 模型筛选短暂展示已删模型（波及 16 域冻结 hook，仅追加失效不动签名） |
| 17-21 | P3 | i18n | ProviderModelSection「添加」按钮硬编码中文（同文件其余走 t()） |
| 17-22 | P3 | 逻辑 | VM 编辑弹窗对 providerModels 中缺失的草稿成员不可见却仍提交，接口类型变更确认也漏算 → 后端 400 而前端无提示（触发面窄：正常态列表完整） |
| 17-23 | P3 | 逻辑 | ProviderModelDetailDialog 的 reset effect 依赖 model 对象身份——后台 refetch 即静默丢弃编辑态退回只读 |
| 17-24 | P3 | 性能 | Add 弹窗候选全量渲染无虚拟化（数百候选 × 2 受控数字输入，大目录供应商刷新可感知卡顿） |
| 17-25 | P3 | 性能/简洁 | VM 弹窗 `providers.map(groupOf)` 算两遍 + modelById/providerById 每渲染重建（O(2·P·M·D)，量级可接受） |
| 17-26 | P3 | 简洁 | 能力图标两实现近同构（CapabilityIcons vs ItemCapabilityIcons）+ 只读详情 dl 两弹窗重复 + 手动表单两处重复 |
| 17-27 | P3 | 注释漂移 | `modelSearchDebounced` 名为防抖实则同步逐字请求目录搜索（注释自述防抖） |
| 17-28 | P3 | 测试覆盖 | 缺口族：候选卡输入点击/回车（17-06 回归网缺失根因）/批量选择矩阵/四条 onError 失败路径/接口类型变更确认全链路零覆盖/详情弹窗 protocolType 与能力开关提交/ProvidersPage 无测试/ hooks 零直测（useMatchTemplate 404 分支、useApiKeyDetail、useProviderUsage） |

**弹窗脚手架收敛定案（01 盘点候选，给 19 票）**：只抽「固定头/尾+可滚动主体」的布局原语 `DialogScrollShell`（ProviderEditDialog 与 ProviderSpeedTestDialog 头栏 className 逐字节相同可证），**不抽** form/zod/toast/reset 装配（四弹窗差异结构性，参数化=ADR-0004 已否决的泥潭；若采纳需在 ADR-0004 加 addendum 划清边界）。19 落地、17 域五弹窗消费。

**本票无需拍板项**。

## 各条证据

### 17-01 切换供应商明文串号（P2，竞态）

ProviderDetail.tsx:136-150：`handleToggleKey`/`复制` 里 `await fetchProviderApiKey(provider.id)` 后直接 `setPlainKey(key)`；切换重置（:104-110）只在渲染期清 state，挡不住旧请求 resolve 后的迟到 setState——A 的明文写到 B 的详情。窗口=API 延迟（远程部署更明显）。默认解：调用时捕获 id、resolve 后比对当前 id 再 setState，或改按 id 键控 query。纯组件层。

### 17-02 用量开关一去不返（P2，门控）

ProviderEditDialog.tsx:463 `{templateExtra && usageFlag(templateExtra) && (...)}`，usageFlag=parseExtra(extra).usage===true（:99-101）；编辑模式 templateExtra=provider.extra（:156-159）。后端会存 `usage:false`（provider_template/tests.rs:295-322 Krill 用例在证）→ 关过后开关与 extra 字段整块消失，无法再开。默认解：可见性改判「extra 存在 usage 键」（`"usage" in parseExtra(...)`），开关值仍绑 usageFlag。

### 17-03 useMatchTemplate 404 未吞（P2）

use-providers.ts:98-105：`await api.post(...).json()` 在 try 之外，ky 默认 throwHttpErrors（lib/api.ts:72-80 未关）→ 后端 404（provider_templates.rs:72）经 beforeError 抛 ApiError → queryFn 抛错。未命中是输入 URL 的常态 → 每键击一次失败 query + 全局 retry:1 放大为 2 请求 + 静默 error；注释声称「404 在此吞掉」不成立。另无防抖（ProviderEditDialog.tsx:147-150 逐键触发）。默认解：.json() 移入 try 或对 404 显式返回 []；顺带 300ms 防抖。波及面：仅 ProviderEditDialog 消费，安全。

### 17-04 不存在的 i18n key（P2）

ProviderDetail.tsx:146 `t("apiKeys.showKeyFailed")`；grep 两 locale 的 apiKeys 块无此键、全仓仅此一处引用；i18n 无 missingKeyHandler → toast 标题显示字面量 `apiKeys.showKeyFailed`。默认解：补词条或改用 common.error。

### 17-05 无保护 JSON.parse（P2，健壮）

ProviderDetail.tsx:253/268 两处 JSON.parse 仅 `!== "{}"` 守卫；ProviderResponse.extra 来自后端 `decrypt_or_passthrough`——密钥缺失/轮换时透传密文（非 JSON）→ 渲染抛错 → 整页 ErrorBoundary。同文件 usageEnabled 与 EditDialog parseExtra 都有 try/catch，独此遗漏。默认解：复用 parseExtra/包 try-catch。

### 17-06 候选卡内数字输入死控件（P2，交互）

AddProviderModelsDialog.tsx:547-577：manual/pending 候选整卡挂 role=button + onClick=jump + onKeyDown(Enter/Space→jump)；卡内数字输入（:595-638）无 stopPropagation（仅 Checkbox :583 有）。点击聚焦冒泡→jump 切手动 Tab→输入卸载；Enter 同理。而 manual/pending 正是需要手填 contextLength/maxOutputTokens 的态。现有测试全用 fireEvent.change 不产生 click/keydown（provider-models-dialogs.test.tsx:211-212 等）故未暴露。默认解：输入容器 stopPropagation，或 jump 下移到标题区，或 clickable 候选不渲染数字输入。

### 17-07 代理 @ 校验口径不齐（P3）

ProxyConfigFields.tsx:25-42 只查非空+http:// 前缀；后端 providers.rs:194-222 额外拒 `@`。前端放行后吃服务端 toast。默认解：proxySuperRefine 补 @ 规则。

### 17-08 truncate 违规（P3，规范）

ProviderEditDialog.tsx:308 模板候选按钮内 `<span className="truncate">`（全 scope 唯一）。默认解：换 MidEllipsis。

### 17-09 ApiKeyCell 错误静默（P3）

ApiKeyCell.tsx:23-55：详情 query 失败→detail undefined→永远掩码，无 toast/错误态。默认解：暴露 error 并 toast。

### 17-10 明文 Key 缓存策略不一（P3，安全/一致性）

ProviderDetail.tsx:99 注释「明文不进任何缓存」；ApiKeyCell.tsx:23 用 useApiKeyDetail（React Query 缓存）+ :32-37 queryClient.fetchQuery 写缓存，gcTime 5min。两处策略相悖（仅内存，风险有限）。默认解：detail query 设 gcTime:0 或复制走命令式。波及面：useApiKeyDetail 被 api-key-overview.tsx:46 消费（只取名称），改缓存策略需回归该页。

### 17-11 删除选中项后空白（P3）

pages/providers.tsx:29-30：`hasUserSelected` 后删除该项，selectedId 悬空 → 右侧空态不回落首个。默认解：providers 找不到 selectedId 时重置选择态。

### 17-12 测速弹窗全局拉模型（P3，性能）

ProviderSpeedTestDialog.tsx:42 useProviderModels() 全量 + :77-80 客户端过滤；弹窗在 providers.tsx:110-114 无条件挂载 → 打开供应商页即拉全量模型。后端有 `/{provider_id}/models` 作用域端点未用。默认解：作用域 query 或 open 门控 enabled。波及面：useProviderModels 是 16 域冻结共享 hook，改签名需协商；新增独立作用域 hook 则安全。

### 17-13 分页被 identity 重置（P3）

ApiKeysTable.tsx:184-186 `useEffect(..., [apiKeys])`：启停 invalidate → 新数组 identity → pageIndex=0。默认解：依赖改 apiKeys.length 或仅总页数下降时重置。

### 17-14 标签映射重复+死导出（P3）

ProviderDetail.tsx:44-54 手写 PROTOCOL_LABELS/BILLING_LABELS 与 ProtocolIcon.tsx 的 labelKey 同源；ProtocolIcon 导出的 billingModeLabel（:28-30）全仓零消费。默认解：ProviderDetail 改用 protocolLabel/billingModeLabel，回收死导出。

### 17-15 编辑态必填星号（P3，文案）

ProviderEditDialog.tsx:339 `<FormLabel required>` 无条件；编辑态留空=不修改（:215/:343 占位符自述）。默认解：`required={!isEdit}`。

### 17-16 用量卡 i18n 绕过（P3）

ProviderUsageCard.tsx:55 固定 `Intl.DateTimeFormat("zh-CN")`、:64 拼「月/日」、:202 `toLocaleTimeString("zh-CN")`。归 20 票 i18n 域一并处理，此记实例锚点。

### 17-17 模板 extra 默认值丢弃（P3）

ProviderEditDialog.tsx:200-206 applyTemplate 对可编辑键一律 `defaults[key]=""`（编辑模式 :177-182 会带模板值）；提交 :252-256 用 extraValues 覆盖模板值。当前种子全空占位无实害；模板带非空默认即被丢。默认解：defaults 取模板值兜底。

### 17-18 numberEdits 闭包覆盖（P3，健壮）

AddProviderModelsDialog.tsx:606-614/627-635：`{...edits, ...}` 用渲染闭包旧快照而非 `prev[id]`；同批两字段更新互相覆盖。实际逐事件触发+React 重渲染，面窄。默认解：改 `prev[candidate.providerModelId]` 展开。

### 17-19 详情弹窗滚动形态违规（P3，规范）

ProviderModelDetailDialog.tsx:191、VirtualModelItemDetailDialog.tsx:90：`DialogContent className="max-h-[85vh] overflow-y-auto"` 整窗滚动，标题/底部随之滚动；违反 AGENTS 固定头底约定（同族 AddProviderModelsDialog:435/VirtualModelEditDialog:461 做对了）。默认解：改 flex 三分布局（正好是定案 DialogScrollShell 的首批消费方）。

### 17-20 useDeleteProvider 漏失效 providerModelKeys（P3，失效面）

use-providers.ts:160-164 只失效 providerKeys+virtualModelKeys；后端删除供应商级联删 provider_model（providers.rs:668）；RequestLogsTable.tsx:157 的模型筛选消费 useProviderModels → 短暂展示已删模型。默认解：onSuccess 补 providerModelKeys.all。波及面：16 域冻结 hook 仅追加失效、不动签名，低风险。

### 17-21 「添加」硬编码（P3，i18n）

ProviderModelSection.tsx:69-72 按钮文案直写中文，同文件其余走 t()。默认解：走 providerModels.* key。

### 17-22 不可解析草稿成员仍提交（P3，逻辑）

VirtualModelEditDialog：mismatchedDrafts（:182-188）与 groupOf（:273-289）用 modelById（源自 providerModels prop）解析草稿，缺失即 undefined → 不成行、不计入候选；onSubmit（:301-314）仍从 draftItems 全量提交 → 后端 400「成员协议不匹配」而前端无提示；接口类型变更确认（:191-210）漏算这类成员。触发面窄（级联清理保证正常态完整）。默认解：提交前对不可解析成员兜底提示/剔除。

### 17-23 详情弹窗 refetch 丢编辑态（P3）

ProviderModelDetailDialog.tsx:117-134：reset effect 依赖 `[open, model, form]`，model 由页面 useMemo 派生自 react-query 数组——任何 refetch（失效/stale 后回焦）产新对象身份 → form.reset+setEditing(false)，编辑中改动静默丢失。默认解：依赖改 `model?.modelId`+open。

### 17-24 Add 弹窗无虚拟化（P3，性能）

AddProviderModelsDialog.tsx:543-642 候选全量 map，每卡 Checkbox+2 受控数字输入；大目录供应商（300+ 模型）刷新可感知卡顿。搜索只定位高亮不裁剪渲染集。默认解：按查询过滤渲染集或窗口化（观察级）。

### 17-25 VM 弹窗重复计算（P3，性能/简洁）

VirtualModelEditDialog.tsx:293-299 providers.map(groupOf) 两遍；groupOf→candidatesOf 内层 filter+some 约 O(2·P·M·D)；modelById/providerById 每渲染重建。默认解：算一次再 filter 分区 + useMemo 两个 Map。

### 17-26 三处近重复（P3，简洁）

CapabilityIcons.tsx:20-49 vs ItemCapabilityIcons.tsx:17-40 近同构（入参类型不同）；只读详情 dl（ProviderModelDetailDialog:292-362 vs VirtualModelItemDetailDialog:116-185）；手动表单（AddProviderModelsDialog:654-778 vs ProviderModelDetailDialog:209-290，schema 已由 provider-model-form 复用）。默认解：能力图标合一（结构化入参可覆盖），只读 dl 抽 ModelInfoList。

### 17-27 假防抖（P3，注释漂移）

AddProviderModelsDialog.tsx:125-127 注释称防抖，:187-192 同步 setState；useCatalogSearch（use-provider-models.ts:88-101）逐键发请求。默认解：加真防抖或改名改注释。

### 17-28 测试缺口族（P3）

已有：provider-models-dialogs 963 行 33 例、virtual-models-dialogs 833 行 19 例、virtual-model-interface-type 4 例、provider-models-page 11 例、virtual-models-page 13 例、provider-detail 22 例、provider-edit-dialog 3 例、provider-usage-card 10 例、api-keys-page 11 例、providers-list-reorder 6 例。缺口：①候选卡输入 click/keydown（17-06 根因=测试全用 fireEvent.change）；②批量选择矩阵+坏数字提交提示；③四条 onError 失败路径（createModel/batchCreate/VM onSubmit/VM 删除）；④接口类型变更确认全链路（mismatchedDrafts→确认→级联移除）与「协议不匹配候选隐藏」；⑤详情弹窗 protocolType Select 与能力开关提交；⑥pages/providers.tsx 整页无测试（默认选中/删除回落 17-11/弹窗接线）；⑦hooks 零直测（useMatchTemplate 404/useApiKeyDetail enabled/useProviderUsage refresh 拼装）；⑧窄面观察：ProviderModelDetailDialog:355-359 继承代理展示未门控 provider.proxyEnabled（VirtualModelItemDetailDialog:166-168 有门控），关闭代理未清空地址时误显示「继承」。

## 已核验无问题区（避免后续票重复审查）

- **弹窗固定头底约定**：大弹窗（ProviderEditDialog/AddProviderModelsDialog/VirtualModelEditDialog/SpeedTestDialog）全部 flex 三分正确（17-19 只命中两个小详情弹窗）。
- **删除确认**：三删除弹窗复用 ConfirmDialog，destructive+isLoading+成功关窗 toast；ProviderModelDetailDialog 删除用 flushSync 先卸嵌套弹窗防 body pointer-events 锁死（测试在）。
- **invalidation 面其余正确**：providers create/update/reorder、api-keys 增删启停、provider-models 增删改（额外失效 virtualModelKeys）、VM 增删改；reorder 乐观更新+onSettled 回滚；sort_order 不参与 LB 排序故无需额外失效。
- **已导入候选排除/去重在后端**（尾段忽略大小写），前端信任列表；批量非法值拦截 toast；刷新清空选中。
- **接口类型门控前后端口径一致**（acceptsProtocol/effectiveProtocol 对应 virtual_models.rs:742，后端不信任前端再校验）。
- **隐藏派生凭据保全**：编辑保存带出 refresh_token/jwt/usage_type 原值（3 用例）。
- **字段同步面**：Provider/ApiKey/ProviderModel/VM 手写类型与后端 DTO camelCase 吻合；protocolType null=跟随、interfaceType 0..4 对齐；duration_ms 裸 json 读取正确。注意：若 11-27 补 disabledReason，前端 Provider 类型需手工同步（无 codegen）。
- **条件 return null 都在 hooks 之后**，hooks 顺序稳定。
- **前端 custom_header 校验比后端严**（要求对象）只会少放行。
- **i18n key 面**：providerModels.*/virtualModels.* 两 locale 均在（17-04 是例外个案）。

## 性能/内存轮结论

无 P1/P2 性能项。主要浪费=17-03（每键击 2 请求且无防抖）与 17-12（页面挂载即全量拉模型）；Add 弹窗全量渲染（17-24）与 VM 弹窗重复计算（17-25）为观察级；ProviderUsageCard 的 refreshToken 换 key 短时堆积旧缓存条目（量微）。全局 staleTime 5min+手动刷新符合管理端形态。结论：修 17-03 顺带防抖是本域最高性价比性能项。
