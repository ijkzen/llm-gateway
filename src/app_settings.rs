//! 语言/时区设置项的进程内缓存与种子。
//!
//! 设置项存于 `setting` 表（`language` / `timezone` 两行）。启动时从数据库
//! 加载进内存，`PUT /api/settings/{key}` 更新后同步刷新。时区用于定时任务的
//! cron 语义（见 `src/cron/`），语言用于 API 消息本地化（见 `src/i18n.rs`）。

use std::str::FromStr;
use std::sync::Arc;

use sea_orm::{ActiveModelTrait, ColumnTrait, DatabaseConnection, EntityTrait, QueryFilter, Set};
use tokio::sync::RwLock;

use crate::entity::setting::{self, SettingType};
use crate::i18n::Lang;

/// 设置项 key：管理后台界面语言（`zh-CN` / `en`）。
pub const KEY_LANGUAGE: &str = "language";
/// 设置项 key：定时任务 cron 语义时区（IANA 名称，如 `Asia/Shanghai`）。
/// 未设置或非法时回退到服务器本地时区（`chrono::Local`）。
pub const KEY_TIMEZONE: &str = "timezone";

/// 种子的默认时区：与生产容器 `TZ=Asia/Shanghai` 语义一致。
/// 引导页初始化后会用浏览器时区覆盖该值。
pub const DEFAULT_TIMEZONE: &str = "Asia/Shanghai";

#[derive(Clone, Debug)]
struct AppSettingsInner {
    language: Lang,
    /// 定时任务 cron 语义时区；`None` 表示服务器本地时区。
    timezone: Option<chrono_tz::Tz>,
}

/// 语言/时区设置的可克隆句柄。内部用 `RwLock` 支持运行时热更新
/// （`PUT /api/settings/{key}` 后无需重启进程）。
#[derive(Clone)]
pub struct AppSettings {
    inner: Arc<RwLock<AppSettingsInner>>,
}

impl Default for AppSettings {
    fn default() -> Self {
        Self {
            inner: Arc::new(RwLock::new(AppSettingsInner {
                language: Lang::default(),
                timezone: None,
            })),
        }
    }
}

impl AppSettings {
    /// 最近一次 [`AppSettings::update`] 写入的全局句柄（中间件等无 State
    /// 上下文的场景读取当前语言用）。未设置时返回 `None`，调用方按默认语言
    /// 处理即可。
    pub fn process_global() -> Option<Self> {
        PROCESS_GLOBAL.get().cloned()
    }

    /// 用 [`AppSettings::load_from_db`] 得到的实例登记为进程全局。
    /// 仅在首次调用时生效（重复调用被忽略）。
    pub fn set_process_global(settings: Self) {
        let _ = PROCESS_GLOBAL.set(settings);
    }
}

static PROCESS_GLOBAL: std::sync::OnceLock<AppSettings> = std::sync::OnceLock::new();

impl AppSettings {
    /// 从 setting 表加载 language / timezone 两行（缺失时用默认值），并幂等
    /// 写入种子行，保证「空表起步」的库也有这两行可被 `PUT` 更新。
    pub async fn load_from_db(db: &DatabaseConnection) -> anyhow::Result<Self> {
        let mut language = Lang::default();
        let mut timezone: Option<chrono_tz::Tz> = None;

        for model in setting::Entity::find().all(db).await? {
            match model.key.as_str() {
                KEY_LANGUAGE => {
                    if let Ok(lang) = Lang::from_str(&model.value) {
                        language = lang;
                    }
                }
                KEY_TIMEZONE => {
                    timezone = chrono_tz::Tz::from_str(model.value.trim()).ok();
                }
                _ => {}
            }
        }

        *LANG_SYNC.lock().unwrap() = language;

        let settings = Self {
            inner: Arc::new(RwLock::new(AppSettingsInner { language, timezone })),
        };
        settings.ensure_seed_rows(db).await?;
        Ok(settings)
    }

    /// 幂等插入 language / timezone 种子行（已存在则跳过）。
    async fn ensure_seed_rows(&self, db: &DatabaseConnection) -> anyhow::Result<()> {
        for (key, value, setting_type) in [
            (
                KEY_LANGUAGE,
                Lang::default().to_string(),
                SettingType::String,
            ),
            (
                KEY_TIMEZONE,
                DEFAULT_TIMEZONE.to_string(),
                SettingType::String,
            ),
        ] {
            let exists = setting::Entity::find()
                .filter(setting::Column::Key.eq(key))
                .one(db)
                .await?;
            if exists.is_some() {
                continue;
            }
            setting::ActiveModel {
                key: Set(key.to_string()),
                value: Set(value),
                r#type: Set(setting_type as i32),
                updated_at: Set(chrono::Utc::now()),
            }
            .insert(db)
            .await?;
        }
        Ok(())
    }

    /// 当前管理后台语言。
    pub async fn lang(&self) -> Lang {
        self.inner.read().await.language
    }

    /// 当前定时任务 cron 语义时区；`None` 表示服务器本地时区。
    pub async fn timezone(&self) -> Option<chrono_tz::Tz> {
        self.inner.read().await.timezone
    }

    /// 更新设置（由 `PUT /api/settings/{key}` 调用）。`timezone` 值非法时
    /// 保持原值（校验已在上层拒绝非法输入，这里只是防御性处理）。
    pub async fn update(&self, key: &str, value: &str) {
        let mut inner = self.inner.write().await;
        match key {
            KEY_LANGUAGE => {
                if let Ok(lang) = Lang::from_str(value) {
                    inner.language = lang;
                    *LANG_SYNC.lock().unwrap() = lang;
                }
            }
            KEY_TIMEZONE => {
                inner.timezone = chrono_tz::Tz::from_str(value.trim()).ok();
            }
            _ => {}
        }
    }

    /// 同步读取当前语言（panic 中间件等无 async 上下文的场景）。
    /// 进程内只可能有单一语言设置，静态缓存与 `inner` 保持一致。
    pub fn lang_sync() -> Lang {
        *LANG_SYNC.lock().unwrap()
    }
}

static LANG_SYNC: std::sync::Mutex<Lang> = std::sync::Mutex::new(Lang::Zh);
