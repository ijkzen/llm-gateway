use sea_orm::entity::prelude::*;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Eq, EnumIter, DeriveActiveEnum, Serialize, Deserialize)]
#[sea_orm(rs_type = "i32", db_type = "Integer")]
pub enum SettingType {
    String = 0,
    Float = 1,
    Int = 2,
    Bool = 3,
    Json = 4,
}

impl std::fmt::Display for SettingType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SettingType::String => write!(f, "String"),
            SettingType::Float => write!(f, "Float"),
            SettingType::Int => write!(f, "Int"),
            SettingType::Bool => write!(f, "Bool"),
            SettingType::Json => write!(f, "Json"),
        }
    }
}

impl std::str::FromStr for SettingType {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "String" => Ok(SettingType::String),
            "Float" => Ok(SettingType::Float),
            "Int" => Ok(SettingType::Int),
            "Bool" => Ok(SettingType::Bool),
            "Json" => Ok(SettingType::Json),
            _ => Err(format!("invalid setting type: {}", s)),
        }
    }
}

impl std::convert::TryFrom<i32> for SettingType {
    type Error = String;

    fn try_from(value: i32) -> Result<Self, Self::Error> {
        match value {
            0 => Ok(SettingType::String),
            1 => Ok(SettingType::Float),
            2 => Ok(SettingType::Int),
            3 => Ok(SettingType::Bool),
            4 => Ok(SettingType::Json),
            _ => Err(format!("invalid setting type value: {}", value)),
        }
    }
}

#[derive(Clone, Debug, PartialEq, DeriveEntityModel, Serialize, Deserialize)]
#[sea_orm(table_name = "setting")]
#[serde(rename_all = "camelCase")]
pub struct Model {
    #[sea_orm(primary_key, column_type = "Text")]
    #[serde(rename = "key")]
    pub key: String,
    #[sea_orm(column_type = "Text")]
    pub value: String,
    pub r#type: i32,
    pub updated_at: DateTimeUtc,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {}

impl ActiveModelBehavior for ActiveModel {}
