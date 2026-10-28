//! 批量测速的运行控制：单实例、可取消、**无并发上限**。
//!
//! 所有候选模型同时开测，也不做同供应商串行（ADR-0027 补记：原设计有全局
//! `Semaphore(4)` 与「同一供应商串行」两道闸，按用户要求全部取消）。代价要
//! 认清：同一把 API Key 的多个长请求会并发打上游，容易撞到厂商限流，此时
//! 测到的是限流后的速度而非模型真实吐字速度。
//!
//! 单实例的实现落点是 [`SpeedTestSlot`]：`try_begin` 拿到槽位即占位，
//! `SlotGuard` 在任务结束时（含 panic）经 `Drop` 释放，不需要额外的收尾清理。

use std::sync::Arc;
use std::sync::atomic::Ordering;

use sea_orm::DatabaseConnection;

use crate::speedtest::bench::{bench_one, to_result_model};
use crate::speedtest::candidate::{self, Candidate, Filter};
use crate::speedtest::store;
use crate::speedtest::{SPEED_TEST_PROMPT, SlotGuard, target_max_tokens};
use crate::state::AppState;

/// 启动结果：新运行被受理，或已有运行在跑（前端据此不报错、直接展示进度）。
#[derive(Debug, Clone)]
pub enum StartOutcome {
    Started { run_id: i32, total: usize },
    AlreadyRunning,
    NoCandidate,
}

/// 启动一次批量测速。同步完成「占槽位 → 清旧结果 → 建运行与占位行」，然后
/// 把实际执行交给后台任务，接口立即返回（长任务不能挂在 HTTP 请求上）。
pub async fn start(state: &AppState, filter: Filter) -> Result<StartOutcome, String> {
    let candidates = candidate::resolve(&state.db, &filter)
        .await
        .map_err(|e| format!("解析候选模型失败：{e}"))?;

    let Some((flag, guard)) = state.speed_test.try_begin() else {
        return Ok(StartOutcome::AlreadyRunning);
    };
    if candidates.is_empty() {
        // 凭证 drop 即释放槽位，不留下「空跑中」的假状态。
        drop(guard);
        return Ok(StartOutcome::NoCandidate);
    }

    if let Err(e) = store::clear_previous(&state.db).await {
        drop(guard);
        return Err(format!("清理上一次测速结果失败：{e}"));
    }

    // 行的 max_tokens 记本次实际使用的目标值：候选模型登记值不一致（有的 1k
    // 有的 128k），统一按夹取规则落到各行；运行级只记上限常量供展示。
    let run_max_tokens = target_max_tokens(i64::MAX) as i32;
    let run_id = match store::create_run(
        &state.db,
        &candidates,
        &filter.provider_ids,
        &filter.vm_ids,
        &filter.keyword,
        SPEED_TEST_PROMPT,
        run_max_tokens,
    )
    .await
    {
        Ok(id) => id,
        Err(e) => {
            drop(guard);
            return Err(format!("创建测速运行失败：{e}"));
        }
    };

    let total = candidates.len();
    tokio::spawn(run_batch(state.clone(), run_id, candidates, flag, guard));
    Ok(StartOutcome::Started { run_id, total })
}

/// 请求取消当前运行。未开始的模型转 `cancelled`，在跑的允许自然结束。
pub async fn cancel(state: &AppState) -> bool {
    if !state.speed_test.request_cancel() {
        return false;
    }
    // 运行行由后台任务收尾（它才知道在跑的请求何时结束），此处只负责把排队
    // 中尚未开始的行标记掉，让页面立刻看到「已取消」而不是干等。
    if let Ok(Some((run, _))) = store::load_current(&state.db).await
        && run.status == store::RUN_RUNNING
    {
        let _ = store::cancel_pending(&state.db, run.id).await;
    }
    true
}

/// 后台执行体：按供应商分组，组内串行、跨组受全局信号量约束。
async fn run_batch(
    state: AppState,
    run_id: i32,
    candidates: Vec<Candidate>,
    flag: Arc<std::sync::atomic::AtomicBool>,
    guard: SlotGuard,
) {
    // 无并发上限：所有候选模型同时开测，也不做同供应商串行。代价是同一把
    // API Key 的多个长请求会并发打上游，容易撞到厂商限流——此时测出来的是
    // 限流后的速度，而非模型的真实吐字速度（ADR-0027 补记）。
    let mut tasks = Vec::with_capacity(candidates.len());
    for candidate in candidates {
        let state = state.clone();
        let flag = flag.clone();
        tasks.push(tokio::spawn(async move {
            if flag.load(Ordering::SeqCst) {
                return;
            }
            run_one(&state, run_id, &candidate).await;
        }));
    }
    for task in tasks {
        let _ = task.await;
    }

    // 收尾：取消过就按 cancelled，否则 completed。剩余的 pending 行只在取消
    // 场景存在（正常跑完每行都已终态化）。
    let cancelled = flag.load(Ordering::SeqCst);
    if cancelled {
        let _ = store::cancel_pending(&state.db, run_id).await;
    }
    let status = if cancelled {
        store::RUN_CANCELLED
    } else {
        store::RUN_COMPLETED
    };
    if let Err(e) = store::finish_run(&state.db, run_id, status).await {
        tracing::warn!(run_id, "测速运行收尾失败：{e}");
    }
    drop(guard);
}

/// 单个模型：测速 → 结果行覆盖占位行。逻辑失败（上游报错）不让整条链中断。
async fn run_one(state: &AppState, run_id: i32, candidate: &Candidate) {
    let outcome = bench_one(state, &candidate.provider, &candidate.model).await;
    if let Some(reason) = &outcome.failure {
        tracing::warn!(
            provider_id = candidate.provider.id,
            provider_name = %candidate.provider.name,
            model_id = %candidate.model.provider_model_id,
            "模型测速失败：{reason}"
        );
    }
    let row = to_result_model(run_id, &candidate.provider, &candidate.model, &outcome);
    if let Err(e) = store::save_row(&state.db, run_id, candidate.model.model_id, row).await {
        tracing::warn!(
            run_id,
            model_id = %candidate.model.provider_model_id,
            "测速结果落库失败：{e}"
        );
    }
}

/// 启动恢复：把残留的 `running` 运行改判 `interrupted`。
pub async fn recover_on_startup(db: &DatabaseConnection) {
    match store::mark_interrupted(db).await {
        Ok(true) => tracing::info!("上次测速运行被服务重启打断，已标记为 interrupted"),
        Ok(false) => {}
        Err(e) => tracing::warn!("测速运行启动恢复失败：{e}"),
    }
}
