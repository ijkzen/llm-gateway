use sea_orm::entity::prelude::*;
use serde::{Deserialize, Serialize};

/// SpeedTestRun: 一次批量测速运行的元信息（ADR-0027）。
///
/// 单行语义：同一时刻至多一条运行记录，开始新运行即删除旧运行与其结果行。
/// 状态四态：`running`（测速中）/ `completed`（已完成）/ `cancelled`（管理员取消）
/// / `interrupted`（服务重启打断，启动时由残留 `running` 改写）。
///
/// 过滤条件与题面以快照形式落库：成绩旁边始终带着「这次是怎么跑出来的」，
/// 将来改内置题面后旧成绩仍可解释；`total` 是候选总数，等于本次 pending 行数。
#[derive(Clone, Debug, PartialEq, DeriveEntityModel, Serialize, Deserialize)]
#[sea_orm(table_name = "speed_test_run")]
#[serde(rename_all = "camelCase")]
pub struct Model {
    #[sea_orm(primary_key, auto_increment = true)]
    pub id: i32,
    pub status: String,
    /// 运行开始时刻（毫秒时间戳）。
    pub started_at: i64,
    /// 运行结束时刻（毫秒时间戳）；运行中为 NULL。
    #[sea_orm(nullable)]
    pub finished_at: Option<i64>,
    /// 候选总数（= 本次结果行数）。
    pub total: i32,
    /// 供应商过滤快照（JSON 字符串数组，空数组 = 不过滤）。
    pub provider_ids: String,
    /// 虚拟模型过滤快照（JSON 字符串数组，空数组 = 不过滤）。
    pub vm_ids: String,
    /// 关键词过滤快照（空串 = 不过滤）。
    pub keyword: String,
    /// 本次使用的题面快照。
    pub prompt: String,
    /// 本次目标输出 token 上限。
    pub max_tokens: i32,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {}

impl ActiveModelBehavior for ActiveModel {}
