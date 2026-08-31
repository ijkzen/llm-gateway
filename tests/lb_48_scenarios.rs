//! 虚拟模型成员排序（负载均衡）在「订阅/按量充足与否」组合下的 48 个场景测试。
//!
//! 覆盖规则（与 `src/proxy/mod.rs::order_members` 的生产逻辑一致）：
//! - 策略 0（订阅优先）/ 策略 1（按量优先）。
//! - 订阅成员 A/B/C：充足 = 有可用窗口数据且剩余 > 0；不足 = 任一窗口剩余为 0 → 剔除。
//! - 按量成员 D/E：充足 = 查得到余额且合计 > 0；不足（余额 0）= 查得到余额但合计为 0 → 剔除；
//!   查不到余额（无数据）→ 不剔除。
//! - 排序假设：充足订阅剩余 A>B>C；按量余额 D>E（D 比 E 高）。全平则随机，此处按该假设给确定顺序。
//! - 期望 = LB 排序结果（即失败降级时的尝试顺序）。
//!
//! 注：测试用 (id, billing_mode) 元组代表成员（`Member` 为 crate 私有），
//! 分组/排序/剔除逻辑与生产 `order_members` 完全一致。

use std::collections::HashMap;

use llm_gateway::proxy::usage_rank;
use llm_gateway::usage::types::{BalanceItem, QuotaWindow, UsageData, UsageKind, WindowKind};

const BILLING_SUB: i32 = 1;
const BILLING_PAYG: i32 = 0;

// ---- 用量数据构造 ----

fn quota_usage(
    provider_id: i32,
    five_hour: Option<f64>,
    weekly: Option<f64>,
    monthly: Option<f64>,
) -> UsageData {
    let mut windows = Vec::new();
    for (kind, val) in [
        (WindowKind::FiveHour, five_hour),
        (WindowKind::Weekly, weekly),
        (WindowKind::Monthly, monthly),
    ] {
        windows.push(match val {
            Some(p) => QuotaWindow::from_remaining_percent(kind, p, None),
            None => QuotaWindow::unavailable(kind),
        });
    }
    UsageData {
        provider_id,
        fetched_at: chrono::Utc::now(),
        kind: UsageKind::Quota,
        plan: None,
        windows,
        balances: vec![],
    }
}

fn balance_usage(provider_id: i32, amount: Option<f64>) -> UsageData {
    let balances = match amount {
        Some(a) => vec![BalanceItem {
            label: "余额".to_string(),
            amount: a,
            currency: None,
        }],
        None => vec![],
    };
    UsageData {
        provider_id,
        fetched_at: chrono::Utc::now(),
        kind: UsageKind::Balance,
        plan: None,
        windows: vec![],
        balances,
    }
}

fn quota_ok(p: i32) -> UsageData {
    quota_usage(p, Some(80.0), Some(70.0), Some(60.0))
}
fn quota_exhausted(p: i32) -> UsageData {
    quota_usage(p, Some(0.0), Some(50.0), Some(50.0))
}
fn balance_ok(p: i32) -> UsageData {
    balance_usage(p, Some(100.0))
}
fn balance_zero(p: i32) -> UsageData {
    balance_usage(p, Some(0.0))
}
fn balance_unknown(p: i32) -> UsageData {
    balance_usage(p, None)
}

/// 按量成员状态。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum BalanceState {
    /// 查得到余额且 > 0（充足）。
    Ok,
    /// 查得到余额但 = 0（耗尽 → 剔除）。
    Zero,
    /// 查不到余额（不剔除）。
    Unknown,
}

/// 组装 5 个成员的用量数据（A=1,B=2,C=3 订阅；D=4,E=5 按量）。
fn usages_for(a: bool, b: bool, c: bool, d: BalanceState, e: BalanceState) -> Vec<UsageData> {
    vec![
        if a { quota_ok(1) } else { quota_exhausted(1) },
        if b { quota_ok(2) } else { quota_exhausted(2) },
        if c { quota_ok(3) } else { quota_exhausted(3) },
        match d {
            BalanceState::Ok => balance_ok(4),
            BalanceState::Zero => balance_zero(4),
            BalanceState::Unknown => balance_unknown(4),
        },
        match e {
            BalanceState::Ok => balance_ok(5),
            BalanceState::Zero => balance_zero(5),
            BalanceState::Unknown => balance_unknown(5),
        },
    ]
}

fn usage_map(usages: Vec<UsageData>) -> HashMap<i32, Option<UsageData>> {
    usages
        .into_iter()
        .map(|u| (u.provider_id, Some(u)))
        .collect()
}

/// 模拟 `order_members` 策略 0/1 的候选结果（生产逻辑复刻）：
/// 分订阅/按量组 → 组内排序 → 订阅剔除 → 按量剔除 → 按策略拼接。
fn order(
    members: &[(i32, i32)],
    strategy: i32,
    usage: &HashMap<i32, Option<UsageData>>,
) -> Vec<i32> {
    let subscription_first = strategy == 0;
    let mut subs: Vec<i32> = members
        .iter()
        .filter(|(_, b)| *b == BILLING_SUB)
        .map(|(id, _)| *id)
        .collect();
    let mut payg: Vec<i32> = members
        .iter()
        .filter(|(_, b)| *b == BILLING_PAYG)
        .map(|(id, _)| *id)
        .collect();

    subs.sort_by(|x, y| {
        usage_rank::cmp_quota_remaining(
            usage.get(y).and_then(Option::as_ref),
            usage.get(x).and_then(Option::as_ref),
        )
    });
    payg.sort_by(|x, y| {
        usage_rank::cmp_balance(
            usage.get(y).and_then(Option::as_ref),
            usage.get(x).and_then(Option::as_ref),
        )
    });

    subs.retain(|id| {
        !matches!(
            usage
                .get(id)
                .and_then(Option::as_ref)
                .and_then(UsageData::subscription_usable),
            Some(false)
        )
    });
    payg.retain(|id| {
        !matches!(
            usage
                .get(id)
                .and_then(Option::as_ref)
                .and_then(UsageData::balance_usable),
            Some(false)
        )
    });

    if subscription_first {
        subs.append(&mut payg);
        subs
    } else {
        payg.append(&mut subs);
        payg
    }
}

fn name_of(id: i32) -> &'static str {
    match id {
        1 => "A",
        2 => "B",
        3 => "C",
        4 => "D",
        5 => "E",
        _ => "?",
    }
}

/// 期望链 = 订阅充足成员（按剩余降序 A>B>C）+ 按量充足成员（按余额降序 D>E）
/// + 查不到余额的按量成员；余额 0 与订阅不足的成员一律不出现。
fn chain_for(
    strategy: i32,
    a: bool,
    b: bool,
    c: bool,
    d: BalanceState,
    e: BalanceState,
) -> Vec<&'static str> {
    let mut subs_ok: Vec<&str> = Vec::new();
    if a {
        subs_ok.push("A");
    }
    if b {
        subs_ok.push("B");
    }
    if c {
        subs_ok.push("C");
    }
    // 充足订阅剩余 80/70/60 → A>B>C，push 顺序即降序。
    let mut payg_ok: Vec<&str> = Vec::new();
    let mut payg_unknown: Vec<&str> = Vec::new();
    for (state, name) in [(d, "D"), (e, "E")] {
        match state {
            BalanceState::Ok => payg_ok.push(name),
            BalanceState::Unknown => payg_unknown.push(name),
            BalanceState::Zero => {}
        }
    }
    let mut payg = payg_ok;
    payg.extend(payg_unknown);
    if strategy == 0 {
        let mut chain = subs_ok;
        chain.extend(payg);
        chain
    } else {
        let mut chain = payg;
        chain.extend(subs_ok);
        chain
    }
}

/// 全量 48 场景表驱动测试。
/// 场景编号：策略 0 = 1..24；策略 1 = 25..48。
/// 订阅 8 态（A/B/C 足否）× 按量 3 类（两足 / D 足 E 零 / D 零 E 足）= 24 场景/策略。
#[test]
fn all_48_scenarios() {
    let members = vec![
        (1, BILLING_SUB),
        (2, BILLING_SUB),
        (3, BILLING_SUB),
        (4, BILLING_PAYG),
        (5, BILLING_PAYG),
    ];
    let mut n = 0;
    for strategy in [0, 1] {
        for a in [true, false] {
            for b in [true, false] {
                for c in [true, false] {
                    for (d, e) in [
                        (BalanceState::Ok, BalanceState::Ok),
                        (BalanceState::Ok, BalanceState::Zero),
                        (BalanceState::Zero, BalanceState::Ok),
                    ] {
                        n += 1;
                        let expect = chain_for(strategy, a, b, c, d, e);
                        let usage = usage_map(usages_for(a, b, c, d, e));
                        let got: Vec<&str> = order(&members, strategy, &usage)
                            .iter()
                            .map(|id| name_of(*id))
                            .collect();
                        assert_eq!(
                            got, expect,
                            "场景 {n}（策略 {strategy}，A足={a} B足={b} C足={c}，D={d:?} E={e:?}）"
                        );
                    }
                }
            }
        }
    }
    assert_eq!(n, 48, "应恰好 48 个场景");
}

/// 补充断言：按量「查不到余额」的成员不剔除，排在充足成员之后。
#[test]
fn unknown_balance_member_is_kept_after_sufficient() {
    let members = vec![
        (1, BILLING_SUB),
        (2, BILLING_SUB),
        (3, BILLING_SUB),
        (4, BILLING_PAYG),
        (5, BILLING_PAYG),
    ];
    // 策略 0：订阅 A、B、C 全部充足 → 按量组 E（充足余额 100）→ D（查不到余额）。
    let usage = usage_map(usages_for(
        true,
        true,
        true,
        BalanceState::Unknown,
        BalanceState::Ok,
    ));
    let got: Vec<&str> = order(&members, 0, &usage)
        .iter()
        .map(|id| name_of(*id))
        .collect();
    assert_eq!(got, vec!["A", "B", "C", "E", "D"]);

    // 策略 1：按量组 E（充足）→ D（查不到余额）→ 订阅 A、B、C。
    let got1: Vec<&str> = order(&members, 1, &usage)
        .iter()
        .map(|id| name_of(*id))
        .collect();
    assert_eq!(got1, vec!["E", "D", "A", "B", "C"]);
}

/// 补充断言：余额为 0 的按量成员被剔除（即使它可能余额排序靠前）。
#[test]
fn zero_balance_member_is_removed() {
    let members = vec![
        (1, BILLING_SUB),
        (2, BILLING_SUB),
        (3, BILLING_SUB),
        (4, BILLING_PAYG),
        (5, BILLING_PAYG),
    ];
    // 策略 1：D 余额 0（剔除）、E 余额 100 → 按量组只剩 E → 订阅 A、B、C。
    let usage = usage_map(usages_for(
        true,
        true,
        true,
        BalanceState::Zero,
        BalanceState::Ok,
    ));
    let got: Vec<&str> = order(&members, 1, &usage)
        .iter()
        .map(|id| name_of(*id))
        .collect();
    assert_eq!(got, vec!["E", "A", "B", "C"]);
}
