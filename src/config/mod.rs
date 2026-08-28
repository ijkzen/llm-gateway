#[derive(Debug, Clone)]
pub enum RuntimeEnv {
    Dev,
    Prod,
}

impl RuntimeEnv {
    pub fn log_dir(&self) -> &'static str {
        match self {
            RuntimeEnv::Prod => "/config/logs",
            RuntimeEnv::Dev => "logs",
        }
    }
}

impl std::str::FromStr for RuntimeEnv {
    type Err = ();
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_lowercase().as_str() {
            "dev" => Ok(RuntimeEnv::Dev),
            "prod" => Ok(RuntimeEnv::Prod),
            _ => Err(()),
        }
    }
}

#[derive(Debug, Clone)]
pub struct Config {
    pub bind_address: String,
    pub env: RuntimeEnv,
    pub database_url: String,
    pub cron_job_queue_size: usize,
    pub cron_job_max_concurrent: usize,
    /// API 密钥加密密钥;为空时敏感字段以明文存储(开发环境)。
    pub api_key_encryption_key: Option<String>,
}

impl Config {
    pub fn from_env() -> anyhow::Result<Self> {
        let bind_address =
            std::env::var("BIND_ADDRESS").unwrap_or_else(|_| "0.0.0.0:4007".to_string());
        let env = std::env::var("APP_ENV")
            .ok()
            .and_then(|s| s.parse().ok())
            .unwrap_or(RuntimeEnv::Dev);
        let database_url = std::env::var("DATABASE_URL").unwrap_or_else(|_| match env {
            RuntimeEnv::Prod => "sqlite:///config/db/app.db?mode=rwc".to_string(),
            RuntimeEnv::Dev => "sqlite://db/app.db?mode=rwc".to_string(),
        });
        let cron_job_queue_size = parse_positive_usize_env("CRON_JOB_QUEUE_SIZE", 1000)?;
        let cron_job_max_concurrent = parse_positive_usize_env("CRON_JOB_MAX_CONCURRENT", 10)?;
        let api_key_encryption_key = std::env::var("API_KEY_ENCRYPTION_KEY")
            .ok()
            .map(|k| k.trim().to_string())
            .filter(|k| !k.is_empty());
        Ok(Self {
            bind_address,
            env,
            database_url,
            cron_job_queue_size,
            cron_job_max_concurrent,
            api_key_encryption_key,
        })
    }
}

fn parse_positive_usize_env(name: &str, default: usize) -> anyhow::Result<usize> {
    match std::env::var(name) {
        Ok(value) => {
            let size: usize = value.parse().map_err(|_| {
                anyhow::anyhow!("{name} must be a valid positive integer, got '{value}'")
            })?;
            if size == 0 {
                return Err(anyhow::anyhow!(
                    "{name} must be greater than 0, got '{value}'"
                ));
            }
            Ok(size)
        }
        Err(_) => Ok(default),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_config_defaults() {
        temp_env::with_vars(
            [
                ("BIND_ADDRESS", None::<&str>),
                ("APP_ENV", None::<&str>),
                ("DATABASE_URL", None::<&str>),
                ("CRON_JOB_QUEUE_SIZE", None::<&str>),
                ("CRON_JOB_MAX_CONCURRENT", None::<&str>),
            ],
            || {
                let config = Config::from_env().unwrap();
                assert_eq!(config.bind_address, "0.0.0.0:4007");
                assert!(matches!(config.env, RuntimeEnv::Dev));
                assert_eq!(config.database_url, "sqlite://db/app.db?mode=rwc");
                assert_eq!(config.cron_job_queue_size, 1000);
                assert_eq!(config.cron_job_max_concurrent, 10);
            },
        );
    }

    #[test]
    fn test_config_custom_cron_job_max_concurrent() {
        temp_env::with_vars(
            [
                ("BIND_ADDRESS", None::<&str>),
                ("APP_ENV", None::<&str>),
                ("DATABASE_URL", None::<&str>),
                ("CRON_JOB_QUEUE_SIZE", None::<&str>),
                ("CRON_JOB_MAX_CONCURRENT", Some("20")),
            ],
            || {
                let config = Config::from_env().unwrap();
                assert_eq!(config.cron_job_max_concurrent, 20);
            },
        );
    }

    #[test]
    fn test_config_invalid_cron_job_max_concurrent_returns_error() {
        temp_env::with_vars(
            [
                ("BIND_ADDRESS", None::<&str>),
                ("APP_ENV", None::<&str>),
                ("DATABASE_URL", None::<&str>),
                ("CRON_JOB_QUEUE_SIZE", None::<&str>),
                ("CRON_JOB_MAX_CONCURRENT", Some("invalid")),
            ],
            || {
                let result = Config::from_env();
                assert!(result.is_err());
                let err = result.unwrap_err().to_string();
                assert!(err.contains("CRON_JOB_MAX_CONCURRENT"));
            },
        );
    }

    #[test]
    fn test_config_zero_cron_job_max_concurrent_returns_error() {
        temp_env::with_vars(
            [
                ("BIND_ADDRESS", None::<&str>),
                ("APP_ENV", None::<&str>),
                ("DATABASE_URL", None::<&str>),
                ("CRON_JOB_QUEUE_SIZE", None::<&str>),
                ("CRON_JOB_MAX_CONCURRENT", Some("0")),
            ],
            || {
                let result = Config::from_env();
                assert!(result.is_err());
                let err = result.unwrap_err().to_string();
                assert!(err.contains("CRON_JOB_MAX_CONCURRENT"));
                assert!(err.contains("greater than 0"));
            },
        );
    }

    #[test]
    fn test_config_custom_cron_job_queue_size() {
        temp_env::with_vars(
            [
                ("BIND_ADDRESS", None::<&str>),
                ("APP_ENV", None::<&str>),
                ("DATABASE_URL", None::<&str>),
                ("CRON_JOB_QUEUE_SIZE", Some("500")),
                ("CRON_JOB_MAX_CONCURRENT", None::<&str>),
            ],
            || {
                let config = Config::from_env().unwrap();
                assert_eq!(config.cron_job_queue_size, 500);
            },
        );
    }

    #[test]
    fn test_config_invalid_cron_job_queue_size_returns_error() {
        temp_env::with_vars(
            [
                ("BIND_ADDRESS", None::<&str>),
                ("APP_ENV", None::<&str>),
                ("DATABASE_URL", None::<&str>),
                ("CRON_JOB_QUEUE_SIZE", Some("invalid")),
                ("CRON_JOB_MAX_CONCURRENT", None::<&str>),
            ],
            || {
                let result = Config::from_env();
                assert!(result.is_err());
                let err = result.unwrap_err().to_string();
                assert!(err.contains("CRON_JOB_QUEUE_SIZE"));
            },
        );
    }

    #[test]
    fn test_config_zero_cron_job_queue_size_returns_error() {
        temp_env::with_vars(
            [
                ("BIND_ADDRESS", None::<&str>),
                ("APP_ENV", None::<&str>),
                ("DATABASE_URL", None::<&str>),
                ("CRON_JOB_QUEUE_SIZE", Some("0")),
                ("CRON_JOB_MAX_CONCURRENT", None::<&str>),
            ],
            || {
                let result = Config::from_env();
                assert!(result.is_err());
                let err = result.unwrap_err().to_string();
                assert!(err.contains("CRON_JOB_QUEUE_SIZE"));
                assert!(err.contains("greater than 0"));
            },
        );
    }

    #[test]
    fn test_config_prod_env() {
        temp_env::with_vars(
            [
                ("BIND_ADDRESS", None::<&str>),
                ("APP_ENV", Some("prod")),
                ("DATABASE_URL", None::<&str>),
                ("CRON_JOB_QUEUE_SIZE", None::<&str>),
                ("CRON_JOB_MAX_CONCURRENT", None::<&str>),
            ],
            || {
                let config = Config::from_env().unwrap();
                assert!(matches!(config.env, RuntimeEnv::Prod));
                assert_eq!(config.database_url, "sqlite:///config/db/app.db?mode=rwc");
            },
        );
    }

    #[test]
    fn test_runtime_env_log_dir() {
        temp_env::with_vars(
            [
                ("BIND_ADDRESS", None::<&str>),
                ("APP_ENV", Some("prod")),
                ("DATABASE_URL", None::<&str>),
                ("CRON_JOB_QUEUE_SIZE", None::<&str>),
                ("CRON_JOB_MAX_CONCURRENT", None::<&str>),
            ],
            || {
                let config = Config::from_env().unwrap();
                assert_eq!(config.env.log_dir(), "/config/logs");
            },
        );
        temp_env::with_vars(
            [
                ("BIND_ADDRESS", None::<&str>),
                ("APP_ENV", None::<&str>),
                ("DATABASE_URL", None::<&str>),
                ("CRON_JOB_QUEUE_SIZE", None::<&str>),
                ("CRON_JOB_MAX_CONCURRENT", None::<&str>),
            ],
            || {
                let config = Config::from_env().unwrap();
                assert_eq!(config.env.log_dir(), "logs");
            },
        );
    }

    #[test]
    fn test_config_custom_bind_address() {
        temp_env::with_vars(
            [
                ("BIND_ADDRESS", Some("127.0.0.1:3000")),
                ("APP_ENV", None::<&str>),
                ("DATABASE_URL", None::<&str>),
                ("CRON_JOB_QUEUE_SIZE", None::<&str>),
                ("CRON_JOB_MAX_CONCURRENT", None::<&str>),
            ],
            || {
                let config = Config::from_env().unwrap();
                assert_eq!(config.bind_address, "127.0.0.1:3000");
            },
        );
    }

    #[test]
    fn test_config_invalid_app_env_defaults_to_dev() {
        temp_env::with_vars(
            [
                ("BIND_ADDRESS", None::<&str>),
                ("APP_ENV", Some("invalid_env_value")),
                ("DATABASE_URL", None::<&str>),
                ("CRON_JOB_QUEUE_SIZE", None::<&str>),
                ("CRON_JOB_MAX_CONCURRENT", None::<&str>),
            ],
            || {
                let config = Config::from_env().unwrap();
                assert!(matches!(config.env, RuntimeEnv::Dev));
            },
        );
    }

    #[test]
    fn test_config_custom_database_url() {
        temp_env::with_vars(
            [
                ("BIND_ADDRESS", None::<&str>),
                ("APP_ENV", None::<&str>),
                ("DATABASE_URL", Some("sqlite:///tmp/test.db?mode=rwc")),
                ("CRON_JOB_QUEUE_SIZE", None::<&str>),
                ("CRON_JOB_MAX_CONCURRENT", None::<&str>),
            ],
            || {
                let config = Config::from_env().unwrap();
                assert_eq!(config.database_url, "sqlite:///tmp/test.db?mode=rwc");
            },
        );
    }
}
