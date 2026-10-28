//! 模型测速路由（`/api/speed-test`，ADR-0027）。
//!
//! 四个端点：候选预览、读当前/上次运行、启动运行、取消运行。筛选条件的唯一
//! 事实源是 `speedtest::candidate::resolve`，预览与启动都调它，因此按钮上的
//! 「将测 N 个模型」与实际会跑的模型不会走歧。

use axum::Json;
use axum::Router;
use axum::extract::{Query, State};
use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::routing::{get, post};
use serde::{Deserialize, Serialize};
use serde_json::json;

use crate::response::{self, Response};
use crate::speedtest::{self, Filter, StartOutcome};
use crate::state::AppState;

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/candidates", get(preview_candidates))
        .route("/run", get(get_run).post(start_run))
        .route("/run/cancel", post(cancel_run))
}

/// 过滤条件（query 与 body 共用同一形状）。id 列表两种形态都接受：
/// query 用逗号分隔字符串（与 `/api/request-logs` 多值过滤同一约定），
/// body 用 JSON 数组（前端直接传过滤状态）。只支持一种会让调用方踩到
/// 「同一个字段在 query 与 body 里格式不同」的隐性契约。
#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
struct FilterInput {
    #[serde(default)]
    provider_ids: Option<IdList>,
    #[serde(default)]
    vm_ids: Option<IdList>,
    #[serde(default)]
    keyword: Option<String>,
}

/// id 列表入参：`"1,2"` 或 `[1, 2]` 均可。
#[derive(Debug, Deserialize)]
#[serde(untagged)]
enum IdList {
    Comma(String),
    List(Vec<i32>),
}

impl IdList {
    fn into_ids(self) -> Result<Vec<i32>, String> {
        match self {
            IdList::List(ids) => Ok(ids),
            IdList::Comma(raw) => parse_csv_i32(Some(&raw)),
        }
    }
}

impl FilterInput {
    fn into_filter(self) -> Result<Filter, String> {
        Ok(Filter {
            provider_ids: self
                .provider_ids
                .map(IdList::into_ids)
                .transpose()?
                .unwrap_or_default(),
            vm_ids: self
                .vm_ids
                .map(IdList::into_ids)
                .transpose()?
                .unwrap_or_default(),
            keyword: self.keyword.unwrap_or_default().trim().to_string(),
        })
    }
}

/// 逗号分隔 id 解析：非法分段报错而非静默丢弃（静默丢弃会让筛选退化成
/// 「不过滤」，用户拿到看似成功却范围错误的结果）。
fn parse_csv_i32(raw: Option<&str>) -> Result<Vec<i32>, String> {
    let Some(raw) = raw else {
        return Ok(Vec::new());
    };
    raw.split(',')
        .map(str::trim)
        .filter(|part| !part.is_empty())
        .map(|part| {
            part.parse::<i32>()
                .map_err(|_| format!("非法 id 参数：{part}"))
        })
        .collect()
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct ProviderPreview {
    provider_id: i32,
    provider_name: String,
    count: usize,
}

/// 候选模型行：筛选一变化就展示的对象（不必等测速）。
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct CandidateView {
    provider_id: i32,
    provider_name: String,
    provider_model_pk: i32,
    model_id: String,
    /// 引用该模型的虚拟模型 display_id（已排序去重），便于解释过滤命中原因。
    virtual_models: Vec<String>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct CandidatePreview {
    total: usize,
    providers: Vec<ProviderPreview>,
    /// 完整候选清单：页面在「筛选已就绪但还没跑过」时直接渲染这些行，
    /// 而不是让用户先点测速才知道会测谁。
    models: Vec<CandidateView>,
}

async fn preview_candidates(
    State(state): State<AppState>,
    Query(input): Query<FilterInput>,
) -> impl IntoResponse {
    let filter = match input.into_filter() {
        Ok(filter) => filter,
        Err(msg) => return response::bad_request::<CandidatePreview>(msg),
    };
    let candidates = match speedtest::resolve(&state.db, &filter).await {
        Ok(list) => list,
        Err(e) => return response::db_error(format!("解析候选模型失败：{e}")),
    };

    // 按供应商归并计数，保持 resolve 的供应商顺序（sort_order, id）。
    let mut providers: Vec<ProviderPreview> = Vec::new();
    for candidate in &candidates {
        match providers.last_mut() {
            Some(last) if last.provider_id == candidate.provider.id => last.count += 1,
            _ => providers.push(ProviderPreview {
                provider_id: candidate.provider.id,
                provider_name: candidate.provider.name.clone(),
                count: 1,
            }),
        }
    }

    let models: Vec<CandidateView> = candidates
        .iter()
        .map(|candidate| CandidateView {
            provider_id: candidate.provider.id,
            provider_name: candidate.provider.name.clone(),
            provider_model_pk: candidate.model.model_id,
            model_id: candidate.model.provider_model_id.clone(),
            virtual_models: candidate.vm_display_ids.clone(),
        })
        .collect();

    (
        StatusCode::OK,
        Json(Response::success(CandidatePreview {
            total: candidates.len(),
            providers,
            models,
        })),
    )
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct RunView {
    id: i32,
    status: String,
    started_at: i64,
    finished_at: Option<i64>,
    total: i32,
    /// 运行中标志（内存槽位真相，与库中 status 互为校验：进程刚重启时库里可能
    /// 还是 running，但内存槽位已空，此时前端应视为「没有在跑」）。
    running: bool,
    /// 本次运行使用的筛选条件快照：前端据此判断「当前结果是否属于当前筛选」——
    /// 用户改了筛选就该看点候选清单，而不是上一批与筛选无关的成绩。
    provider_ids: Vec<i32>,
    vm_ids: Vec<i32>,
    keyword: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct ResultView {
    provider_id: i32,
    provider_name: String,
    provider_model_pk: i32,
    model_id: String,
    status: String,
    ttft_ms: Option<i64>,
    output_tokens: Option<i64>,
    output_tokens_time_ms: Option<i64>,
    total_ms: Option<i64>,
    tps: Option<f64>,
    request_id: Option<String>,
    fail_reason: Option<String>,
    finished_at: Option<i64>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct RunResponse {
    run: Option<RunView>,
    results: Vec<ResultView>,
}

/// 解析运行行里的筛选快照（JSON 字符串数组；损坏时退回空 = 不过滤，与写入
/// 侧 `serde_json::to_string(...).unwrap_or_else(|_| "[]")` 的容错口径一致）。
fn parse_snapshot_ids(raw: &str) -> Vec<i32> {
    serde_json::from_str::<Vec<i32>>(raw).unwrap_or_default()
}

fn build_run_response(
    running: bool,
    loaded: Option<(
        crate::entity::speed_test_run::Model,
        Vec<crate::entity::speed_test_result::Model>,
    )>,
) -> RunResponse {
    let Some((run, rows)) = loaded else {
        return RunResponse {
            run: None,
            results: Vec::new(),
        };
    };
    RunResponse {
        run: Some(RunView {
            id: run.id,
            status: run.status,
            started_at: run.started_at,
            finished_at: run.finished_at,
            total: run.total,
            running,
            provider_ids: parse_snapshot_ids(&run.provider_ids),
            vm_ids: parse_snapshot_ids(&run.vm_ids),
            keyword: run.keyword,
        }),
        results: rows
            .into_iter()
            .map(|row| ResultView {
                provider_id: row.provider_id,
                provider_name: row.provider_name,
                provider_model_pk: row.provider_model_pk,
                model_id: row.model_id,
                status: row.status,
                ttft_ms: row.ttft_ms,
                output_tokens: row.output_tokens,
                output_tokens_time_ms: row.output_tokens_time_ms,
                total_ms: row.total_ms,
                tps: row.tps,
                request_id: row.request_id,
                fail_reason: row.fail_reason,
                finished_at: row.finished_at,
            })
            .collect(),
    }
}

async fn get_run(State(state): State<AppState>) -> impl IntoResponse {
    match speedtest::store::load_current(&state.db).await {
        Ok(loaded) => (
            StatusCode::OK,
            Json(Response::success(build_run_response(
                state.speed_test.is_running(),
                loaded,
            ))),
        ),
        Err(e) => response::db_error(format!("读取测速结果失败：{e}")),
    }
}

async fn start_run(
    State(state): State<AppState>,
    Json(input): Json<FilterInput>,
) -> impl IntoResponse {
    let filter = match input.into_filter() {
        Ok(filter) => filter,
        Err(msg) => return response::bad_request::<serde_json::Value>(msg),
    };

    let outcome = match speedtest::start(&state, filter).await {
        Ok(outcome) => outcome,
        Err(e) => return response::internal_error::<serde_json::Value>(e),
    };

    let body = match outcome {
        StartOutcome::Started { run_id, total } => {
            json!({"alreadyRunning": false, "runId": run_id, "total": total})
        }
        // 已有运行在跑：不报错，回当前进度让前端直接展示——用户连点两下不该
        // 看到红色错误提示。
        StartOutcome::AlreadyRunning => json!({"alreadyRunning": true}),
        StartOutcome::NoCandidate => {
            return response::bad_request::<serde_json::Value>(
                "当前筛选条件下没有可测速的模型（只测已启用的供应商）",
            );
        }
    };
    (StatusCode::OK, Json(Response::success(body)))
}

async fn cancel_run(State(state): State<AppState>) -> impl IntoResponse {
    let cancelled = speedtest::cancel(&state).await;
    (
        StatusCode::OK,
        Json(Response::success(json!({"cancelled": cancelled}))),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn csv_ids_parse_and_reject_invalid() {
        assert_eq!(parse_csv_i32(None).unwrap(), Vec::<i32>::new());
        assert_eq!(parse_csv_i32(Some("")).unwrap(), Vec::<i32>::new());
        assert_eq!(parse_csv_i32(Some("1, 2 ,3")).unwrap(), vec![1, 2, 3]);
        assert!(parse_csv_i32(Some("1,abc")).is_err());
    }

    #[test]
    fn filter_input_accepts_csv_and_array_forms() {
        let csv = FilterInput {
            provider_ids: Some(IdList::Comma("1,2".into())),
            vm_ids: None,
            keyword: Some("  claude  ".into()),
        }
        .into_filter()
        .unwrap();
        assert_eq!(csv.provider_ids, vec![1, 2]);
        assert!(csv.vm_ids.is_empty());
        assert_eq!(csv.keyword, "claude");

        let list = FilterInput {
            provider_ids: Some(IdList::List(vec![3, 4])),
            vm_ids: Some(IdList::List(vec![7])),
            keyword: None,
        }
        .into_filter()
        .unwrap();
        assert_eq!(list.provider_ids, vec![3, 4]);
        assert_eq!(list.vm_ids, vec![7]);
        assert!(list.keyword.is_empty());
    }

    #[test]
    fn array_form_rejects_malformed_csv() {
        let bad = FilterInput {
            provider_ids: Some(IdList::Comma("1,abc".into())),
            ..Default::default()
        };
        assert!(bad.into_filter().is_err());
    }
}
