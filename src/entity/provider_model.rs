use sea_orm::entity::prelude::*;
use serde::{Deserialize, Serialize};

/// ProviderModel:登记在某个供应商名下的具体模型条目。
#[derive(Clone, Debug, PartialEq, DeriveEntityModel, Serialize, Deserialize)]
#[sea_orm(table_name = "provider_model")]
#[serde(rename_all = "camelCase")]
pub struct Model {
    #[sea_orm(primary_key, auto_increment = true)]
    pub model_id: i32,
    /// 所属供应商 id(逻辑外键;供应商删除时应用层级联硬删)。
    pub provider_id: i32,
    /// 远端模型 ID 字符串,如 `gpt-4o`;同一供应商内唯一(复合唯一索引,见 migration 5)。
    pub provider_model_id: String,
    /// 上下文长度(token 数)。
    pub context_length: i64,
    /// 最大输出 token 数。
    pub max_output_tokens: i64,
    /// 是否支持推理。
    #[sea_orm(default_value = "0")]
    pub reasoning: bool,
    /// 是否支持工具调用。
    #[sea_orm(default_value = "0")]
    pub tool_use: bool,
    /// 是否支持图像理解。
    #[sea_orm(default_value = "0")]
    pub image_understand: bool,
    /// 是否支持视频理解。
    #[sea_orm(default_value = "0")]
    pub video_understand: bool,
    /// 是否经网络代理（HTTP 代理）转发该模型的上游请求。
    /// 关闭（false）时回落到供应商代理。
    #[sea_orm(default_value = "0")]
    pub proxy_enabled: bool,
    /// HTTP 代理地址（如 `http://127.0.0.1:7890`，无认证）。
    #[sea_orm(default_value = "")]
    pub proxy_addr: String,
    pub created_at: DateTimeUtc,
    pub updated_at: DateTimeUtc,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {}

impl ActiveModelBehavior for ActiveModel {}
