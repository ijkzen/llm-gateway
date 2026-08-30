use axum::{
    Json, Router,
    extract::{Path, State},
    http::StatusCode,
    response::IntoResponse,
    response::sse::{Event as SseEvent, KeepAlive, Sse},
    routing::{delete, get, post, put},
};
use serde::Deserialize;
use tokio_stream::{StreamExt, wrappers::BroadcastStream};

use crate::cron::JobInfo;
use crate::cron::SchedulerError;
use crate::cron::log_repository::{
    CronJobLogRepository, LogRecord, MAX_RUNS_KEPT, RunRecord, SeaOrmCronJobLogRepository,
};
use crate::cron::repository::{CronJobRepository, JobDefinition, SeaOrmCronJobRepository};
use crate::response::{self, Response};
use crate::state::AppState;

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/", get(list_jobs))
        .route("/{name}", put(update_job))
        .route("/{name}/run", post(run_job))
        .route("/{name}", delete(delete_job))
        .route("/{name}/logs", get(list_job_logs))
        .route("/{name}/logs/stream", get(stream_job_logs))
        .route("/{name}/logs/{run_id}", get(list_run_logs))
}

#[derive(Deserialize)]
struct UpdateJobRequest {
    title: Option<String>,
    description: Option<String>,
    expression: Option<String>,
    enabled: Option<bool>,
    group: Option<String>,
}

#[derive(serde::Serialize)]
struct JobResponse {
    name: String,
    title: String,
    description: String,
    expression: String,
    enabled: bool,
    group: String,
    last_run_at: String,
    next_run_at: String,
    updated_at: String,
    frequency_secs: i64,
}

impl From<JobInfo> for JobResponse {
    fn from(info: JobInfo) -> Self {
        Self {
            name: info.name,
            title: info.title,
            description: info.description,
            expression: info.expression,
            enabled: info.enabled,
            group: info.group,
            last_run_at: info.last_run_at.map(|t| t.to_rfc3339()).unwrap_or_default(),
            next_run_at: info.next_run_at.map(|t| t.to_rfc3339()).unwrap_or_default(),
            updated_at: info.updated_at.map(|t| t.to_rfc3339()).unwrap_or_default(),
            frequency_secs: info.frequency_secs,
        }
    }
}

fn scheduler_error_status(e: &SchedulerError) -> StatusCode {
    match e {
        SchedulerError::ParseError(_) | SchedulerError::ComputeNextRun(_) => {
            StatusCode::BAD_REQUEST
        }
        SchedulerError::JobNotFound(_) => StatusCode::NOT_FOUND,
        _ => StatusCode::INTERNAL_SERVER_ERROR,
    }
}

async fn list_jobs(State(state): State<AppState>) -> impl IntoResponse {
    let repo = SeaOrmCronJobRepository::new(state.db.clone());
    match state.scheduler.list_jobs_detailed(&repo).await {
        Ok(jobs) => {
            let response: Vec<JobResponse> = jobs.into_iter().map(Into::into).collect();
            (StatusCode::OK, Json(Response::success(response)))
        }
        Err(e) => response::scheduler_error(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()),
    }
}

async fn update_job(
    State(state): State<AppState>,
    Path(name): Path<String>,
    Json(req): Json<UpdateJobRequest>,
) -> impl IntoResponse {
    if let Some(ref title) = req.title
        && title.is_empty()
    {
        return response::bad_request("标题不能为空");
    }

    if let Some(ref expression) = req.expression
        && expression.is_empty()
    {
        return response::bad_request("表达式不能为空");
    }

    let _guard = state.scheduler.modification_lock().await;

    let repo = SeaOrmCronJobRepository::new(state.db.clone());

    let model = match repo.find_by_name(&name).await {
        Ok(Some(model)) => model,
        Ok(None) => {
            return response::not_found(format!("任务 '{name}' 不存在"));
        }
        Err(e) => {
            return response::db_error(e.to_string());
        }
    };

    // The job exists in the DB but may have been skipped at load time (no
    // handler registered). Reject before touching the DB so a failed update
    // never leaves the database and the scheduler out of sync.
    if !state.scheduler.has_job(&name).await {
        return response::bad_request(format!(
            "任务 '{name}' 未加载到调度器中（未注册对应的 Handler）"
        ));
    }

    let new_expression = req.expression.unwrap_or(model.expression);

    let next_run_at = match crate::cron::parser::compute_next_run(&new_expression) {
        Ok(next) => next,
        Err(e) => {
            return response::bad_request(format!("表达式无效：{e}"));
        }
    };

    let definition = JobDefinition {
        name: name.clone(),
        title: req.title.unwrap_or(model.title),
        description: req.description.unwrap_or(model.description),
        expression: new_expression,
        enabled: req.enabled.unwrap_or(model.enabled),
        group: req.group.unwrap_or(model.group),
    };

    match repo
        .update_job_full(&name, &definition, model.last_run_at, next_run_at)
        .await
    {
        Ok(Some(_)) => {}
        Ok(None) => {
            return response::not_found(format!("任务 '{name}' 不存在"));
        }
        Err(e) => {
            return response::db_error(e.to_string());
        }
    }

    if let Err(e) = state
        .scheduler
        .update_job_in_memory(&name, &definition)
        .await
    {
        return response::scheduler_error(scheduler_error_status(&e), e.to_string());
    }

    (StatusCode::OK, Json(Response::success(())))
}

async fn run_job(State(state): State<AppState>, Path(name): Path<String>) -> impl IntoResponse {
    match state.scheduler.run_job_now(&name).await {
        Ok(_) => (StatusCode::OK, Json(Response::success(()))),
        Err(e) => response::scheduler_error(scheduler_error_status(&e), e.to_string()),
    }
}

async fn delete_job(State(state): State<AppState>, Path(name): Path<String>) -> impl IntoResponse {
    let _guard = state.scheduler.modification_lock().await;
    let repo = SeaOrmCronJobRepository::new(state.db.clone());

    match repo.find_by_name(&name).await {
        Ok(Some(_)) => match state.scheduler.soft_delete_job(&repo, &name).await {
            Ok(_) => (StatusCode::OK, Json(Response::success(()))),
            Err(e) => response::scheduler_error(scheduler_error_status(&e), e.to_string()),
        },
        Ok(None) => response::not_found(format!("任务 '{name}' 不存在")),
        Err(e) => response::db_error(e.to_string()),
    }
}

#[derive(serde::Serialize)]
struct RunResponse {
    run_id: String,
    status: String,
    started_at: String,
    ended_at: String,
    log_count: i32,
    truncated: bool,
}

impl From<RunRecord> for RunResponse {
    fn from(record: RunRecord) -> Self {
        Self {
            run_id: record.run_id,
            status: record.status,
            started_at: record.started_at.to_rfc3339(),
            ended_at: record.ended_at.map(|t| t.to_rfc3339()).unwrap_or_default(),
            log_count: record.log_count,
            truncated: record.truncated,
        }
    }
}

#[derive(serde::Serialize)]
struct LogResponse {
    seq: i32,
    level: String,
    message: String,
    ts: String,
}

impl From<LogRecord> for LogResponse {
    fn from(record: LogRecord) -> Self {
        Self {
            seq: record.seq,
            level: record.level,
            message: record.message,
            ts: record.ts.to_rfc3339(),
        }
    }
}

/// 最近 30 次执行的列表（最新在前）。
async fn list_job_logs(
    State(state): State<AppState>,
    Path(name): Path<String>,
) -> impl IntoResponse {
    let log_repo = SeaOrmCronJobLogRepository::new(state.db.clone());
    match log_repo.list_runs(&name, MAX_RUNS_KEPT).await {
        Ok(runs) => {
            let response: Vec<RunResponse> = runs.into_iter().map(Into::into).collect();
            (StatusCode::OK, Json(Response::success(response)))
        }
        Err(e) => response::db_error(e.to_string()),
    }
}

/// 某次执行的日志（按 seq 升序）。
async fn list_run_logs(
    State(state): State<AppState>,
    Path((name, run_id)): Path<(String, String)>,
) -> impl IntoResponse {
    let log_repo = SeaOrmCronJobLogRepository::new(state.db.clone());

    // 只允许访问该任务最近 MAX_RUNS_KEPT 次执行中的记录。
    let runs = match log_repo.list_runs(&name, MAX_RUNS_KEPT).await {
        Ok(runs) => runs,
        Err(e) => return response::db_error(e.to_string()),
    };
    if !runs.iter().any(|run| run.run_id == run_id) {
        return response::not_found(format!("任务 '{name}' 不存在执行记录 '{run_id}'"));
    }

    match log_repo.list_logs(&run_id).await {
        Ok(logs) => {
            let response: Vec<LogResponse> = logs.into_iter().map(Into::into).collect();
            (StatusCode::OK, Json(Response::success(response)))
        }
        Err(e) => response::db_error(e.to_string()),
    }
}

/// 任务日志实时流（SSE）。
///
/// 打开连接时若任务正在执行，先发送 `snapshot`（回放该次执行已落库的日志），
/// 否则发送 `idle`；之后持续推送 `log` / `run_started` / `run_ended` 事件。
/// 事件数据为 [`crate::cron::log_capture::JobLogEvent`] 的 JSON，前端按 `seq` 去重。
async fn stream_job_logs(
    State(state): State<AppState>,
    Path(name): Path<String>,
) -> impl IntoResponse {
    let log_repo = SeaOrmCronJobLogRepository::new(state.db.clone());

    let initial = match log_repo.list_runs(&name, 1).await {
        Ok(runs) => match runs.into_iter().next() {
            Some(run) if run.status == "running" => {
                let logs = log_repo.list_logs(&run.run_id).await.unwrap_or_default();
                let snapshot = serde_json::json!({
                    "run_id": run.run_id,
                    "started_at": run.started_at.to_rfc3339(),
                    "logs": logs.into_iter().map(|log| serde_json::json!({
                        "seq": log.seq,
                        "level": log.level,
                        "message": log.message,
                        "ts": log.ts.to_rfc3339(),
                    })).collect::<Vec<_>>(),
                });
                SseEvent::default()
                    .event("snapshot")
                    .data(snapshot.to_string())
            }
            _ => SseEvent::default().event("idle").data("{}"),
        },
        Err(_) => SseEvent::default().event("idle").data("{}"),
    };

    let rx = state.log_tx.subscribe();
    let updates = BroadcastStream::new(rx).filter_map(move |result| {
        let event = match result {
            Ok(event) if event.job_name == name => {
                let kind = event.kind.clone();
                let data = serde_json::to_string(&event).unwrap_or_else(|_| "{}".to_string());
                Some(SseEvent::default().event(kind).data(data))
            }
            Ok(_) => None,
            // 接收端积压（Lagged）或通道关闭：通知前端重置本地日志（重拉全量）。
            Err(_) => Some(SseEvent::default().event("reset").data("{}")),
        };
        event.map(Ok::<_, std::convert::Infallible>)
    });

    let stream = tokio_stream::once(Ok::<_, std::convert::Infallible>(initial)).chain(updates);

    Sse::new(stream).keep_alive(KeepAlive::new().interval(std::time::Duration::from_secs(15)))
}
