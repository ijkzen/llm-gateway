use super::*;

/// 路由解析产物：虚拟模型行 + LB 排序后的成员列表 + 降级开关，
/// 交由 failover 核心的尝试循环消费。
pub(crate) struct ResolvedRoute {
    pub(crate) virtual_model: virtual_model::Model,
    pub(crate) ordered: Vec<Member>,
    pub(crate) retry_enabled: bool,
}

/// 路由解析失败（两入口各自映射为自己的协议错误信封）。
pub(crate) enum RouteError {
    /// display_id 未命中或被接口类型门拒绝（一律按模型不存在处理）。
    NotFound,
    /// 数据库查询失败（虚拟模型/成员）。
    QueryFailed(String),
    /// 成员列表为空（无任何可用成员）。
    NoMembers,
}

/// 路由解析前半段（forward_chat/forward_native 共用）：display_id 精确
/// 路由 → 接口类型门（accept）→ 成员加载与附加过滤（member_keep）→
/// 空成员拒绝 → LB 排序 + 选路日志（统一形状：debug 排序明细 +
/// info 选路结果）。
pub(crate) async fn resolve_and_order(
    state: &AppState,
    request_id: &str,
    requested_model: &str,
    accept: impl Fn(&virtual_model::Model) -> bool,
    member_keep: impl Fn(&Member) -> bool,
) -> Result<ResolvedRoute, RouteError> {
    // 路由：display_id 精确匹配（鉴权失败与路由未命中不落 request 表）。
    let virtual_model = virtual_model::Entity::find()
        .filter(virtual_model::Column::DisplayId.eq(requested_model))
        .filter(virtual_model::Column::Enable.eq(true))
        .one(&state.db)
        .await
        .map_err(|e| RouteError::QueryFailed(format!("查询虚拟模型失败：{e}")))?
        .ok_or(RouteError::NotFound)?;
    if !accept(&virtual_model) {
        return Err(RouteError::NotFound);
    }

    let members = load_members(&state.db, virtual_model.virtual_model_id)
        .await
        .map_err(|e| RouteError::QueryFailed(format!("查询模型成员失败：{e}")))?;
    let members: Vec<_> = members.into_iter().filter(|m| member_keep(m)).collect();
    if members.is_empty() {
        return Err(RouteError::NoMembers);
    }

    let ordered = order_members(
        state,
        members,
        virtual_model.load_balancing_strategy,
        &state.lb_state,
        virtual_model.virtual_model_id,
        request_id,
    )
    .await;
    let retry_enabled = virtual_model.fallback_strategy == 1;

    // 负载均衡决策日志：选路结果每请求 1 条 info；完整排序明细 debug
    //（默认 RUST_LOG=info 不输出，深排时临时调 debug）。
    let ordered_desc: Vec<String> = ordered
        .iter()
        .map(|m| format!("{}:{}", m.provider_id, m.model_id))
        .collect();
    tracing::debug!(
        request_id,
        virtual_model_id = virtual_model.virtual_model_id,
        requested_model = %requested_model,
        strategy = virtual_model.load_balancing_strategy,
        member_order = ?ordered_desc,
        "LB 排序明细",
    );
    if let Some(first) = ordered.first() {
        tracing::info!(
            request_id,
            virtual_model_id = virtual_model.virtual_model_id,
            requested_model = %requested_model,
            strategy = virtual_model.load_balancing_strategy,
            member_count = ordered.len(),
            selected_provider_id = first.provider_id,
            selected_model_id = %first.model_id,
            "LB 选路结果",
        );
    }

    Ok(ResolvedRoute {
        virtual_model,
        ordered,
        retry_enabled,
    })
}
