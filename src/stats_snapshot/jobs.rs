//! 两个内置快照任务的 handler 工厂（`lib.rs::init` 注册时调用）。
//!
//! 任务内部错误经 `?` 交回 worker：run 记 failed 并追加「任务执行失败：…」；
//! 被进程级锁跳过的运行返回 Ok（不算失败）。

use std::sync::Arc;

use crate::cron::{JobContext, JobHandler};

use super::{run_snapshot_generation, run_snapshot_heal};

/// `stats_snapshot`（生成/全量回填）handler。
pub fn generation_job_handler() -> JobHandler {
    Arc::new(|ctx: JobContext| {
        Box::pin(async move {
            run_snapshot_generation(&ctx.db).await?;
            Ok(())
        })
    })
}

/// `stats_snapshot_rebuild`（自愈）handler。
pub fn heal_job_handler() -> JobHandler {
    Arc::new(|ctx: JobContext| {
        Box::pin(async move {
            run_snapshot_heal(&ctx.db).await?;
            Ok(())
        })
    })
}
