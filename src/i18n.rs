//! 管理后台界面语言（API 消息本地化）。
//!
//! 采用「调用点即翻译点」的轻量方案：调用方把中文原文与英文译文一并给出，
//! 由当前全局语言选择。默认 zh-CN 时输出与改造前完全一致（译文不参与），
//! 因此既有断言中文消息的测试在默认语言下无需改动。

use std::str::FromStr;

/// 管理后台支持的语言。与设置项 `language` 的取值一一对应。
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum Lang {
    /// zh-CN（默认）
    #[default]
    Zh,
    /// en
    En,
}

impl FromStr for Lang {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.trim() {
            "zh-CN" | "zh" => Ok(Lang::Zh),
            "en" | "en-US" => Ok(Lang::En),
            _ => Err(format!("unsupported language: {s}")),
        }
    }
}

impl std::fmt::Display for Lang {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Lang::Zh => write!(f, "zh-CN"),
            Lang::En => write!(f, "en"),
        }
    }
}

impl Lang {
    /// 按当前语言返回中/英文案。占位符（`{}`）保留，需要插值时由调用方
    /// 对返回值做 `format!`。
    pub fn tr(self, zh: &'static str, en: &'static str) -> &'static str {
        match self {
            Lang::Zh => zh,
            Lang::En => en,
        }
    }
}
