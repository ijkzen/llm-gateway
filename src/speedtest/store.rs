//! 测速运行与结果的持久化读写（全部经 SeaORM 实体，不写裸 SQL）。
//!
//! 单行语义由 [`clear_previous`] 保证：开始新运行先删旧运行与旧结果行。这样
//! 页面只有两种形态——「正在跑」或「上一次的成绩」，不需要在 UI 上表达
//! 「新一次正在跑但旧成绩还在」的叠加态。

use sea_orm::sea_query::Expr;
use sea_orm::{
    ActiveModelTrait, ColumnTrait, DatabaseConnection, DbErr, EntityTrait, QueryFilter, QueryOrder,
    Set,
};

use crate::entity::{speed_test_result, speed_test_run};
use crate::speedtest::candidate::Candidate;

pub const RUN_RUNNING: &str = "running";
pub const RUN_COMPLETED: &str = "completed";
pub const RUN_CANCELLED: &str = "cancelled";
pub const RUN_INTERRUPTED: &str = "interrupted";

pub const ROW_PENDING: &str = "pending";
pub const ROW_DONE: &str = "done";
pub const ROW_FAILED: &str = "failed";
pub const ROW_CANCELLED: &str = "cancelled";

/// 删除既有运行与其全部结果行（单行语义的唯一入口）。
pub async fn clear_previous(db: &DatabaseConnection) -> Result<(), DbErr> {
    speed_test_result::Entity::delete_many().exec(db).await?;
    speed_test_run::Entity::delete_many().exec(db).await?;
    Ok(())
}

/// 创建运行行（`running`）并写入全部 pending 占位行，返回运行主键。
///
/// 占位行让「还剩几个没测」天然可读：行数从开始就是候选总数，逐行终态化，
/// 取消时把剩余 pending 转 `cancelled`。
pub async fn create_run(
    db: &DatabaseConnection,
    candidates: &[Candidate],
    provider_ids: &[i32],
    vm_ids: &[i32],
    keyword: &str,
    prompt: &str,
    max_tokens: i32,
) -> Result<i32, DbErr> {
    let run = speed_test_run::ActiveModel {
        status: Set(RUN_RUNNING.to_string()),
        started_at: Set(chrono::Utc::now().timestamp_millis()),
        finished_at: Set(None),
        total: Set(candidates.len() as i32),
        provider_ids: Set(serde_json::to_string(provider_ids).unwrap_or_else(|_| "[]".into())),
        vm_ids: Set(serde_json::to_string(vm_ids).unwrap_or_else(|_| "[]".into())),
        keyword: Set(keyword.to_string()),
        prompt: Set(prompt.to_string()),
        max_tokens: Set(max_tokens),
        ..Default::default()
    }
    .insert(db)
    .await?;

    let rows: Vec<speed_test_result::ActiveModel> = candidates
        .iter()
        .map(|c| speed_test_result::ActiveModel {
            run_id: Set(run.id),
            provider_id: Set(c.provider.id),
            provider_name: Set(c.provider.name.clone()),
            provider_model_pk: Set(c.model.model_id),
            model_id: Set(c.model.provider_model_id.clone()),
            status: Set(ROW_PENDING.to_string()),
            ..Default::default()
        })
        .collect();
    if !rows.is_empty() {
        speed_test_result::Entity::insert_many(rows)
            .exec(db)
            .await?;
    }
    Ok(run.id)
}

/// 用内存结果覆盖某一行（按 run + 供应商模型主键定位）。
pub async fn save_row(
    db: &DatabaseConnection,
    run_id: i32,
    provider_model_pk: i32,
    row: speed_test_result::ActiveModel,
) -> Result<(), DbErr> {
    speed_test_result::Entity::update_many()
        .set(row)
        .filter(speed_test_result::Column::RunId.eq(run_id))
        .filter(speed_test_result::Column::ProviderModelPk.eq(provider_model_pk))
        .exec(db)
        .await?;
    Ok(())
}

/// 把仍为 pending 的行转 `cancelled`（取消时调用）。
pub async fn cancel_pending(db: &DatabaseConnection, run_id: i32) -> Result<u64, DbErr> {
    let now = chrono::Utc::now().timestamp_millis();
    let res = speed_test_result::Entity::update_many()
        .set(speed_test_result::ActiveModel {
            status: Set(ROW_CANCELLED.to_string()),
            finished_at: Set(Some(now)),
            ..Default::default()
        })
        .filter(speed_test_result::Column::RunId.eq(run_id))
        .filter(speed_test_result::Column::Status.eq(ROW_PENDING))
        .exec(db)
        .await?;
    Ok(res.rows_affected)
}

/// 运行收尾：状态 + 结束时刻。
pub async fn finish_run(db: &DatabaseConnection, run_id: i32, status: &str) -> Result<(), DbErr> {
    speed_test_run::Entity::update_many()
        .set(speed_test_run::ActiveModel {
            status: Set(status.to_string()),
            finished_at: Set(Some(chrono::Utc::now().timestamp_millis())),
            ..Default::default()
        })
        .filter(speed_test_run::Column::Id.eq(run_id))
        .exec(db)
        .await?;
    Ok(())
}

/// 读取当前/最近一次运行及其全部结果行（无运行返回 None）。
///
/// 结果行按「成功优先（TPS 降序）→ 失败 → 未开始」排序：服务端给出稳定默认序，
/// 前端点列头只做本地重排，两者对「默认第一行是谁」的理解一致。
pub async fn load_current(
    db: &DatabaseConnection,
) -> Result<Option<(speed_test_run::Model, Vec<speed_test_result::Model>)>, DbErr> {
    let Some(run) = speed_test_run::Entity::find()
        .order_by_desc(speed_test_run::Column::Id)
        .one(db)
        .await?
    else {
        return Ok(None);
    };
    let rows = speed_test_result::Entity::find()
        .filter(speed_test_result::Column::RunId.eq(run.id))
        .order_by_desc(speed_test_result::Column::Tps.is_not_null())
        .order_by_desc(speed_test_result::Column::Tps)
        .order_by_asc(speed_test_result::Column::ProviderName)
        .order_by_asc(speed_test_result::Column::ModelId)
        .all(db)
        .await?;
    Ok(Some((run, rows)))
}

/// 启动时把残留的 `running` 运行改判 `interrupted`（服务重启打断）。
///
/// 已完成的结果行原样保留：用户回来能看见「上次测速被中断，以下已完成」，
/// 比悄悄清空更符合直觉。返回是否有残留运行被改判。
pub async fn mark_interrupted(db: &DatabaseConnection) -> Result<bool, DbErr> {
    let now = chrono::Utc::now().timestamp_millis();
    let res = speed_test_run::Entity::update_many()
        .col_expr(
            speed_test_run::Column::Status,
            Expr::value(RUN_INTERRUPTED.to_string()),
        )
        .col_expr(speed_test_run::Column::FinishedAt, Expr::value(Some(now)))
        .filter(speed_test_run::Column::Status.eq(RUN_RUNNING))
        .exec(db)
        .await?;
    Ok(res.rows_affected > 0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn status_constants_cover_four_run_states() {
        assert_ne!(RUN_RUNNING, RUN_COMPLETED);
        assert_ne!(RUN_COMPLETED, RUN_CANCELLED);
        assert_ne!(RUN_CANCELLED, RUN_INTERRUPTED);
        assert_eq!(ROW_PENDING, "pending");
    }
}
