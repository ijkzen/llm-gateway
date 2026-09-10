//! 配置备份导出 / 恢复（整体替换）。
//!
//! 导出内容：供应商（含明文 api_key/extra）、各供应商名下模型、虚拟模型及
//! 成员（成员以 `providerName`+`providerModelId` 自然键引用）、API Key（明文）、
//! 系统设置，组装为 versioned JSON。
//! 导入流程：解析与语义校验（自然键唯一、成员引用可解析、枚举/数值合法）后，
//! 在单事务内整体替换上述配置（设置只 upsert 覆盖、不删除），任一步失败回滚。

use sea_orm::{
    ActiveModelTrait, ColumnTrait, DatabaseConnection, DbErr, EntityTrait, QueryFilter, QueryOrder,
    Set, TransactionTrait,
};
use serde::{Deserialize, Serialize};
use serde_json::Value as JsonValue;

use crate::auth::hash_token;
use crate::crypto;
use crate::entity::provider_template::{BILLING_MODE_PAY_AS_YOU_GO, BILLING_MODE_SUBSCRIPTION};
use crate::entity::virtual_model::{
    INTERFACE_FULL_COMPATIBLE, INTERFACE_OPENAI_COMPAT, LB_RANDOM, LB_SUBSCRIPTION_FIRST,
};
use crate::entity::{
    api_key, provider, provider_model, setting, virtual_model, virtual_model_item,
};
use crate::provider_model::refresh::{PROTOCOL_GEMINI, PROTOCOL_OPENAI_COMPATIBLE};

/// 当前备份格式版本；`parse_backup` 只接受与该值一致的版本。
pub const BACKUP_VERSION: i32 = 1;

// ─── 备份文件结构 ───

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BackupFile {
    pub version: i32,
    pub exported_at: String,
    pub providers: Vec<BackupProvider>,
    pub virtual_models: Vec<BackupVirtualModel>,
    pub api_keys: Vec<BackupApiKey>,
    pub settings: Vec<BackupSetting>,
    /// 15-04：导出时解密失败的凭据条数（密钥轮换/异机导出等）。非 0 表示文件里
    /// 这些凭据已退化为空串——原本静默（校验全过、凭据全空），现由调用方回带提示。
    #[serde(default, skip_serializing_if = "is_zero")]
    pub decrypt_failures: usize,
}

fn is_zero(value: &usize) -> bool {
    *value == 0
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BackupProvider {
    pub name: String,
    pub enable: bool,
    pub base_url: String,
    pub api_key: String,
    pub custom_header: String,
    pub protocol_type: i32,
    pub billing_mode: i32,
    pub extra: String,
    pub sort_order: i32,
    pub proxy_enabled: bool,
    pub proxy_addr: String,
    pub disabled_reason: Option<String>,
    pub models: Vec<BackupProviderModel>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BackupProviderModel {
    pub provider_model_id: String,
    pub context_length: i64,
    pub max_output_tokens: i64,
    pub reasoning: bool,
    pub tool_use: bool,
    pub image_understand: bool,
    pub video_understand: bool,
    pub protocol_type: Option<i32>,
    pub proxy_enabled: bool,
    pub proxy_addr: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BackupVirtualModel {
    pub display_id: String,
    pub enable: bool,
    pub load_balancing_strategy: i32,
    pub fallback_strategy: i32,
    pub interface_type: i32,
    pub items: Vec<BackupVirtualModelItem>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BackupVirtualModelItem {
    /// 成员归属供应商名（自然键，跨库稳定）。
    pub provider_name: String,
    /// 成员远端模型 ID（自然键，与 providerName 联合定位模型）。
    pub provider_model_id: String,
    pub enable: bool,
    pub cascade_disabled: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BackupApiKey {
    pub name: String,
    pub key: String,
    pub enable: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BackupSetting {
    pub key: String,
    pub value: String,
    /// 设置类型名（String/Float/Int/Bool/Json），与前端展示口径一致。
    pub r#type: String,
}

/// 导入结果计数，供路由返回给前端。
#[derive(Debug, Clone, Serialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct ImportSummary {
    pub providers: usize,
    pub models: usize,
    pub virtual_models: usize,
    pub api_keys: usize,
    pub settings: usize,
}

// ─── 导出 ───

/// 读全量配置组装为备份结构；供应商 api_key/extra 与 API Key 的 key 解密为明文。
pub async fn build_export(db: &DatabaseConnection) -> Result<BackupFile, DbErr> {
    // 15-04：统计解密失败（含 provider.api_key/extra 与 api_key.key）。
    let mut decrypt_failures = 0usize;
    let mut decrypt_or_count = |stored: &str| match crypto::decrypt(stored) {
        Ok(plain) => plain,
        Err(_) => {
            decrypt_failures += 1;
            String::new()
        }
    };
    let providers = provider::Entity::find()
        .order_by_asc(provider::Column::Id)
        .all(db)
        .await?;
    let models = provider_model::Entity::find()
        .order_by_asc(provider_model::Column::ModelId)
        .all(db)
        .await?;
    let virtual_models = virtual_model::Entity::find()
        .order_by_asc(virtual_model::Column::VirtualModelId)
        .all(db)
        .await?;
    let items = virtual_model_item::Entity::find()
        .order_by_asc(virtual_model_item::Column::VirtualModelItemId)
        .all(db)
        .await?;
    let api_keys = api_key::Entity::find()
        .order_by_asc(api_key::Column::Id)
        .all(db)
        .await?;
    // 15-05：与其余五表一致加排序——否则 settings 序不稳定，导出→再导出
    // 无法逐字节比对（该表原先漏了 order_by）。
    let settings = setting::Entity::find()
        .order_by_asc(setting::Column::Key)
        .all(db)
        .await?;

    let model_by_id: std::collections::HashMap<i32, &provider_model::Model> =
        models.iter().map(|m| (m.model_id, m)).collect();
    let provider_by_id: std::collections::HashMap<i32, &provider::Model> =
        providers.iter().map(|p| (p.id, p)).collect();

    let mut providers_out = Vec::with_capacity(providers.len());
    for p in &providers {
        let models_for_p: Vec<BackupProviderModel> = models
            .iter()
            .filter(|m| m.provider_id == p.id)
            .map(|m| BackupProviderModel {
                provider_model_id: m.provider_model_id.clone(),
                context_length: m.context_length,
                max_output_tokens: m.max_output_tokens,
                reasoning: m.reasoning,
                tool_use: m.tool_use,
                image_understand: m.image_understand,
                video_understand: m.video_understand,
                protocol_type: m.protocol_type,
                proxy_enabled: m.proxy_enabled,
                proxy_addr: m.proxy_addr.clone(),
            })
            .collect();
        providers_out.push(BackupProvider {
            name: p.name.clone(),
            enable: p.enable,
            base_url: p.base_url.clone(),
            api_key: decrypt_or_count(&p.api_key),
            custom_header: p.custom_header.clone(),
            protocol_type: p.protocol_type,
            billing_mode: p.billing_mode,
            extra: decrypt_or_count(&p.extra),
            sort_order: p.sort_order,
            proxy_enabled: p.proxy_enabled,
            proxy_addr: p.proxy_addr.clone(),
            disabled_reason: p.disabled_reason.clone(),
            models: models_for_p,
        });
    }

    let virtual_models_out = virtual_models
        .into_iter()
        .map(|vm| {
            let vm_items: Vec<BackupVirtualModelItem> = items
                .iter()
                .filter(|it| it.virtual_model_id == vm.virtual_model_id)
                .filter_map(|it| {
                    let model = model_by_id.get(&it.model_id)?;
                    let prov = provider_by_id.get(&model.provider_id)?;
                    Some(BackupVirtualModelItem {
                        provider_name: prov.name.clone(),
                        provider_model_id: model.provider_model_id.clone(),
                        enable: it.enable,
                        cascade_disabled: it.cascade_disabled,
                    })
                })
                .collect();
            BackupVirtualModel {
                display_id: vm.display_id,
                enable: vm.enable,
                load_balancing_strategy: vm.load_balancing_strategy,
                fallback_strategy: vm.fallback_strategy,
                interface_type: vm.interface_type,
                items: vm_items,
            }
        })
        .collect();

    let api_keys_out: Vec<BackupApiKey> = api_keys
        .into_iter()
        .map(|k| BackupApiKey {
            name: k.name,
            key: decrypt_or_count(&k.key),
            enable: k.enable,
        })
        .collect();

    let settings_out: Vec<BackupSetting> = settings
        .into_iter()
        .map(|s| BackupSetting {
            key: s.key,
            value: s.value,
            r#type: setting_type_name(s.r#type),
        })
        .collect();

    Ok(BackupFile {
        version: BACKUP_VERSION,
        exported_at: chrono::Utc::now().to_rfc3339(),
        providers: providers_out,
        virtual_models: virtual_models_out,
        api_keys: api_keys_out,
        settings: settings_out,
        decrypt_failures,
    })
}

// ─── 设置类型字符串 ↔ 枚举 ───

/// i32 → 设置类型名；未知类型按 String 处理（与前端展示口径一致）。
/// 设置类型编号 → 名称。15-06：未知编号不再静默降级为 String（那会让再导入
/// 时把原类型改写成 String），改为原样回带数字串，导入侧 `setting_type_from_name`
/// 会明确报「类型非法」——保真且失败可见。
fn setting_type_name(t: i32) -> String {
    setting::SettingType::try_from(t)
        .map(|v| v.to_string())
        .unwrap_or_else(|_| t.to_string())
}

/// 设置类型名 → i32。非法名返回错误消息。
fn setting_type_from_name(name: &str) -> Result<i32, String> {
    name.parse::<setting::SettingType>()
        .map(|t| t as i32)
        .map_err(|_| format!("设置类型 '{name}' 非法（支持 String/Float/Int/Bool/Json）"))
}

// ─── 解析与校验 ───

/// 解析备份 JSON 并做结构校验（版本号、顶层类型），返回带定位的精确错误。
pub fn parse_backup(input: &str) -> Result<BackupFile, String> {
    let value: JsonValue =
        serde_json::from_str(input).map_err(|e| format!("不是合法的 JSON：{e}"))?;

    let obj = value
        .as_object()
        .ok_or_else(|| "备份文件顶层必须是 JSON 对象".to_string())?;

    let version = obj
        .get("version")
        .and_then(JsonValue::as_i64)
        .ok_or_else(|| "备份文件缺少 version 字段（须为整数）".to_string())?;
    if version != BACKUP_VERSION as i64 {
        return Err(format!(
            "备份文件版本不支持：version={version}（当前支持 {BACKUP_VERSION}）"
        ));
    }

    for key in ["providers", "virtualModels", "apiKeys", "settings"] {
        if !obj.contains_key(key) {
            return Err(format!("备份文件缺少 {key} 字段"));
        }
    }

    serde_json::from_value::<BackupFile>(value).map_err(|e| format!("备份文件结构不完整：{e}"))
}

/// 语义校验：自然键非空且唯一、成员引用可解析、枚举/数值在合法范围、
/// 成员生效协议与虚拟模型接口类型匹配。返回第一个错误（中文，含定位）。
pub fn validate_backup(file: &BackupFile) -> Result<(), String> {
    let mut provider_names = std::collections::HashSet::new();
    let mut model_keys = std::collections::HashSet::new();
    // 15-03：同次导入内的成员唯一性（跨虚拟模型也不允许同一 (供应商,模型) 重复）。
    let mut seen_members: std::collections::HashSet<(String, String)> =
        std::collections::HashSet::new();
    let mut model_protocols: std::collections::HashMap<(String, String), i32> =
        std::collections::HashMap::new();
    let mut vm_display_ids = std::collections::HashSet::new();
    let mut api_key_names = std::collections::HashSet::new();

    for (pi, p) in file.providers.iter().enumerate() {
        let loc = format!("providers[{pi}]");
        if p.name.trim().is_empty() {
            return Err(format!("{loc}.name 不能为空"));
        }
        if !provider_names.insert(p.name.trim().to_string()) {
            return Err(format!("{loc}.name 重复：{}", p.name));
        }
        if p.base_url.trim().is_empty() {
            return Err(format!("{loc}.baseUrl 不能为空"));
        }
        // 14-07：协议编号区间由常量收敛（新增编号时不会静默误拒）。
        if !(PROTOCOL_OPENAI_COMPATIBLE..=PROTOCOL_GEMINI).contains(&p.protocol_type) {
            return Err(format!("{loc}.protocolType 取值非法：{}", p.protocol_type));
        }
        if !(BILLING_MODE_PAY_AS_YOU_GO..=BILLING_MODE_SUBSCRIPTION).contains(&p.billing_mode) {
            return Err(format!("{loc}.billingMode 取值非法：{}", p.billing_mode));
        }
        for (mi, m) in p.models.iter().enumerate() {
            let m_loc = format!("{loc}.models[{mi}]");
            let key = (
                p.name.trim().to_string(),
                m.provider_model_id.trim().to_string(),
            );
            if m.provider_model_id.trim().is_empty() {
                return Err(format!("{m_loc}.providerModelId 不能为空"));
            }
            if !model_keys.insert(key.clone()) {
                return Err(format!(
                    "{m_loc}.providerModelId 重复：{}",
                    m.provider_model_id
                ));
            }
            if m.context_length <= 0 {
                return Err(format!("{m_loc}.contextLength 必须为正整数"));
            }
            if m.max_output_tokens <= 0 {
                return Err(format!("{m_loc}.maxOutputTokens 必须为正整数"));
            }
            if m.protocol_type
                .is_some_and(|v| !(PROTOCOL_OPENAI_COMPATIBLE..=PROTOCOL_GEMINI).contains(&v))
            {
                return Err(format!("{m_loc}.protocolType 取值非法"));
            }
            // 生效协议 = 模型级覆盖 ?? 供应商协议（与选路口径一致）。
            model_protocols.insert(key, m.protocol_type.unwrap_or(p.protocol_type));
        }
    }

    for (vi, vm) in file.virtual_models.iter().enumerate() {
        let loc = format!("virtualModels[{vi}]");
        if vm.display_id.trim().is_empty() {
            return Err(format!("{loc}.displayId 不能为空"));
        }
        if !vm_display_ids.insert(vm.display_id.trim().to_string()) {
            return Err(format!("{loc}.displayId 重复：{}", vm.display_id));
        }
        if !(LB_SUBSCRIPTION_FIRST..=LB_RANDOM).contains(&vm.load_balancing_strategy) {
            return Err(format!(
                "{loc}.loadBalancingStrategy 取值非法：{}",
                vm.load_balancing_strategy
            ));
        }
        if !(0..=1).contains(&vm.fallback_strategy) {
            return Err(format!(
                "{loc}.fallbackStrategy 取值非法：{}",
                vm.fallback_strategy
            ));
        }
        if !(INTERFACE_OPENAI_COMPAT..=INTERFACE_FULL_COMPATIBLE).contains(&vm.interface_type) {
            return Err(format!(
                "{loc}.interfaceType 取值非法：{}",
                vm.interface_type
            ));
        }
        for (ii, it) in vm.items.iter().enumerate() {
            let i_loc = format!("{loc}.items[{ii}]");
            let key = (
                it.provider_name.trim().to_string(),
                it.provider_model_id.trim().to_string(),
            );
            if key.0.is_empty() || key.1.is_empty() {
                return Err(format!(
                    "{i_loc} 成员引用缺失（providerName / providerModelId）"
                ));
            }
            if !model_keys.contains(&key) {
                return Err(format!(
                    "{i_loc} 引用的模型不存在：{}/{}",
                    it.provider_name, it.provider_model_id
                ));
            }
            // 15-03：同次导入内 (providerName, providerModelId) 不得重复——数据库
            // 唯一索引会把重复撞成裸 SQL 错误文案。
            if !seen_members.insert(key.clone()) {
                return Err(format!(
                    "{i_loc} 成员重复：{}/{}（同一次导入内同一供应商模型不可重复）",
                    it.provider_name, it.provider_model_id
                ));
            }
            // 非 Full Compatible 时成员生效协议必须与虚拟模型接口类型一致
            // （接口类型编号与协议编号对齐，见 virtual_model 常量）。
            if vm.interface_type != INTERFACE_FULL_COMPATIBLE {
                let effective = model_protocols.get(&key).copied().unwrap_or_default();
                if effective != vm.interface_type {
                    return Err(format!(
                        "{i_loc} 成员协议与虚拟模型接口类型不匹配：模型 {} 生效协议 {effective}，虚拟模型接口类型 {}",
                        it.provider_model_id, vm.interface_type
                    ));
                }
            }
        }
    }

    for (ki, k) in file.api_keys.iter().enumerate() {
        if k.name.trim().is_empty() {
            return Err(format!("apiKeys[{ki}].name 不能为空"));
        }
        if !api_key_names.insert(k.name.trim().to_string()) {
            return Err(format!("apiKeys[{ki}].name 重复：{}", k.name));
        }
    }

    for (si, s) in file.settings.iter().enumerate() {
        if s.key.trim().is_empty() {
            return Err(format!("settings[{si}].key 不能为空"));
        }
        setting_type_from_name(&s.r#type).map_err(|e| format!("settings[{si}]：{e}"))?;
    }

    Ok(())
}

// ─── 导入（整体替换） ───

/// 在单事务内整体替换配置。失败时整体回滚，库保持原样。
pub async fn apply_import(
    db: &DatabaseConnection,
    file: BackupFile,
) -> Result<ImportSummary, String> {
    let txn = db.begin().await.map_err(|e| format!("开启事务失败：{e}"))?;

    // 依赖序删除：成员 → 虚拟模型 → 供应商模型 → 供应商 → API Key。
    virtual_model_item::Entity::delete_many()
        .exec(&txn)
        .await
        .map_err(|e| format!("清空成员失败：{e}"))?;
    virtual_model::Entity::delete_many()
        .exec(&txn)
        .await
        .map_err(|e| format!("清空虚拟模型失败：{e}"))?;
    provider_model::Entity::delete_many()
        .exec(&txn)
        .await
        .map_err(|e| format!("清空供应商模型失败：{e}"))?;
    provider::Entity::delete_many()
        .exec(&txn)
        .await
        .map_err(|e| format!("清空供应商失败：{e}"))?;
    // 11-21：用量缓存随供应商一并清空（与删除供应商路径成对失效；不清会留下
    // 指向已消失供应商的孤儿缓存行）。
    crate::entity::usage_cache::Entity::delete_many()
        .exec(&txn)
        .await
        .map_err(|e| format!("清空用量缓存失败：{e}"))?;
    api_key::Entity::delete_many()
        .exec(&txn)
        .await
        .map_err(|e| format!("清空 API Key 失败：{e}"))?;

    let mut summary = ImportSummary::default();

    // 供应商：name → 新 id。
    let mut provider_ids: std::collections::HashMap<String, i32> = std::collections::HashMap::new();
    for p in &file.providers {
        let now = chrono::Utc::now();
        let row = provider::ActiveModel {
            name: Set(p.name.trim().to_string()),
            enable: Set(p.enable),
            // 停用原因镜像不变式（ADR-0003）：enable ⇔ disabled_reason。
            disabled_reason: Set(p.disabled_reason.clone()),
            base_url: Set(p.base_url.trim().to_string()),
            api_key: Set(crypto::encrypt(&p.api_key)),
            custom_header: Set(p.custom_header.clone()),
            extra: Set(crypto::encrypt(&p.extra)),
            protocol_type: Set(p.protocol_type),
            billing_mode: Set(p.billing_mode),
            sort_order: Set(p.sort_order),
            proxy_enabled: Set(p.proxy_enabled),
            proxy_addr: Set(p.proxy_addr.trim().to_string()),
            created_at: Set(now),
            updated_at: Set(now),
            ..Default::default()
        };
        let model = row
            .insert(&txn)
            .await
            .map_err(|e| format!("导入供应商 {} 失败：{e}", p.name))?;
        provider_ids.insert(p.name.trim().to_string(), model.id);
        summary.providers += 1;
    }

    // 供应商模型：(provider_id, provider_model_id) → 新 model_id。
    let mut model_ids: std::collections::HashMap<(i32, String), i32> =
        std::collections::HashMap::new();
    for p in &file.providers {
        let Some(&provider_id) = provider_ids.get(p.name.trim()) else {
            continue;
        };
        for m in &p.models {
            let now = chrono::Utc::now();
            let row = provider_model::ActiveModel {
                provider_id: Set(provider_id),
                provider_model_id: Set(m.provider_model_id.trim().to_string()),
                context_length: Set(m.context_length),
                max_output_tokens: Set(m.max_output_tokens),
                reasoning: Set(m.reasoning),
                tool_use: Set(m.tool_use),
                image_understand: Set(m.image_understand),
                video_understand: Set(m.video_understand),
                protocol_type: Set(m.protocol_type),
                proxy_enabled: Set(m.proxy_enabled),
                proxy_addr: Set(m.proxy_addr.trim().to_string()),
                created_at: Set(now),
                updated_at: Set(now),
                ..Default::default()
            };
            let model = row.insert(&txn).await.map_err(|e| {
                format!(
                    "导入供应商模型 {}/{} 失败：{e}",
                    p.name, m.provider_model_id
                )
            })?;
            model_ids.insert(
                (provider_id, m.provider_model_id.trim().to_string()),
                model.model_id,
            );
            summary.models += 1;
        }
    }

    // 虚拟模型：display_id → 新 id。
    let mut vm_ids: std::collections::HashMap<String, i32> = std::collections::HashMap::new();
    for vm in &file.virtual_models {
        let now = chrono::Utc::now();
        let row = virtual_model::ActiveModel {
            display_id: Set(vm.display_id.trim().to_string()),
            enable: Set(vm.enable),
            load_balancing_strategy: Set(vm.load_balancing_strategy),
            fallback_strategy: Set(vm.fallback_strategy),
            interface_type: Set(vm.interface_type),
            created_at: Set(now),
            updated_at: Set(now),
            ..Default::default()
        };
        let model = row
            .insert(&txn)
            .await
            .map_err(|e| format!("导入虚拟模型 {} 失败：{e}", vm.display_id))?;
        vm_ids.insert(vm.display_id.trim().to_string(), model.virtual_model_id);
        summary.virtual_models += 1;
    }

    // 成员：解析 (providerName, providerModelId) → model_id。
    for vm in &file.virtual_models {
        let Some(&virtual_model_id) = vm_ids.get(vm.display_id.trim()) else {
            continue;
        };
        for it in &vm.items {
            let Some(&provider_id) = provider_ids.get(it.provider_name.trim()) else {
                continue;
            };
            let Some(&model_id) =
                model_ids.get(&(provider_id, it.provider_model_id.trim().to_string()))
            else {
                continue;
            };
            let now = chrono::Utc::now();
            virtual_model_item::ActiveModel {
                virtual_model_id: Set(virtual_model_id),
                model_id: Set(model_id),
                enable: Set(it.enable),
                cascade_disabled: Set(it.cascade_disabled),
                created_at: Set(now),
                updated_at: Set(now),
                ..Default::default()
            }
            .insert(&txn)
            .await
            .map_err(|e| {
                format!(
                    "导入成员 {}/{}（虚拟模型 {}）失败：{e}",
                    it.provider_name, it.provider_model_id, vm.display_id
                )
            })?;
        }
    }

    // API Key：重算哈希后加密入库。
    for k in &file.api_keys {
        let plain = k.key.trim().to_string();
        let now = chrono::Utc::now();
        api_key::ActiveModel {
            name: Set(k.name.trim().to_string()),
            key: Set(crypto::encrypt(&plain)),
            key_hash: Set(Some(hash_token(&plain))),
            enable: Set(k.enable),
            created_at: Set(now),
            updated_at: Set(now),
            ..Default::default()
        }
        .insert(&txn)
        .await
        .map_err(|e| format!("导入 API Key {} 失败：{e}", k.name))?;
        summary.api_keys += 1;
    }

    // 系统设置：upsert 覆盖，不删除（备份里没有的键保留当前值）。
    for s in &file.settings {
        let t = setting_type_from_name(&s.r#type)?;
        let existing = setting::Entity::find()
            .filter(setting::Column::Key.eq(s.key.trim()))
            .one(&txn)
            .await
            .map_err(|e| format!("查询设置 {} 失败：{e}", s.key))?;
        let now = chrono::Utc::now();
        if let Some(model) = existing {
            let mut active: setting::ActiveModel = model.into();
            active.value = Set(s.value.clone());
            active.r#type = Set(t);
            active.updated_at = Set(now);
            active
                .update(&txn)
                .await
                .map_err(|e| format!("更新设置 {} 失败：{e}", s.key))?;
        } else {
            setting::ActiveModel {
                key: Set(s.key.trim().to_string()),
                value: Set(s.value.clone()),
                r#type: Set(t),
                updated_at: Set(now),
            }
            .insert(&txn)
            .await
            .map_err(|e| format!("写入设置 {} 失败：{e}", s.key))?;
        }
        summary.settings += 1;
    }

    txn.commit()
        .await
        .map_err(|e| format!("提交事务失败：{e}"))?;

    Ok(summary)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_provider(name: &str) -> BackupProvider {
        BackupProvider {
            name: name.to_string(),
            enable: true,
            base_url: format!("https://{name}.example.com/v1"),
            api_key: "sk-plain".to_string(),
            custom_header: "{}".to_string(),
            protocol_type: 0,
            billing_mode: 0,
            extra: "{}".to_string(),
            sort_order: 0,
            proxy_enabled: false,
            proxy_addr: String::new(),
            disabled_reason: None,
            models: vec![BackupProviderModel {
                provider_model_id: "m-1".to_string(),
                context_length: 1000,
                max_output_tokens: 1000,
                reasoning: false,
                tool_use: true,
                image_understand: false,
                video_understand: false,
                protocol_type: None,
                proxy_enabled: false,
                proxy_addr: String::new(),
            }],
        }
    }

    fn sample_file() -> BackupFile {
        BackupFile {
            version: BACKUP_VERSION,
            decrypt_failures: 0,
            exported_at: "2026-09-07T00:00:00Z".to_string(),
            providers: vec![sample_provider("openai")],
            virtual_models: vec![BackupVirtualModel {
                display_id: "gpt-turbo".to_string(),
                enable: true,
                load_balancing_strategy: 0,
                fallback_strategy: 0,
                interface_type: 0,
                items: vec![BackupVirtualModelItem {
                    provider_name: "openai".to_string(),
                    provider_model_id: "m-1".to_string(),
                    enable: true,
                    cascade_disabled: false,
                }],
            }],
            api_keys: vec![BackupApiKey {
                name: "itest-key".to_string(),
                key: "lg-plaintext".to_string(),
                enable: true,
            }],
            settings: vec![BackupSetting {
                key: "site_name".to_string(),
                value: "gw".to_string(),
                r#type: "String".to_string(),
            }],
        }
    }

    #[test]
    fn parse_backup_rejects_invalid_json() {
        let err = parse_backup("not json").unwrap_err();
        assert!(err.contains("不是合法的 JSON"), "{err}");
    }

    #[test]
    fn parse_backup_rejects_non_object() {
        let err = parse_backup("[1,2,3]").unwrap_err();
        assert!(err.contains("顶层必须是 JSON 对象"), "{err}");
    }

    #[test]
    fn parse_backup_rejects_missing_version() {
        let err = parse_backup(r#"{"providers":[]}"#).unwrap_err();
        assert!(err.contains("version"), "{err}");
    }

    #[test]
    fn parse_backup_rejects_unsupported_version() {
        let input = serde_json::json!({
            "version": 99,
            "providers": [], "virtualModels": [], "apiKeys": [], "settings": []
        });
        let err = parse_backup(&input.to_string()).unwrap_err();
        assert!(err.contains("版本不支持"), "{err}");
    }

    #[test]
    fn parse_backup_rejects_missing_top_field() {
        let input = serde_json::json!({
            "version": 1,
            "providers": [], "virtualModels": [], "apiKeys": []
        });
        let err = parse_backup(&input.to_string()).unwrap_err();
        assert!(err.contains("settings"), "{err}");
    }

    #[test]
    fn parse_backup_accepts_wellformed() {
        let json = serde_json::to_string(&sample_file()).unwrap();
        let parsed = parse_backup(&json).unwrap();
        assert_eq!(parsed.version, BACKUP_VERSION);
        assert_eq!(parsed.providers.len(), 1);
        assert_eq!(parsed.virtual_models.len(), 1);
        assert_eq!(parsed.api_keys.len(), 1);
        assert_eq!(parsed.settings.len(), 1);
    }

    #[test]
    fn validate_backup_rejects_dup_provider_name() {
        let mut f = sample_file();
        f.providers.push(sample_provider("openai"));
        let err = validate_backup(&f).unwrap_err();
        assert!(err.contains("providers[1].name 重复"), "{err}");
    }

    #[test]
    fn validate_backup_rejects_empty_provider_name() {
        let mut f = sample_file();
        f.providers[0].name = "  ".to_string();
        let err = validate_backup(&f).unwrap_err();
        assert!(err.contains("providers[0].name 不能为空"), "{err}");
    }

    #[test]
    fn validate_backup_rejects_bad_protocol() {
        let mut f = sample_file();
        f.providers[0].protocol_type = 7;
        let err = validate_backup(&f).unwrap_err();
        assert!(err.contains("protocolType 取值非法"), "{err}");
    }

    #[test]
    fn validate_backup_rejects_model_ref_to_missing_model() {
        let mut f = sample_file();
        f.virtual_models[0].items[0].provider_model_id = "missing".to_string();
        let err = validate_backup(&f).unwrap_err();
        assert!(err.contains("引用的模型不存在"), "{err}");
    }

    #[test]
    fn validate_backup_rejects_dup_display_id() {
        let mut f = sample_file();
        f.virtual_models.push(f.virtual_models[0].clone());
        let err = validate_backup(&f).unwrap_err();
        assert!(err.contains("virtualModels[1].displayId 重复"), "{err}");
    }

    #[test]
    fn validate_backup_rejects_bad_interface_type() {
        let mut f = sample_file();
        f.virtual_models[0].interface_type = 9;
        let err = validate_backup(&f).unwrap_err();
        assert!(err.contains("interfaceType 取值非法"), "{err}");
    }

    #[test]
    fn validate_backup_rejects_bad_setting_type_name() {
        let mut f = sample_file();
        f.settings[0].r#type = "Yaml".to_string();
        let err = validate_backup(&f).unwrap_err();
        assert!(err.contains("设置类型"), "{err}");
    }

    #[test]
    fn validate_backup_accepts_wellformed() {
        assert!(validate_backup(&sample_file()).is_ok());
    }

    #[test]
    fn validate_backup_rejects_member_protocol_mismatch() {
        // 虚拟模型接口类型 2（Anthropic），成员模型协议 0（OpenAI Compat）→ 不匹配。
        let mut f = sample_file();
        f.virtual_models[0].interface_type = 2;
        let err = validate_backup(&f).unwrap_err();
        assert!(err.contains("成员协议与虚拟模型接口类型不匹配"), "{err}");
    }

    #[test]
    fn validate_backup_accepts_member_protocol_via_provider() {
        // 成员模型无模型级协议，但供应商协议为 0 → 生效协议 0，接口类型 0 匹配。
        let mut f = sample_file();
        f.virtual_models[0].items[0].provider_model_id = "m-1".to_string();
        assert!(validate_backup(&f).is_ok());
    }

    #[test]
    fn validate_backup_full_compatible_accepts_any_protocol() {
        // Full Compatible（4）豁免协议匹配。
        let mut f = sample_file();
        f.virtual_models[0].interface_type = 4;
        f.providers[0].protocol_type = 3; // Gemini 成员
        assert!(validate_backup(&f).is_ok());
    }
    /// 15-03：同次导入内的成员重复在校验阶段被拒（不再撞数据库唯一索引报裸 SQL）。
    #[test]
    fn validate_backup_rejects_duplicate_member() {
        let mut file = sample_file();
        let first = file.virtual_models[0].items[0].clone();
        file.virtual_models[0].items.push(first);
        let err = validate_backup(&file).unwrap_err();
        assert!(err.contains("成员重复"), "{err}");
    }

    /// 15-06：未知设置类型编号导出时不再降级为 String（保真）。
    #[test]
    fn setting_type_name_keeps_unknown_number() {
        assert_eq!(setting_type_name(0), "String");
        assert_eq!(
            setting_type_name(99),
            "99",
            "未知编号应原样回带，不再降级 String"
        );
    }

    /// 15-05：settings 导出按 key 排序（与其余五表一致，保证导出可逐字节比对）。
    #[tokio::test]
    async fn export_sorts_settings_by_key() {
        let db = crate::db::connect("sqlite::memory:").await.unwrap();
        let now = chrono::Utc::now();
        for (key, value) in [("zz_last", "1"), ("aa_first", "2"), ("mm_middle", "3")] {
            setting::ActiveModel {
                key: sea_orm::Set(key.to_string()),
                value: sea_orm::Set(value.to_string()),
                r#type: sea_orm::Set(0),
                updated_at: sea_orm::Set(now),
            }
            .insert(&db)
            .await
            .unwrap();
        }
        let file = build_export(&db).await.unwrap();
        let keys: Vec<&str> = file.settings.iter().map(|s| s.key.as_str()).collect();
        let mut sorted = keys.clone();
        sorted.sort();
        assert_eq!(keys, sorted, "settings 应按 key 排序导出");
    }

    /// 15-04：导出解密失败计数（密钥不匹配时凭据置空并计数）。
    #[tokio::test]
    async fn export_counts_decrypt_failures() {
        let db = crate::db::connect("sqlite::memory:").await.unwrap();
        let now = chrono::Utc::now();
        // 直接用「带前缀但非法」的密文模拟密钥轮换后的不可解凭据。
        provider::ActiveModel {
            name: sea_orm::Set("p-broken".to_string()),
            enable: sea_orm::Set(true),
            base_url: sea_orm::Set("https://a.example".to_string()),
            api_key: sea_orm::Set("enc:v1:broken-not-decryptable".to_string()),
            custom_header: sea_orm::Set("{}".to_string()),
            protocol_type: sea_orm::Set(0),
            billing_mode: sea_orm::Set(0),
            extra: sea_orm::Set("enc:v1:broken-not-decryptable".to_string()),
            sort_order: sea_orm::Set(0),
            proxy_enabled: sea_orm::Set(false),
            proxy_addr: sea_orm::Set(String::new()),
            created_at: sea_orm::Set(now),
            updated_at: sea_orm::Set(now),
            ..Default::default()
        }
        .insert(&db)
        .await
        .unwrap();

        let file = build_export(&db).await.unwrap();
        assert_eq!(file.decrypt_failures, 2, "api_key + extra 两处失败应计数");
        assert!(file.providers[0].api_key.is_empty(), "不可解凭据应置空");
    }
}
