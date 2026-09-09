use sea_orm::entity::prelude::*;
use serde::{Deserialize, Serialize};

/// Request 统计快照：闭口时间桶（duration_type × [start_time, end_time)）内按
/// 主体（entity_type × entity）× 指标（metric_type）预聚合的一行（EAV 窄表）。
/// 只由快照生成任务（stats_snapshot/重建）写入，读路径闭桶取数、未覆盖部分实时兑底。
#[derive(Clone, Debug, PartialEq, DeriveEntityModel, Serialize, Deserialize)]
#[sea_orm(table_name = "request_log_snapshot")]
pub struct Model {
    #[sea_orm(primary_key, auto_increment = true)]
    pub id: i32,
    /// 时间粒度：hour / day / month / year。
    pub duration_type: String,
    /// 桶起点（毫秒时间戳，含；按生成时设置表时区对齐的本地边界）。
    pub start_time: i64,
    /// 桶终点（毫秒时间戳，不含）。
    pub end_time: i64,
    /// 主体类型：whole / provider / model / virtual_model / api_key / 交叉型。
    pub entity_type: String,
    /// 主体键：对应表主键 id 的字符串；交叉型为英文逗号分隔复合键；whole 为空串。
    pub entity: String,
    /// 指标名（注册表单一事实源，见 stats_snapshot registry）。
    pub metric_type: String,
    /// 指标值：加和/计数原语或闭桶标量（分位），一律 REAL。
    pub metric_value: f64,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {}

impl ActiveModelBehavior for ActiveModel {}
