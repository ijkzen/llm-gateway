# llm-gateway

Rust + React 单体网关，内嵌管理后台。当前域词汇以供应商接入为主，随会话逐步补充。

## Language

### 供应商域

**供应商 (Provider)**:
一个可调用的上游接入实例：base_url、api_key（加密存储）与协议类型的组合。
_Avoid_: 模型提供商、模型供应商、上游、渠道

**供应商 extra (Provider Extra)**:
provider 表 `extra` 列承载的 JSON 对象：供应商敏感凭据（ak/sk、refresh_token、oauth_token、cookie_cloud 系密码）与用量开关（usage/usage_type）。整段 AES-256-GCM 加密落库（`enc:v1:`，密钥与 api_key 同源），API 层解密/透传后返回；所有写入口（CRUD、动态凭据轮换、模板补齐/历史回填、备份导入）落库前统一经 `crypto::encrypt`（ADR-0002）。
_Avoid_: 附加配置、扩展字段

**供应商模板 (ProviderTemplate)**:
按 base_url 域名匹配的预设接入参数（协议、付费模式、用量字段），创建供应商时套用。
_Avoid_: 预设、厂商

**供应商模型 (ProviderModel)**:
登记在某个供应商名下的具体模型条目：远端模型 ID 字符串、上下文长度、最大输出与四项能力开关。
_Avoid_: 模型（裸称）、上游模型

**网络代理 (Network Proxy)**:
provider 或 provider_model 的可选出口代理（`proxy_enabled`+`proxy_addr`）。转发与测速按「模型级 → 供应商级 → 直连」解析生效代理（`resolve_proxy`，模型级覆盖供应商级）；用量抓取/模型刷新/连通性测试等旁路请求只认供应商级代理，不受模型级影响；http 与 https 目标一律经代理（Proxy::all），禁止仅 http 代理配置。模型只读态显示「继承供应商代理」表示回落供应商而非直连（ADR-0013）。
_Avoid_: 出口代理（裸称）

**模型目录 (Model Catalog)**:
内嵌于二进制的 models.dev 全量模型元数据（363 条），是智能填充的唯一数据源。
_Avoid_: models.json（实现指代）、知识库

**远端模型刷新 (Model Refresh)**:
按供应商协议调用其 Models 接口，拉取远端模型 ID 列表作为待导入候选。
_Avoid_: 同步、拉取、刷新（裸称，与页面刷新混淆）

**候选模型 (Candidate)**:
远端模型刷新返回的、尚未导入该供应商的远端模型条目，在添加弹窗中可勾选批量导入。
_Avoid_: 远端模型（裸称）

**智能填充 (Smart Fill)**:
拿候选模型 ID 与模型目录条目做尾段匹配，命中则用目录数据补全详情；补全完整记绿色「已智能填充」，目录条目缺 limit 记黄色「信息不完整」，未命中记「需手动填写」。
_Avoid_: 自动匹配、AI 填充

**尾段匹配 (Suffix Match)**:
两个模型 ID 各自按 `/` 分割取最后一段，忽略大小写做精确相等比较。
_Avoid_: 模糊匹配、前缀匹配

### 虚拟模型域

**虚拟模型 (VirtualModel)**:
对外暴露的模型条目：display_id、负载均衡策略与降级策略的组合，聚合多个供应商模型成员；禁用后不出现在 /v1/models。
_Avoid_: 聚合模型、组合模型、逻辑模型

**display_id (Display ID)**:
客户端调用时使用的虚拟模型标识，全局唯一；与供应商模型的远端模型 ID（provider_model_id）相互独立。
_Avoid_: 模型名、别名、虚拟 ID

**虚拟模型条目 (VirtualModelItem)**:
虚拟模型名下的成员映射：一个供应商模型条目 + 独立启用开关；实际可用性 = 条目启用 ∧ 所属供应商「选路可用」∧ 成员协议匹配（生效协议 = 模型级协议覆盖 > 供应商，匹配语义见「接口类型」）。级联停用打 `cascade_disabled` 标记，级联恢复只动带标记条目——用户手动关闭的成员不被自动恢复（见「停用原因」）。
_Avoid_: 成员（裸称）、子模型

**互斥映射 (Exclusive Mapping)**:
一个供应商模型最多归属一个虚拟模型；已被占用的模型不出现在其他虚拟模型的可选列表中，删除虚拟模型即释放其成员。
_Avoid_: 多对多、共享映射

**负载均衡策略 (Load Balancing Strategy)**:
虚拟模型在多个可用成员间分配请求的规则：订阅制优先(0)、按量付费优先(1)、轮转(2)、随机(3)，均已执行。0/1 先按付费模式分组，组内用量感知排序（订阅 FEFO 截止链 / 按量主余额），额度耗尽的成员先剔除（判定口径同「选路可用」，排序细节见 ADR-0011）；2/3 轮转/随机基于候选成员静态序。成员展示序与选路序同源（编辑弹窗保持后端序）。
_Avoid_: 调度策略、分流策略

**降级策略 (Fallback Strategy)**:
成员调用失败时的处理规则：直接失败(0)、依次重试本虚拟模型内其他被启用的成员(1)。
_Avoid_: 容错策略、重试策略

**接口类型 (Interface Type)**:
virtual_model 对外端点的协议形态（`interface_type`）：0=OpenAI Compat / 1=OpenAI Responses / 2=Anthropic Messages / 3=Gemini（保留值，拒绝创建）/ 4=Full Compatible（任意协议成员 + chat/completions 转换聚合；历史存量回填默认 4，新建默认 0）。端点与类型严格对应：`/v1/chat/completions` 接 0/4、`/v1/responses` 仅 1、`/v1/messages` 仅 2，`/v1/models` 只出 0/4；受限类型成员按生效协议匹配，接口类型/协议变更级联移除不匹配成员（Full Compatible 豁免）（ADR-0005）。
_Avoid_: 协议类型（provider/模型级覆盖列，指上游协议）、接口协议

### API Key 域

**API Key (ApiKey)**:
调用方访问网关的凭证：名称、服务端自动生成的密钥（`lg-` + 32 位 hex，加密存储）与启用开关的组合；用于后续 /v1 接口鉴权。
_Avoid_: 令牌、Token、访问密钥

**key 掩码 (Key Masking)**:
列表/创建响应中密钥的脱敏展示：保留前 3 位与后 4 位，中间以 `****` 填充；明文仅通过详情接口返回。
_Avoid_: 脱敏串、打码（裸称）

### 用量域

**用量窗口 (Quota Window)**:
订阅制用量按固定周期切分的观察单位（5 小时/日/周/月等），每个窗口含 limit/used/剩余与重置时间 `resets_at`；厂商不提供某窗口时 `available=false`。供应商「订阅额度耗尽」= 任一已提供窗口剩余为 0（`subscription_usable`）；窗口级「同类取最差剩余」的扫描收敛在 `UsageData::worst_window`，与 FEFO 排序共用；无时区厂商的重置时间按设置表时区解释（ADR-0009/0011）。窗口归属标注见「窗口标签」。
_Avoid_: 周期、额度段

**积分池 (Pool)**:
商汤 SenseNova Token Plan 内的独立额度容器。一个套餐可含多个池：default 通用池覆盖多模型，dedicated 专属池只覆盖特定模型，各自拥有独立的 5 小时与 7 天窗口。展示逐池独立；额度门控沿用逐窗口判定——任一池任一窗口剩余为 0 即视为该供应商额度耗尽。
_Avoid_: 额度池、资源池

**窗口标签 (Window Label)**:
用量窗口的可选标注，说明该窗口属于哪个容量容器（如积分池名）；无标签即厂商整体口径（现有所有厂商）。
_Avoid_: 池名（裸称）、分组名

**用量缓存 (Usage Cache)**:
供应商用量的数据库缓存（`provider_usage_cache` 表）：真实抓取结果落库，10 分钟内直出，过期/缺失才真实抓取，`?refresh=1` 强制重取，更新/删除供应商时失效；定时任务 `usage_refresh` 全量刷新（含停用供应商）。展示与 LB 排序只读缓存不触发抓取，失败复查结果也回写缓存（ADR-0009）。
_Avoid_: 实时用量、抓取结果（裸称）

### 可用性域

**停用原因 (Disabled Reason)**:
供应商停用时的机器可读原因，四值：正常（None）/连续失败禁用/额度耗尽/手动停用。与 enable 互为镜像（启用 ⇔ None），由 availability 模块统一写入保证一致；额度刷新只自动恢复「额度耗尽」的供应商，手动停用不被任何自动流程解除，连续失败禁用仅由恢复探测或手动启用解除。
_Avoid_: failure_disabled（旧布尔列，已由本字段取代）

**额度门控禁用 (Quota Gate Disable)**:
因订阅额度耗尽（任一已提供窗口剩余为 0）或按量余额耗尽而自动停用供应商连同名下全部虚拟模型条目；订阅窗口剩余百分比落在 (0,1) 边界区（未耗尽但逼近耗尽）时先发最小测试请求实测，失败同样停用（quota 标记）、成功保持或解除停用。恢复通道有二：窗口余量回升离开边界区（≥1%），或边界实测成功——均由用量定时刷新自动反向恢复；manual/failure 态不被触碰（ADR-0010）。
_Avoid_: 自动禁用（裸称）、熔断

**连续失败禁用 (Failure Disabled)**:
因转发失败连击达到阈值而自动停用供应商连同名下全部虚拟模型条目；与额度门控禁用来源不同，普通用量刷新不会自动恢复它，仅管理员手动启用或自动恢复探测成功时解除。恢复即回到正常参与选路的状态。
_Avoid_: 熔断、黑名单、封禁

**连续失败计数 (Consecutive Failure Count)**:
供应商粒度的失败连击数：该供应商任一转发请求失败（不论失败能否重试）加一，任一请求成功即清零，进程重启清零；达到设置的最大连续失败次数即触发连续失败禁用。
_Avoid_: 错误率、失败总数

**失败复查 (Failure Recheck)**:
转发失败后对支持用量查询的供应商异步发起的实时用量核验：确认耗尽则转入额度门控禁用（可自动恢复），确认充足则维持连续失败计数路径；核验结果同时回写用量缓存供后续选路使用。
_Avoid_: 实时刷新、重查、二次确认

**自动恢复探测 (Automatic Recovery Probe)**:
对连续失败禁用的供应商定期执行的健康确认：可查询用量者须先确认仍有剩余，再通过一次真实模型请求验证；成功即解除连续失败禁用并恢复级联停用的虚拟模型条目。
_Avoid_: 用量恢复、自动重试、半开

**选路可用 (Traffic-Eligible)**:
供应商「此刻能否参与选路」的只读判定，分两层：实体层 = 启用 ∧ 停用原因为 None（读侧统一经 availability 模块 `traffic_available` 谓词，调用点不再各自拼 `enable`/`disabled_reason` 组合；写入侧镜像不变式由该模块动作保证）；用量层 = 按付费模式经 `UsageData::subscription_usable` / `balance_usable` 判定，查不到用量（无法判定）视为可用避免上游抖动误伤。窗口级「同类取最差剩余」扫描收敛在 `UsageData::worst_window`，额度耗尽判定与 FEFO 排序共用，两口径一致性有测试锁定。
_Avoid_: 熔断（状态变更动作，见连续失败禁用）、可用状态（裸称）

**用量预估 (Usage Estimate)**:
订阅制供应商「整个订阅周期 token 总量」的折算预估：网关 request 表在窗口内已用 token ÷ 用量卡已用比例（used/limit 优先、used_percent 兜底），周窗前端 ×4 折月。窗口起点由 resets_at 反推（周 −7 天 / 月 −30 天），统计上界 min(resets_at, now)。信任边界显式化：网关记录 > 0 且比例可折算才可预估（流量未全走网关时 0 记录不可折算）；按天覆盖检查不参与算术。折算算术收敛在 `usage::estimate` 纯核心。
_Avoid_: 配额预估（裸称）、月用量

### 统计域

**快照桶 (Snapshot Bucket)**:
`request_log_snapshot` 的预聚合单位：按设置表时区对齐的闭口时间片（小时/天/月/年四级，ADR-0021），每个闭桶内存「主体 × 指标名 × 数值」行；生成后内容不再变化。四级独立物化、各自直算 request 表，不从细粒度行滚粗。
_Avoid_: 窗口（查询侧的任意 [起,止) 区间，由多个桶拼接）

**闭桶 (Closed Bucket)**:
终点时刻 + 固化余量（60 分钟）已过的快照桶——其请求行集合不再增长，可安全固化（事务性整批写入，含零流量哨兵行）；读取时闭桶只从快照取数。持续超过 60 分钟的流式请求若跨过固化时刻才落库，会从该小时桶漏记（更粗粒度桶覆盖不受影响）。
_Avoid_: 历史桶、finalized（裸称）

**实时兑底 (Live Fallback)**:
统计读路径的第一规则：窗口内未被快照覆盖的部分（未闭桶、哨兵行缺失、时区/边界不匹配）一律实时聚合 request 表——快照只是加速层，绝不因快照缺失返回残缺数字。
_Avoid_: 兜底查询、fallback（转发降级用词，勿混）

**统计主体 (Stats Entity)**:
快照行归属的聚合维度：全局（whole）、供应商、供应商模型、虚拟模型、API Key；组合型需求由主体交叉相乘派生新主体类型（如虚拟模型成员 = 虚拟模型 × 供应商模型），复合键用英文逗号连接。主体键一律用对应表的主键 id（供应商/供应商模型/虚拟模型/API Key 均如此，不用名称字符串）：生成时由 request 行的 provider_id+model_id、api_key_name 映射，映射不到（模型/Key 已删除）的请求不计入该主体行、仍计入 whole/provider 行。
_Avoid_: 维度、实体行

**哨兵行 (Sentinel Row)**:
每个闭桶必写的 whole 全局行（空流量也写全 0）：「存在哨兵行」=「该桶已固化」，是图表补零、自愈缺口检测与时区重算的依据。

### 转发与协议域

**思考载体 (Reasoning Details)**:
上游思考输出的客户端侧载体（OpenRouter 兼容格式，ADR-0006）：非流式装 `message`、流式装 `delta.reasoning_details`，条目为 `reasoning.text`（明文）/ `reasoning.encrypted`（加密签名）并带 `format` 标记来源（anthropic-claude-v1 / openai-responses-v1 / google-gemini-v1）。网关把 Anthropic thinking+signature/redacted_thinking、Responses reasoning item encrypted_content（请求侧恒带 include）、Gemini thoughtSignature 捕获装入该载体；客户端把 assistant 消息原样回传后按 format 注入对应上游载体，跨 format/failover 换厂商 debug 丢弃，OpenAI Compat 直通剥离该字段（存量 `reasoning_content` 归一不受影响）。
_Avoid_: 思考块（裸称）、thinking 字段（实现指代）

### 前端域

**页面刷新 (Page Refresh)**:
顶栏刷新按钮的语义：清空全部非布局级前端查询缓存（登录态与健康检查除外）并重新取数当前页面；其它页面的缓存随之一并清空，切页时重新请求。
_Avoid_: 缓存刷新、全量刷新、刷新（裸称，与远端模型刷新混淆）
