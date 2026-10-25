//! OpenDesign（open-design.ai）用量查询。
//!
//! 推理网关（`amr-link.open-design.ai/v1`）与控制面（`amr-api.open-design.ai`）
//! 分离：控制面按工作区读取订阅窗口与钱包余额，凭据为 Control Key
//! （`extra.control_key`）+ 工作区 ID（`extra.workspace_id`），推理 API Key
//! 不能替代。两端点独立读取、单边失败保留另一侧；套餐窗口耗尽后仍可由钱包
//! 余额继续扣费，因此窗口与钱包同响应返回（混合形态），可用性判定见
//! `UsageData::subscription_usable`。

use std::collections::HashSet;

use serde_json::{Map, Value};

use super::{Credentials, num, reset_ts, snippet};
use crate::usage::error::UsageError;
use crate::usage::http::{HttpReply, UsageHttp, is_session_invalid_status};
use crate::usage::types::{
    BalanceItem, FetchOutput, QuotaWindow, WindowKind, empty_windows, set_window,
};

const CONTROL_BASE: &str = "https://amr-api.open-design.ai/api/v1";

struct Preflight {
    eligible: bool,
    windows: Vec<QuotaWindow>,
    balance_usd: Option<f64>,
    member: Option<String>,
}

struct Wallet {
    item: BalanceItem,
    member: String,
}

/// 读取套餐额度与钱包余额；任一侧失败保留另一侧。
pub async fn fetch_opendesign(
    http: &UsageHttp,
    creds: &Credentials<'_>,
) -> Result<FetchOutput, UsageError> {
    let workspace = creds.require("workspace_id")?;
    if !valid_identifier(workspace) {
        return Err(UsageError::Parse("workspace_id 格式无效".to_string()));
    }
    let control_key = creds.require("control_key")?;
    let headers = [
        ("Authorization", format!("Bearer {control_key}")),
        ("x-vela-workspace-id", workspace.to_string()),
        ("Accept", "application/json".to_string()),
    ];
    let base = format!("{CONTROL_BASE}/workspaces/{workspace}");
    let quota_url = format!("{base}/billing/preflight");
    let wallet_url = format!("{base}/wallet/balance");
    let (quota_reply, wallet_reply) = tokio::join!(
        http.get(&quota_url, &headers),
        http.get(&wallet_url, &headers),
    );

    let quota = quota_reply
        .and_then(read_ok)
        .and_then(|body| parse_preflight(&body, workspace));
    let wallet = wallet_reply
        .and_then(read_ok)
        .and_then(|body| parse_wallet(&body, workspace));

    match (quota, wallet) {
        (Ok(quota), Ok(wallet)) => {
            if let Some(member) = &quota.member
                && member != &wallet.member
            {
                return Err(UsageError::Parse(
                    "OpenDesign 钱包与额度的工作区成员不一致".to_string(),
                ));
            }
            Ok(combine(quota, Some(wallet.item)))
        }
        (Ok(quota), Err(wallet_error)) => {
            tracing::warn!(error = %wallet_error, "OpenDesign 钱包余额读取失败，仅返回套餐窗口");
            Ok(combine(quota, None))
        }
        (Err(quota_error), Ok(wallet)) => {
            tracing::warn!(error = %quota_error, "OpenDesign 套餐额度读取失败，仅返回钱包余额");
            Ok(FetchOutput::Balance {
                items: vec![wallet.item],
            })
        }
        (Err(quota_error), Err(wallet_error)) => Err(more_actionable(quota_error, wallet_error)),
    }
}

/// 套餐窗口与钱包余额的合并：有生效套餐 → 混合形态（钱包可选）；无套餐 →
/// 钱包余额；无套餐且没钱包 → 固定三槽位（全部不可用）。
fn combine(quota: Preflight, wallet: Option<BalanceItem>) -> FetchOutput {
    // 钱包端点失败时用 preflight 自带的 balanceUsd 兜底。
    let wallet = wallet.or_else(|| quota.balance_usd.map(balance_item));
    let balances: Vec<BalanceItem> = wallet.into_iter().collect();
    if !quota.eligible {
        if balances.is_empty() {
            return FetchOutput::Quota {
                plan: None,
                windows: quota.windows,
            };
        }
        return FetchOutput::Balance { items: balances };
    }
    FetchOutput::QuotaWithBalance {
        plan: None,
        windows: quota.windows,
        balances,
    }
}

fn balance_item(amount: f64) -> BalanceItem {
    BalanceItem {
        label: "钱包余额".to_string(),
        amount,
        currency: Some("USD".to_string()),
        primary: true,
    }
}

fn read_ok(reply: HttpReply) -> Result<String, UsageError> {
    if is_session_invalid_status(reply.status) {
        return Err(UsageError::Auth);
    }
    if reply.status != 200 {
        return Err(UsageError::Upstream(reply.status, snippet(&reply.body)));
    }
    Ok(reply.body)
}

/// 额度响应：窗口可能直接位于顶层，也可能包在 `codingPlan` 内（兼容两种形态）。
fn parse_preflight(body: &str, workspace: &str) -> Result<Preflight, UsageError> {
    let value: Value = serde_json::from_str(body)
        .map_err(|e| UsageError::Parse(format!("OpenDesign 额度响应不是合法 JSON：{e}")))?;
    let object = value
        .as_object()
        .ok_or_else(|| UsageError::Parse("OpenDesign 额度响应不是 JSON 对象".to_string()))?;

    validate_workspace(object, workspace)?;
    let mut member = member_of(object)?;
    let mut merged = object.clone();
    if let Some(plan) = object.get("codingPlan").and_then(Value::as_object) {
        validate_workspace(plan, workspace)?;
        if let Some(inner) = member_of(plan)? {
            if let Some(outer) = &member
                && outer != &inner
            {
                return Err(UsageError::Parse(
                    "OpenDesign 额度响应的外层与套餐成员不一致".to_string(),
                ));
            }
            member = Some(inner);
        }
        for (key, value) in plan {
            merged.insert(key.clone(), value.clone());
        }
    }

    let eligible = merged
        .get("eligible")
        .and_then(Value::as_bool)
        .ok_or_else(|| UsageError::Parse("OpenDesign 额度响应缺少 eligible".to_string()))?;
    let rows = merged
        .get("windows")
        .and_then(Value::as_array)
        .ok_or_else(|| UsageError::Parse("OpenDesign 额度响应缺少 windows 数组".to_string()))?;
    if rows.len() > 32 {
        return Err(UsageError::Parse(
            "OpenDesign 额度窗口数量超出上限".to_string(),
        ));
    }
    if !eligible && !rows.is_empty() {
        return Err(UsageError::Parse(
            "OpenDesign 额度响应标记不可用但仍返回窗口".to_string(),
        ));
    }

    let mut policy_ids = HashSet::new();
    let mut parsed: Vec<(WindowKind, QuotaWindow, String)> = Vec::new();
    for row in rows {
        let row = row
            .as_object()
            .ok_or_else(|| UsageError::Parse("OpenDesign 额度窗口不是对象".to_string()))?;
        let duration = row
            .get("durationSeconds")
            .and_then(Value::as_u64)
            .filter(|duration| *duration > 0)
            .ok_or_else(|| {
                UsageError::Parse("OpenDesign 额度窗口缺少 durationSeconds".to_string())
            })?;
        let Some(kind) = window_kind(duration) else {
            continue;
        };
        let policy_id = row
            .get("policyId")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|id| !id.is_empty())
            .ok_or_else(|| UsageError::Parse("OpenDesign 额度窗口缺少 policyId".to_string()))?;
        if !policy_ids.insert(policy_id.to_string()) {
            return Err(UsageError::Parse(
                "OpenDesign 额度窗口 policyId 重复".to_string(),
            ));
        }
        let used = credits(row, "usedCredits")?;
        let remaining = credits(row, "remainingCredits")?;
        let limit = credits(row, "limitCredits")?;
        if limit == 0 || remaining > limit || remaining != limit.saturating_sub(used) {
            return Err(UsageError::Parse(
                "OpenDesign 额度窗口的已用/剩余/上限不一致".to_string(),
            ));
        }
        let resets_at = match row.get("resetsAt") {
            None | Some(Value::Null) => None,
            Some(value) => Some(reset_ts(value).ok_or_else(|| {
                UsageError::Parse("OpenDesign 额度窗口 resetsAt 非法".to_string())
            })?),
        };
        let remaining_percent = remaining as f64 / limit as f64 * 100.0;
        let window = QuotaWindow::from_remaining_percent(kind, remaining_percent, resets_at);
        parsed.push((kind, window, policy_id.to_string()));
    }

    if eligible && parsed.is_empty() {
        return Err(UsageError::Parse(
            "OpenDesign 套餐标记可用但没有可识别的额度窗口".to_string(),
        ));
    }

    let mut kind_counts: Vec<(WindowKind, usize)> = Vec::new();
    for (kind, _, _) in &parsed {
        if let Some(entry) = kind_counts.iter_mut().find(|(seen, _)| seen == kind) {
            entry.1 += 1;
        } else {
            kind_counts.push((*kind, 1));
        }
    }
    let mut windows = empty_windows();
    for (kind, mut window, policy_id) in parsed {
        // 同类窗口多条时用 policyId 区分，避免前端进度条 key 撞车。
        if kind_counts
            .iter()
            .any(|(seen, count)| *seen == kind && *count > 1)
        {
            window.label = Some(policy_id);
        }
        if matches!(
            kind,
            WindowKind::FiveHour | WindowKind::Weekly | WindowKind::Monthly
        ) && windows
            .iter()
            .any(|slot| slot.window == kind && !slot.available)
        {
            set_window(&mut windows, window);
        } else {
            windows.push(window);
        }
    }

    Ok(Preflight {
        eligible,
        windows,
        balance_usd: merged.get("balanceUsd").and_then(num),
        member,
    })
}

fn window_kind(duration: u64) -> Option<WindowKind> {
    match duration {
        18_000 => Some(WindowKind::FiveHour),
        86_400 => Some(WindowKind::Daily),
        604_800 => Some(WindowKind::Weekly),
        2_592_000 => Some(WindowKind::Monthly),
        _ => None,
    }
}

fn credits(row: &Map<String, Value>, key: &str) -> Result<u128, UsageError> {
    let text = row
        .get(key)
        .and_then(Value::as_str)
        .ok_or_else(|| UsageError::Parse(format!("OpenDesign 额度窗口缺少 {key}")))?;
    if text.is_empty() || !text.bytes().all(|byte| byte.is_ascii_digit()) {
        return Err(UsageError::Parse(format!(
            "OpenDesign 额度窗口 {key} 不是非负整数"
        )));
    }
    text.parse::<u128>()
        .map_err(|_| UsageError::Parse(format!("OpenDesign 额度窗口 {key} 超出可解析范围")))
}

fn validate_workspace(object: &Map<String, Value>, workspace: &str) -> Result<(), UsageError> {
    if let Some(id) = object.get("workspaceId").and_then(Value::as_str)
        && id != workspace
    {
        return Err(UsageError::Parse(
            "OpenDesign 额度响应的工作区与配置不一致".to_string(),
        ));
    }
    Ok(())
}

fn member_of(object: &Map<String, Value>) -> Result<Option<String>, UsageError> {
    match object.get("workspaceMemberId") {
        None | Some(Value::Null) => Ok(None),
        Some(value) => value
            .as_str()
            .filter(|member| valid_identifier(member))
            .map(|member| Some(member.to_string()))
            .ok_or_else(|| UsageError::Parse("OpenDesign 工作区成员 ID 格式无效".to_string())),
    }
}

fn parse_wallet(body: &str, workspace: &str) -> Result<Wallet, UsageError> {
    let value: Value = serde_json::from_str(body)
        .map_err(|e| UsageError::Parse(format!("OpenDesign 钱包响应不是合法 JSON：{e}")))?;
    let object = value
        .as_object()
        .ok_or_else(|| UsageError::Parse("OpenDesign 钱包响应不是 JSON 对象".to_string()))?;
    if object.get("billingScopeVersion").and_then(Value::as_u64) != Some(2) {
        return Err(UsageError::Parse(
            "OpenDesign 钱包响应缺少 billingScopeVersion: 2".to_string(),
        ));
    }
    if object.get("workspaceId").and_then(Value::as_str) != Some(workspace) {
        return Err(UsageError::Parse(
            "OpenDesign 钱包响应的工作区与配置不一致".to_string(),
        ));
    }
    let member = object
        .get("workspaceMemberId")
        .and_then(Value::as_str)
        .filter(|member| valid_identifier(member))
        .ok_or_else(|| UsageError::Parse("OpenDesign 钱包响应缺少工作区成员".to_string()))?;
    let amount = object
        .get("balanceUsd")
        .and_then(num)
        .filter(|amount| amount.is_finite())
        .ok_or_else(|| UsageError::Parse("OpenDesign 钱包响应缺少 balanceUsd".to_string()))?;
    Ok(Wallet {
        item: balance_item(amount),
        member: member.to_string(),
    })
}

fn valid_identifier(value: &str) -> bool {
    let bytes = value.as_bytes();
    !bytes.is_empty()
        && bytes.len() <= 128
        && bytes[0].is_ascii_alphanumeric()
        && bytes
            .iter()
            .all(|byte| byte.is_ascii_alphanumeric() || *byte == b'_' || *byte == b'-')
}

/// 两侧都失败时按可操作性取一个：鉴权 > 响应形态 > HTTP > 网络。
fn more_actionable(a: UsageError, b: UsageError) -> UsageError {
    fn rank(error: &UsageError) -> u8 {
        match error {
            UsageError::Auth => 0,
            UsageError::Parse(_) => 1,
            UsageError::Upstream(..) => 2,
            UsageError::Network(_) => 3,
            _ => 4,
        }
    }
    if rank(&a) <= rank(&b) { a } else { b }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn preflight_body() -> &'static str {
        r#"{
            "workspaceId": "ws-1",
            "workspaceMemberId": "member-1",
            "eligible": true,
            "balanceUsd": "3.50",
            "codingPlan": {
                "workspaceId": "ws-1",
                "workspaceMemberId": "member-1",
                "windows": [
                    {"policyId":"p5h","durationSeconds":18000,"usedCredits":"250","remainingCredits":"750","limitCredits":"1000","resetsAt":"2026-10-25T10:00:00Z"},
                    {"policyId":"pweek","durationSeconds":604800,"usedCredits":"0","remainingCredits":"100","limitCredits":"100","resetsAt":null}
                ]
            }
        }"#
    }

    #[test]
    fn preflight_parses_nested_windows() {
        let parsed = parse_preflight(preflight_body(), "ws-1").unwrap();
        assert!(parsed.eligible);
        assert_eq!(parsed.windows.len(), 3, "固定 5h/周/月三槽位");
        assert_eq!(parsed.windows[0].window, WindowKind::FiveHour);
        assert_eq!(parsed.windows[0].remaining_percent, Some(75.0));
        assert_eq!(parsed.windows[0].used_percent, Some(25.0));
        assert!(parsed.windows[0].resets_at.is_some());
        assert_eq!(parsed.windows[1].window, WindowKind::Weekly);
        assert_eq!(parsed.windows[1].remaining_percent, Some(100.0));
        assert!(parsed.windows[1].resets_at.is_none());
        assert_eq!(parsed.windows[2].window, WindowKind::Monthly);
        assert!(!parsed.windows[2].available);
        assert_eq!(parsed.balance_usd, Some(3.5));
        assert_eq!(parsed.member.as_deref(), Some("member-1"));
    }

    #[test]
    fn preflight_accepts_flat_windows_and_missing_workspace_id() {
        let body = r#"{
            "eligible": true,
            "windows": [
                {"policyId":"p","durationSeconds":2592000,"usedCredits":"1","remainingCredits":"3","limitCredits":"4","resetsAt":"2026-10-25T10:00:00Z"}
            ]
        }"#;
        let parsed = parse_preflight(body, "ws-1").unwrap();
        assert_eq!(parsed.windows.len(), 3);
        assert_eq!(parsed.windows[0].window, WindowKind::FiveHour);
        assert!(!parsed.windows[0].available);
        assert_eq!(parsed.windows[2].window, WindowKind::Monthly);
        assert_eq!(parsed.windows[2].remaining_percent, Some(75.0));
        assert!(parsed.member.is_none());
    }

    #[test]
    fn preflight_no_subscription_keeps_unavailable_slots() {
        let body = r#"{"workspaceId":"ws-1","eligible":false,"windows":[],"balanceUsd":"1.25"}"#;
        let parsed = parse_preflight(body, "ws-1").unwrap();
        assert!(!parsed.eligible);
        assert_eq!(parsed.windows.len(), 3);
        assert!(parsed.windows.iter().all(|window| !window.available));
        assert_eq!(parsed.balance_usd, Some(1.25));
    }

    #[test]
    fn preflight_rejects_identity_and_shape_violations() {
        let cases = [
            r#"{"workspaceId":"other","eligible":true,"windows":[{"policyId":"p","durationSeconds":18000,"usedCredits":"0","remainingCredits":"1","limitCredits":"1"}]}"#,
            r#"{"workspaceId":"ws-1","workspaceMemberId":"m1","eligible":true,"codingPlan":{"workspaceMemberId":"m2"},"windows":[{"policyId":"p","durationSeconds":18000,"usedCredits":"0","remainingCredits":"1","limitCredits":"1"}]}"#,
            r#"{"workspaceId":"ws-1","workspaceMemberId":"bad member","eligible":true,"windows":[{"policyId":"p","durationSeconds":18000,"usedCredits":"0","remainingCredits":"1","limitCredits":"1"}]}"#,
            r#"{"workspaceId":"ws-1","eligible":true,"windows":[]}"#,
            r#"{"workspaceId":"ws-1","eligible":false,"windows":[{"policyId":"p","durationSeconds":18000,"usedCredits":"0","remainingCredits":"1","limitCredits":"1"}]}"#,
            r#"{"workspaceId":"ws-1","eligible":true,"windows":[{"policyId":"p","durationSeconds":18000,"usedCredits":"2","remainingCredits":"1","limitCredits":"1"}]}"#,
            r#"{"workspaceId":"ws-1","eligible":true,"windows":[{"policyId":"p","durationSeconds":18000,"usedCredits":"0","remainingCredits":"1","limitCredits":"0"}]}"#,
            r#"{"workspaceId":"ws-1","eligible":true,"windows":[{"policyId":"p","durationSeconds":18000,"usedCredits":"0","remainingCredits":"1","limitCredits":"1"},{"policyId":"p","durationSeconds":604800,"usedCredits":"0","remainingCredits":"1","limitCredits":"1"}]}"#,
            r#"{"workspaceId":"ws-1","eligible":true,"windows":[{"policyId":"p","durationSeconds":18000,"usedCredits":"0","remainingCredits":"1","limitCredits":"1","resetsAt":"nope"}]}"#,
            r#"{"workspaceId":"ws-1","eligible":true,"windows":[{"policyId":"p","durationSeconds":999,"usedCredits":"0","remainingCredits":"1","limitCredits":"1"}]}"#,
        ];
        for body in cases {
            assert!(parse_preflight(body, "ws-1").is_err(), "应拒绝：{body}");
        }
    }

    #[test]
    fn preflight_duplicate_window_kinds_get_policy_labels() {
        let body = r#"{
            "workspaceId":"ws-1",
            "eligible":true,
            "windows":[
                {"policyId":"a","durationSeconds":18000,"usedCredits":"1","remainingCredits":"1","limitCredits":"2"},
                {"policyId":"b","durationSeconds":18000,"usedCredits":"0","remainingCredits":"1","limitCredits":"1"}
            ]
        }"#;
        let parsed = parse_preflight(body, "ws-1").unwrap();
        assert_eq!(parsed.windows.len(), 4, "三槽位 + 重复 5h 追加一条");
        let labels: Vec<&str> = parsed
            .windows
            .iter()
            .filter_map(|window| window.label.as_deref())
            .collect();
        assert_eq!(labels, vec!["a", "b"]);
    }

    #[test]
    fn wallet_parses_balance_and_identity() {
        let body = r#"{"billingScopeVersion":2,"workspaceId":"ws-1","workspaceMemberId":"member-1","balanceUsd":"12.34"}"#;
        let wallet = parse_wallet(body, "ws-1").unwrap();
        assert_eq!(wallet.member, "member-1");
        assert_eq!(wallet.item.label, "钱包余额");
        assert_eq!(wallet.item.amount, 12.34);
        assert_eq!(wallet.item.currency.as_deref(), Some("USD"));
        assert!(wallet.item.primary);
    }

    #[test]
    fn wallet_rejects_invalid_shapes() {
        let cases = [
            r#"{"billingScopeVersion":1,"workspaceId":"ws-1","workspaceMemberId":"member-1","balanceUsd":"1"}"#,
            r#"{"billingScopeVersion":2,"workspaceId":"other","workspaceMemberId":"member-1","balanceUsd":"1"}"#,
            r#"{"billingScopeVersion":2,"workspaceId":"ws-1","workspaceMemberId":"bad member","balanceUsd":"1"}"#,
            r#"{"billingScopeVersion":2,"workspaceId":"ws-1","workspaceMemberId":"member-1"}"#,
        ];
        for body in cases {
            assert!(parse_wallet(body, "ws-1").is_err(), "应拒绝：{body}");
        }
    }

    #[test]
    fn workspace_identifier_is_path_safe() {
        assert!(valid_identifier("ws-1_abc"));
        assert!(!valid_identifier(""));
        assert!(!valid_identifier("../ws"));
        assert!(!valid_identifier("ws/1"));
        assert!(!valid_identifier(" ws"));
    }
}
