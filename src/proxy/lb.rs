use super::*;

/// 上游协议（provider.protocol_type）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Protocol {
    OpenAiCompat,
    OpenAiResponses,
    Anthropic,
    Gemini,
}

impl Protocol {
    pub fn from_i32(value: i32) -> Self {
        match value {
            crate::provider_model::refresh::PROTOCOL_OPENAI_RESPONSE => Protocol::OpenAiResponses,
            crate::provider_model::refresh::PROTOCOL_ANTHROPIC => Protocol::Anthropic,
            crate::provider_model::refresh::PROTOCOL_GEMINI => Protocol::Gemini,
            _ => Protocol::OpenAiCompat,
        }
    }
}

/// 成员请求失败后记连续失败（所有失败，含 4xx）；达到设置项
/// `max_consecutive_failures` 阈值时由可用性状态机熔断停用供应商（计数、
/// 阈值判断与状态迁移见 `availability::on_forward_failure`）。
/// `counted` 为本次请求已计数的 provider 集合：同一请求内同一供应商的多个
/// 成员失败只计一次，避免一次降级链把计数顶到阈值。
pub(crate) async fn note_member_failure(
    state: &AppState,
    member: &Member,
    request_id: &str,
    counted: &mut HashSet<i32>,
) {
    if !counted.insert(member.provider_id) {
        return;
    }
    let threshold = state.settings.max_consecutive_failures().await;
    if let Err(e) = crate::availability::on_forward_failure(
        &state.db,
        &state.failure_counter,
        member.provider_id,
        threshold,
        request_id,
    )
    .await
    {
        tracing::warn!(
            request_id,
            provider_id = member.provider_id,
            "连续失败熔断执行失败：{e}"
        );
    }
    // 失败复查（异步节流）：耗尽即门控禁用，切断缓存过期导致的后续降级。
    failure_recheck::trigger(state, member.provider_id, request_id);
}

/// LB 轮转状态：虚拟模型 id → 已轮转次数。
#[derive(Clone, Default)]
pub struct LbState {
    counters: Arc<Mutex<HashMap<i32, u64>>>,
}

impl LbState {
    fn next_offset(&self, virtual_model_id: i32, len: usize) -> usize {
        if len == 0 {
            return 0;
        }
        let mut counters = self.counters.lock().expect("lb counters lock");
        let counter = counters.entry(virtual_model_id).or_insert(0);
        let offset = (*counter % len as u64) as usize;
        *counter += 1;
        offset
    }
}

/// 参与转发的一个成员（供应商 + 真实模型）。
#[derive(Debug, Clone)]
pub(crate) struct Member {
    pub(crate) provider_id: i32,
    /// 发给上游的真实模型 ID（provider_model.provider_model_id）。
    pub(crate) model_id: String,
    pub(crate) protocol: Protocol,
    /// 0=按量付费，1=订阅制。
    pub(crate) billing_mode: i32,
    pub(crate) base_url: String,
    pub(crate) api_key_encrypted: String,
    pub(crate) custom_header: String,
    /// 该成员最终生效的网络代理（模型级优先，其次供应商级；都未开启则直连）。
    pub(crate) proxy_enabled: bool,
    /// HTTP 代理地址（如 `http://127.0.0.1:7890`）。
    pub(crate) proxy_addr: String,
}

/// 解析成员最终代理：模型级开启且地址有效 → 用模型地址；否则供应商级开启
/// 且地址有效 → 用供应商地址；都没有 → 直连。
pub(crate) fn resolve_proxy(
    model: &provider_model::Model,
    provider: &provider::Model,
) -> (bool, String) {
    if model.proxy_enabled && !model.proxy_addr.trim().is_empty() {
        (true, model.proxy_addr.clone())
    } else if provider.proxy_enabled && !provider.proxy_addr.trim().is_empty() {
        (true, provider.proxy_addr.clone())
    } else {
        (false, String::new())
    }
}

/// 加载虚拟模型全部可用成员（item 启用 + 供应商启用且状态可用）。
pub(crate) async fn load_members(
    db: &DatabaseConnection,
    virtual_model_id: i32,
) -> Result<Vec<Member>, sea_orm::DbErr> {
    let items = virtual_model_item::Entity::find()
        .filter(virtual_model_item::Column::VirtualModelId.eq(virtual_model_id))
        .filter(virtual_model_item::Column::Enable.eq(true))
        .order_by_asc(virtual_model_item::Column::VirtualModelItemId)
        .all(db)
        .await?;
    if items.is_empty() {
        return Ok(Vec::new());
    }

    let model_pks: Vec<i32> = items.iter().map(|item| item.model_id).collect();
    let models = provider_model::Entity::find()
        .filter(provider_model::Column::ModelId.is_in(model_pks.clone()))
        .all(db)
        .await?;
    let model_by_pk: HashMap<i32, provider_model::Model> =
        models.into_iter().map(|m| (m.model_id, m)).collect();

    let provider_ids: Vec<i32> = {
        let ids: HashSet<i32> = model_by_pk.values().map(|m| m.provider_id).collect();
        ids.into_iter().collect()
    };
    let providers = provider::Entity::find()
        .filter(provider::Column::Id.is_in(provider_ids))
        .all(db)
        .await?;
    let provider_by_id: HashMap<i32, provider::Model> =
        providers.into_iter().map(|p| (p.id, p)).collect();

    Ok(items
        .iter()
        .filter_map(|item| {
            let model = model_by_pk.get(&item.model_id)?;
            let p = provider_by_id.get(&model.provider_id)?;
            // 实体层可用性（启用 ∧ 无停用原因）统一经 availability 读侧谓词；
            // 用量层剔除见 order_members（UsageData 判定，缓存 10 分钟新鲜度）。
            if !crate::availability::traffic_available(p) {
                return None;
            }
            let (proxy_enabled, proxy_addr) = resolve_proxy(model, p);
            // 协议优先级：模型单独指定（非空）→ 供应商协议。与代理同款覆盖语义。
            let protocol_value = model.protocol_type.unwrap_or(p.protocol_type);
            Some(Member {
                provider_id: p.id,
                model_id: model.provider_model_id.clone(),
                protocol: Protocol::from_i32(protocol_value),
                billing_mode: p.billing_mode,
                base_url: p.base_url.clone(),
                api_key_encrypted: p.api_key.clone(),
                custom_header: p.custom_header.clone(),
                proxy_enabled,
                proxy_addr,
            })
        })
        .collect())
}

/// 把用量数据格式化为可读字符串，用于 LB 决策日志：
/// - 订阅制：`quota[5h:92.4%,week:70.3%,mon:85.2%]`（无数据的窗口显示 `-`）
/// - 按量：`balance=7.65`（余额合计）
/// - 无数据：`no-usage`
pub(crate) fn format_usage(data: Option<&UsageData>) -> String {
    let Some(data) = data else {
        return "no-usage".to_string();
    };
    match data.kind {
        UsageKind::Quota => {
            let parts: Vec<String> = [
                WindowKind::FiveHour,
                WindowKind::Weekly,
                WindowKind::Monthly,
            ]
            .iter()
            .map(|kind| {
                let pct = data
                    .windows
                    .iter()
                    .find(|w| w.window == *kind)
                    .and_then(|w| w.remaining_percent_value());
                match pct {
                    Some(p) => format!("{p}"),
                    None => "-".to_string(),
                }
            })
            .collect();
            format!("quota[5h:{},week:{},mon:{}]", parts[0], parts[1], parts[2])
        }
        UsageKind::Balance => format!("balance={:.2}", usage_rank::balance_amount(Some(data))),
    }
}

/// 按虚拟模型的负载均衡策略排序成员。
///
/// 策略 0/1 分组后做组内用量感知排序（订阅制按截止时间优先，更上层截止链全平
/// 回退剩余百分比；按量付费按剩余金额降序），用量优先取 10 分钟数据库缓存，缺失/
/// 过期才真实抓取。排序结果即 failover 优先级（`forward_chat` 按 ordered 顺序
/// 逐个重试）。策略 2/3 保持轮转/随机。
pub(crate) async fn order_members(
    state: &AppState,
    members: Vec<Member>,
    strategy: i32,
    lb_state: &LbState,
    virtual_model_id: i32,
    request_id: &str,
) -> Vec<Member> {
    match strategy {
        // 订阅制优先 / 按量优先：先按付费模式分组，再组内按用量排序。
        0 | 1 => {
            let subscription_first = strategy == 0;
            let mut subs = Vec::new();
            let mut payg = Vec::new();
            for member in members {
                let group = if member.billing_mode == 1 {
                    &mut subs
                } else {
                    &mut payg
                };
                group.push(member);
            }
            // 决策过程日志：先打印订阅制与按量两组所有成员的用量明细，
            // 再打印排序后的顺序，便于事后还原「为什么选它」。
            // 用量一次解析全部成员（10 分钟缓存/抓取），两组共用同一份。
            let usage_map = resolve_usage_map(
                state,
                &subs.iter().chain(&payg).cloned().collect::<Vec<_>>(),
            )
            .await;
            let member_detail = |member: &Member, usage: &HashMap<i32, Option<UsageData>>| {
                format!(
                    "{}:{} billing={} {}",
                    member.provider_id,
                    member.model_id,
                    member.billing_mode,
                    format_usage(usage.get(&member.provider_id).and_then(Option::as_ref)),
                )
            };
            let subs_desc: Vec<String> =
                subs.iter().map(|m| member_detail(m, &usage_map)).collect();
            let payg_desc: Vec<String> =
                payg.iter().map(|m| member_detail(m, &usage_map)).collect();
            tracing::info!(
                request_id,
                virtual_model_id,
                strategy,
                subscription_first,
                subscription_members = ?subs_desc,
                payg_members = ?payg_desc,
                "LB 决策：成员用量明细",
            );

            let mut subs = rank_by_quota_with(state, subs, &usage_map).await;
            let mut payg = rank_by_balance_with(state, payg, &usage_map).await;
            // 订阅制额度耗尽即跳过：任一已提供窗口剩余为 0 的订阅成员视为当前
            // 不可用，从候选里剔除，让位给还有额度的订阅成员或按量成员；无法
            // 判定（无窗口数据）的保持原状。判定口径唯一来源：
            // `UsageData::subscription_usable`（用量门控与恢复探测同源调用）。
            let mut skipped: Vec<String> = Vec::new();
            subs.retain(|m| {
                let usable = usage_map
                    .get(&m.provider_id)
                    .and_then(Option::as_ref)
                    .and_then(UsageData::subscription_usable);
                match usable {
                    Some(false) => {
                        skipped.push(member_detail(m, &usage_map));
                        false
                    }
                    _ => true,
                }
            });
            // 按量付费余额耗尽即跳过：查得到余额且合计为 0 的按量成员不可用
            // （口径同 `UsageData::balance_usable`），从候选剔除；查不到余额
            // （无法判定）的保持原状。
            let mut skipped_balance: Vec<String> = Vec::new();
            payg.retain(|m| {
                let usable = usage_map
                    .get(&m.provider_id)
                    .and_then(Option::as_ref)
                    .and_then(UsageData::balance_usable);
                match usable {
                    Some(false) => {
                        skipped_balance.push(member_detail(m, &usage_map));
                        false
                    }
                    _ => true,
                }
            });
            let (first_group, second_group) = if subscription_first {
                (subs.as_slice(), payg.as_slice())
            } else {
                (payg.as_slice(), subs.as_slice())
            };
            let ordered_desc: Vec<String> = first_group
                .iter()
                .chain(second_group)
                .map(|m| member_detail(m, &usage_map))
                .collect();
            tracing::info!(
                request_id,
                virtual_model_id,
                strategy,
                skipped_quota_exhausted = ?skipped,
                skipped_balance_exhausted = ?skipped_balance,
                ordered = ?ordered_desc,
                "LB 决策：排序结果",
            );

            if subscription_first {
                subs.append(&mut payg);
                subs
            } else {
                payg.append(&mut subs);
                payg
            }
        }
        // RoundRobin
        2 => {
            let len = members.len();
            if len <= 1 {
                return members;
            }
            let offset = lb_state.next_offset(virtual_model_id, len);
            let mut rotated: Vec<Member> = members[offset..].to_vec();
            rotated.extend_from_slice(&members[..offset]);
            rotated
        }
        // Random
        3 => {
            let mut shuffled = members;
            shuffled.shuffle(&mut rand::thread_rng());
            shuffled
        }
        _ => members,
    }
}

/// 订阅制组内排序：截止时间优先（FEFO）——逐层检查双方该层都有额度（剩余 > 0），
/// 有则比较更上层截止时间（早的优先），截止链全平回退剩余百分比；层内无额度判平
/// 进下一层。详见 `usage_rank::cmp_quota_deadline_priority`。
/// 先 shuffle 再稳定排序，全部平局的成员保持随机相对顺序（即“同等条件随机选一个”）。
/// `usage` 由调用方已解析（决策日志共用同一份，避免重复抓取）。
pub(crate) async fn rank_by_quota_with(
    _state: &AppState,
    mut members: Vec<Member>,
    usage: &HashMap<i32, Option<UsageData>>,
) -> Vec<Member> {
    if members.len() <= 1 {
        return members;
    }
    members.shuffle(&mut rand::thread_rng());
    // 自然序比较器（a vs b）；sort_by 升序，因此交换参数实现「截止更近/剩余更多的在前」。
    members.sort_by(|a, b| {
        usage_rank::cmp_quota_deadline_priority(
            usage.get(&b.provider_id).and_then(Option::as_ref),
            usage.get(&a.provider_id).and_then(Option::as_ref),
        )
    });
    members
}

/// 按量付费组内排序：剩余金额合计降序（同额保持原序）。
/// `usage` 由调用方已解析（决策日志共用同一份，避免重复抓取）。
pub(crate) async fn rank_by_balance_with(
    _state: &AppState,
    mut members: Vec<Member>,
    usage: &HashMap<i32, Option<UsageData>>,
) -> Vec<Member> {
    if members.len() <= 1 {
        return members;
    }
    members.sort_by(|a, b| {
        usage_rank::cmp_balance(
            usage.get(&b.provider_id).and_then(Option::as_ref),
            usage.get(&a.provider_id).and_then(Option::as_ref),
        )
    });
    members
}

/// 收集成员用量：10 分钟数据库缓存新鲜即用；缺失/过期并发真实抓取并落库，
/// 抓取失败按无数据处理（排在本组末尾）。
pub(crate) async fn resolve_usage_map(
    state: &AppState,
    members: &[Member],
) -> HashMap<i32, Option<UsageData>> {
    let mut seen = HashSet::new();
    let provider_ids: Vec<i32> = members
        .iter()
        .map(|m| m.provider_id)
        .filter(|id| seen.insert(*id))
        .collect();

    let mut map = HashMap::new();
    let mut stale = Vec::new();
    for id in provider_ids {
        let cached = read_usage_cache(&state.db, id).await.ok().flatten();
        if let Some(data) = cached {
            map.insert(id, Some(data));
        } else {
            stale.push(id);
        }
    }
    if stale.is_empty() {
        return map;
    }

    let mut set = tokio::task::JoinSet::new();
    for id in stale {
        let db = state.db.clone();
        set.spawn(async move { (id, fetch_and_store(&db, id).await.ok()) });
    }
    while let Some(outcome) = set.join_next().await {
        if let Ok((id, data)) = outcome {
            map.insert(id, data);
        }
    }
    map
}
impl Member {
    pub(crate) fn protocol_code(&self) -> i32 {
        match self.protocol {
            Protocol::OpenAiCompat => crate::provider_model::refresh::PROTOCOL_OPENAI_COMPATIBLE,
            Protocol::OpenAiResponses => crate::provider_model::refresh::PROTOCOL_OPENAI_RESPONSE,
            Protocol::Anthropic => crate::provider_model::refresh::PROTOCOL_ANTHROPIC,
            Protocol::Gemini => crate::provider_model::refresh::PROTOCOL_GEMINI,
        }
    }
}
