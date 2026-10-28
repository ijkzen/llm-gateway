//! 测速候选集与过滤（单一事实源）。
//!
//! 预览接口（「将测 N 个模型」）与启动接口共用 [`resolve`]，保证按钮上的数字
//! 与实际会跑的模型完全一致——两处各写一遍过滤必然走歧。
//!
//! 候选门槛（ADR-0027）：
//! - 供应商：`enable = true` 且 `disabled_reason IS NULL`。额度耗尽（`quota`）、
//!   连续失败禁用（`failure`）、手动停用（`manual`）一律不进候选，避免对注定
//!   失败的上游白烧长请求。`provider_model` 表本身没有 enable 字段，模型层不再
//!   二次判定。
//! - 虚拟模型过滤：选中虚拟模型时，只保留被这些虚拟模型的条目引用到的模型
//!   （不关心条目自身是否启用——测的是上游模型能力，不是它在某个虚拟模型里
//!   当前是否被路由到）。
//! - 关键词：供应商名 ∪ 上游模型名 ∪ 关联虚拟模型 display_id，子串、忽略大小写。

use std::collections::{HashMap, HashSet};

use sea_orm::{ColumnTrait, DatabaseConnection, DbErr, EntityTrait, QueryFilter, QueryOrder};

use crate::entity::{provider, provider_model, virtual_model, virtual_model_item};

/// 一次测速的过滤条件。空集合/空串 = 该维度不过滤。
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Filter {
    pub provider_ids: Vec<i32>,
    pub vm_ids: Vec<i32>,
    pub keyword: String,
}

impl Filter {
    /// 是否与「什么条件都不加」等价（用于展示「全部启用模型」之类的提示）。
    pub fn is_empty(&self) -> bool {
        self.provider_ids.is_empty() && self.vm_ids.is_empty() && self.keyword.trim().is_empty()
    }
}

/// 一个待测单元：供应商行 + 其名下的一个模型行。
#[derive(Debug, Clone)]
pub struct Candidate {
    pub provider: provider::Model,
    pub model: provider_model::Model,
    /// 该模型被哪些虚拟模型引用（display_id，已排序去重），供关键词匹配与展示。
    pub vm_display_ids: Vec<String>,
}

/// 关键词是否命中：供应商名 / 上游模型名 / 任一类联虚拟模型 display_id。
pub fn matches_keyword(
    provider_name: &str,
    model_id: &str,
    vm_display_ids: &[String],
    kw: &str,
) -> bool {
    let kw = kw.trim();
    if kw.is_empty() {
        return true;
    }
    let kw = kw.to_lowercase();
    provider_name.to_lowercase().contains(&kw)
        || model_id.to_lowercase().contains(&kw)
        || vm_display_ids
            .iter()
            .any(|d| d.to_lowercase().contains(&kw))
}

/// 解析候选集：供应商门槛 → 虚拟模型过滤 → 关键词，按供应商名、模型名排序。
pub async fn resolve(db: &DatabaseConnection, filter: &Filter) -> Result<Vec<Candidate>, DbErr> {
    let providers = provider::Entity::find()
        .filter(provider::Column::Enable.eq(true))
        .filter(provider::Column::DisabledReason.is_null())
        .order_by_asc(provider::Column::SortOrder)
        .order_by_asc(provider::Column::Id)
        .all(db)
        .await?;

    let allowed_providers: Option<HashSet<i32>> = if filter.provider_ids.is_empty() {
        None
    } else {
        Some(filter.provider_ids.iter().copied().collect())
    };

    // 模型 → 引用它的虚拟模型 display_id（全量，供关键词与虚拟模型过滤共用）。
    let items = virtual_model_item::Entity::find().all(db).await?;
    let vms = virtual_model::Entity::find().all(db).await?;
    let display_by_vm: HashMap<i32, &str> = vms
        .iter()
        .map(|v| (v.virtual_model_id, v.display_id.as_str()))
        .collect();

    let mut vms_by_model: HashMap<i32, Vec<String>> = HashMap::new();
    for item in &items {
        if let Some(display) = display_by_vm.get(&item.virtual_model_id) {
            vms_by_model
                .entry(item.model_id)
                .or_default()
                .push((*display).to_string());
        }
    }
    for displays in vms_by_model.values_mut() {
        displays.sort();
        displays.dedup();
    }

    // 虚拟模型过滤：选中集合下被引用到的模型主键。
    let vm_scoped_models: Option<HashSet<i32>> = if filter.vm_ids.is_empty() {
        None
    } else {
        let selected: HashSet<i32> = filter.vm_ids.iter().copied().collect();
        Some(
            items
                .iter()
                .filter(|item| selected.contains(&item.virtual_model_id))
                .map(|item| item.model_id)
                .collect(),
        )
    };

    let mut out = Vec::new();
    for provider_row in providers {
        if let Some(allowed) = &allowed_providers
            && !allowed.contains(&provider_row.id)
        {
            continue;
        }
        let models = provider_model::Entity::find()
            .filter(provider_model::Column::ProviderId.eq(provider_row.id))
            .order_by_asc(provider_model::Column::ProviderModelId)
            .all(db)
            .await?;
        for model in models {
            if let Some(scoped) = &vm_scoped_models
                && !scoped.contains(&model.model_id)
            {
                continue;
            }
            let vm_display_ids = vms_by_model
                .get(&model.model_id)
                .cloned()
                .unwrap_or_default();
            if !matches_keyword(
                &provider_row.name,
                &model.provider_model_id,
                &vm_display_ids,
                &filter.keyword,
            ) {
                continue;
            }
            out.push(Candidate {
                provider: provider_row.clone(),
                model,
                vm_display_ids,
            });
        }
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keyword_matches_any_of_three_fields() {
        let vms = vec!["gpt-4o-prod".to_string()];
        assert!(matches_keyword("云雾", "gpt-4o", &vms, "云雾"));
        assert!(matches_keyword("云雾", "gpt-4o", &vms, "4O"));
        assert!(matches_keyword("云雾", "gpt-4o", &vms, "PROD"));
        assert!(!matches_keyword("云雾", "gpt-4o", &vms, "claude"));
    }

    #[test]
    fn empty_keyword_matches_everything_including_whitespace() {
        assert!(matches_keyword("a", "b", &[], ""));
        assert!(matches_keyword("a", "b", &[], "   "));
    }

    #[test]
    fn filter_is_empty_only_when_all_dimensions_blank() {
        assert!(Filter::default().is_empty());
        assert!(
            Filter {
                keyword: "  ".into(),
                ..Default::default()
            }
            .is_empty()
        );
        assert!(
            !Filter {
                provider_ids: vec![1],
                ..Default::default()
            }
            .is_empty()
        );
        assert!(
            !Filter {
                vm_ids: vec![2],
                ..Default::default()
            }
            .is_empty()
        );
        assert!(
            !Filter {
                keyword: "x".into(),
                ..Default::default()
            }
            .is_empty()
        );
    }
}
