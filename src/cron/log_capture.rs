//! 定时任务日志捕获层。
//!
//! worker 执行 handler 时会创建一个带 `job_name` / `run_id` 字段的 span，
//! 本模块的 [`JobLogLayer`] 从全局 tracing 事件流中捕获该 span 内的日志事件，
//! 通过 std 同步通道转发给 lib.rs 的桥接任务，再进入 tokio broadcast 供
//! worker（落库）与 SSE（实时推送）订阅。span 外的普通日志（启动日志、
//! HTTP 访问日志等）不会被捕获。

use std::collections::HashMap;
use std::sync::Mutex;
use std::sync::mpsc::Sender;

use chrono::{DateTime, Utc};
use serde::Serialize;
use tracing::field::Visit;
use tracing::span::{Attributes, Id};
use tracing::{Event, Subscriber};
use tracing_subscriber::layer::{Context, Layer};
use tracing_subscriber::registry::LookupSpan;

/// 单条日志消息的最大长度（字符），超出部分截断，防止异常输出撑爆数据库。
const MAX_LOG_MESSAGE_CHARS: usize = 4096;

/// span 中用于归属日志的 target 标记。
const JOB_SPAN_TARGET: &str = "cron_job_log";

/// 通过广播通道发布的任务日志事件，worker 与 SSE 各自按 `job_name`/`run_id` 过滤。
///
/// `kind` 取值：
/// - `log`：handler 内捕获的一条日志（携带 `seq`/`level`/`message`）
/// - `run_started`：一次执行开始
/// - `run_ended`：一次执行结束（携带 `status`）
#[derive(Clone, Debug, Serialize)]
pub struct JobLogEvent {
    pub kind: String,
    pub job_name: String,
    pub run_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub seq: Option<i32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub level: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub status: Option<String>,
    /// 仅 run_ended 携带：本次执行是否因日志条数上限被截断。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub truncated: Option<bool>,
    /// RFC 3339 UTC 时间戳。
    pub ts: String,
}

impl JobLogEvent {
    pub fn run_started(job_name: &str, run_id: &str, started_at: DateTime<Utc>) -> Self {
        Self {
            kind: "run_started".to_string(),
            job_name: job_name.to_string(),
            run_id: run_id.to_string(),
            seq: None,
            level: None,
            message: None,
            status: None,
            truncated: None,
            ts: started_at.to_rfc3339(),
        }
    }

    pub fn run_ended(
        job_name: &str,
        run_id: &str,
        status: &str,
        ended_at: DateTime<Utc>,
        truncated: bool,
    ) -> Self {
        Self {
            kind: "run_ended".to_string(),
            job_name: job_name.to_string(),
            run_id: run_id.to_string(),
            seq: None,
            level: None,
            message: None,
            status: Some(status.to_string()),
            truncated: Some(truncated),
            ts: ended_at.to_rfc3339(),
        }
    }
}

/// 捕获 cron job span 内日志事件的 [`Layer`]。
///
/// `on_event` 是同步回调，不能 await，因此使用 std 同步通道；
/// lib.rs 中的桥接任务负责把事件转发到 tokio broadcast，供 worker
/// 与 SSE 按 `job_name`（+ `run_id`）过滤订阅。
pub struct JobLogLayer {
    sender: Sender<JobLogEvent>,
    /// span id -> (job_name, run_id)，只登记带归属字段的任务 span。
    job_spans: Mutex<HashMap<Id, (String, String)>>,
}

impl JobLogLayer {
    pub fn new(sender: Sender<JobLogEvent>) -> Self {
        Self {
            sender,
            job_spans: Mutex::new(HashMap::new()),
        }
    }
}

impl<S> Layer<S> for JobLogLayer
where
    S: Subscriber + for<'a> LookupSpan<'a>,
{
    fn on_new_span(&self, attrs: &Attributes<'_>, id: &Id, _ctx: Context<'_, S>) {
        if attrs.metadata().target() != JOB_SPAN_TARGET {
            return;
        }
        let mut recorder = SpanFields::default();
        attrs.record(&mut recorder);
        if let (Some(job_name), Some(run_id)) = (recorder.job_name, recorder.run_id) {
            self.job_spans
                .lock()
                .unwrap()
                .insert(id.clone(), (job_name, run_id));
        }
    }

    fn on_close(&self, id: Id, _ctx: Context<'_, S>) {
        self.job_spans.lock().unwrap().remove(&id);
    }

    fn on_event(&self, event: &Event<'_>, ctx: Context<'_, S>) {
        // 事件必须发生在一个带归属字段的任务 span 内才捕获。
        let Some((job_name, run_id)) = self.lookup_owner(event, &ctx) else {
            return;
        };

        let mut recorder = MessageRecorder::default();
        event.record(&mut recorder);
        let message = trim_and_limit(recorder.message.as_deref().unwrap_or_default());

        // 无界 std 通道的 send 同步且不阻塞；接收端（桥接任务）关闭后事件
        // 静默丢弃，此时没有任何订阅者，不影响运行。
        let _ = self.sender.send(JobLogEvent {
            kind: "log".to_string(),
            job_name,
            run_id,
            seq: None,
            level: Some(event.metadata().level().to_string()),
            message: Some(message),
            status: None,
            truncated: None,
            ts: Utc::now().to_rfc3339(),
        });
    }
}

impl JobLogLayer {
    /// 从事件的实际上下文 span 链（内向外）查找最近的任务 span 归属。
    ///
    /// 注意：`event.parent()` 只返回显式指定的 parent，contextual 事件（宏
    /// 默认形式）返回 None，必须用 `ctx.event_scope` 解析当前 span 链。
    fn lookup_owner<'a, S>(
        &self,
        event: &Event<'_>,
        ctx: &Context<'a, S>,
    ) -> Option<(String, String)>
    where
        S: Subscriber + for<'b> LookupSpan<'b>,
    {
        let scope = ctx.event_scope(event)?;
        for span in scope {
            if let Some(owner) = self.job_spans.lock().unwrap().get(&span.id()) {
                return Some(owner.clone());
            }
        }
        None
    }
}

/// 从 span attributes 中提取 `job_name` / `run_id` 字段。
#[derive(Default)]
struct SpanFields {
    job_name: Option<String>,
    run_id: Option<String>,
}

impl Visit for SpanFields {
    fn record_str(&mut self, field: &tracing::field::Field, value: &str) {
        match field.name() {
            "job_name" => self.job_name = Some(value.to_string()),
            "run_id" => self.run_id = Some(value.to_string()),
            _ => {}
        }
    }

    fn record_debug(&mut self, field: &tracing::field::Field, value: &dyn std::fmt::Debug) {
        match field.name() {
            "job_name" => self.job_name = Some(format!("{value:?}")),
            "run_id" => self.run_id = Some(format!("{value:?}")),
            _ => {}
        }
    }
}

/// 提取日志事件中的 `message` 字段。
#[derive(Default)]
struct MessageRecorder {
    message: Option<String>,
}

impl Visit for MessageRecorder {
    fn record_str(&mut self, field: &tracing::field::Field, value: &str) {
        if field.name() == "message" {
            self.message = Some(value.to_string());
        }
    }

    fn record_debug(&mut self, field: &tracing::field::Field, value: &dyn std::fmt::Debug) {
        if field.name() == "message" {
            self.message = Some(format!("{value:?}"));
        }
    }

    fn record_i64(&mut self, field: &tracing::field::Field, value: i64) {
        if field.name() == "message" {
            self.message = Some(value.to_string());
        }
    }

    fn record_u64(&mut self, field: &tracing::field::Field, value: u64) {
        if field.name() == "message" {
            self.message = Some(value.to_string());
        }
    }

    fn record_bool(&mut self, field: &tracing::field::Field, value: bool) {
        if field.name() == "message" {
            self.message = Some(value.to_string());
        }
    }
}

fn trim_and_limit(message: &str) -> String {
    let trimmed = message.trim();
    let mut chars = trimmed.chars();
    let limited: String = chars.by_ref().take(MAX_LOG_MESSAGE_CHARS).collect();
    if chars.next().is_some() {
        limited + "…"
    } else {
        limited
    }
}

/// tracing 的默认 subscriber 是全局共享的，`with_default`/`set_default` 在并行
/// 测试间会互相覆盖。所有依赖全局 subscriber 的测试（log_capture 模块与
/// worker 模块）都持有这把锁串行执行，保证事件归属正确。
#[cfg(test)]
pub(crate) static SUBSCRIBER_LOCK: Mutex<()> = Mutex::new(());

#[cfg(test)]
mod tests {
    use std::sync::mpsc::Receiver;

    use super::*;
    use tracing_subscriber::Registry;
    use tracing_subscriber::layer::SubscriberExt;

    /// 在全局默认 subscriber 上挂载 JobLogLayer，收集任务 span 内的日志事件。
    /// 返回的 keep_alive 必须存活到事件消费完，避免 channel 断连。
    fn capture_events(
        job_name: &str,
        run_id: &str,
    ) -> (Receiver<JobLogEvent>, std::sync::mpsc::Sender<JobLogEvent>) {
        let (tx, rx) = std::sync::mpsc::channel();
        // 额外保留一个 Sender，避免 subscriber 销毁后 channel 断连。
        let keep_alive = tx.clone();
        let subscriber = Registry::default().with(JobLogLayer::new(tx));
        tracing::subscriber::with_default(subscriber, || {
            let span = tracing::info_span!(
                target: "cron_job_log",
                "cron_job_run",
                job_name = job_name,
                run_id = run_id,
            );
            span.in_scope(|| {
                tracing::info!("step one");
                tracing::warn!("step two with {}", 42);
            });
        });
        (rx, keep_alive)
    }

    #[test]
    fn test_captures_events_inside_job_span() {
        let _guard = SUBSCRIBER_LOCK.lock().unwrap();
        let (rx, _keep_alive) = capture_events("job_a", "run_1");

        let first = rx.recv().unwrap();
        assert_eq!(first.kind, "log");
        assert_eq!(first.job_name, "job_a");
        assert_eq!(first.run_id, "run_1");
        assert_eq!(first.level.as_deref(), Some("INFO"));
        assert_eq!(first.message.as_deref(), Some("step one"));

        let second = rx.recv().unwrap();
        assert_eq!(second.message.as_deref(), Some("step two with 42"));
        assert_eq!(second.level.as_deref(), Some("WARN"));

        // 没有第三条事件。
        assert!(matches!(
            rx.try_recv(),
            Err(std::sync::mpsc::TryRecvError::Empty)
        ));
    }

    #[test]
    fn test_ignores_events_outside_job_span() {
        let _guard = SUBSCRIBER_LOCK.lock().unwrap();
        let (tx, rx) = std::sync::mpsc::channel();
        let keep_alive = tx.clone();
        let subscriber = Registry::default().with(JobLogLayer::new(tx));
        tracing::subscriber::with_default(subscriber, || {
            tracing::info!("unrelated log");
            let span = tracing::info_span!("other_span", foo = "bar");
            span.in_scope(|| {
                tracing::info!("nested but not a job span");
            });
        });
        assert!(matches!(
            rx.try_recv(),
            Err(std::sync::mpsc::TryRecvError::Empty)
        ));
        drop(keep_alive);
    }

    #[test]
    fn test_captures_events_nested_inside_job_span() {
        let _guard = SUBSCRIBER_LOCK.lock().unwrap();
        let (tx, rx) = std::sync::mpsc::channel();
        let keep_alive = tx.clone();
        let subscriber = Registry::default().with(JobLogLayer::new(tx));
        tracing::subscriber::with_default(subscriber, || {
            let span = tracing::info_span!(
                target: "cron_job_log",
                "cron_job_run",
                job_name = "job_b",
                run_id = "run_2",
            );
            span.in_scope(|| {
                let inner = tracing::debug_span!("inner_span");
                inner.in_scope(|| {
                    tracing::info!("nested inside job span");
                });
            });
        });
        let event = rx.recv().unwrap();
        assert_eq!(event.job_name, "job_b");
        assert_eq!(event.run_id, "run_2");
        assert_eq!(event.message.as_deref(), Some("nested inside job span"));
        drop(keep_alive);
    }

    #[test]
    fn test_message_trimmed_and_limited() {
        let long = format!("  {}  ", "x".repeat(5000));
        let limited = trim_and_limit(&long);
        assert!(limited.chars().count() <= MAX_LOG_MESSAGE_CHARS + 1);
        assert!(limited.ends_with('…'));
        assert_eq!(trim_and_limit("  plain  "), "plain");
    }
}
