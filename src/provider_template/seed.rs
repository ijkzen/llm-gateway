//! 由 models.dev/api.json（2026-08-29 抓取）+ coding-plan-usage-apis.md 生成。
//! provider 模板种子数据：协议类型、付费模式按调研标注，
//! extra 为用量获取所需字段：cookie 类/OAuth 类/AK-SK 类，
//! 以及 usage（是否支持用量查询）与 usage_type（0=余额，1=月剩余额度/百分比）。
//! 规则：api.json 中无 base_url 的 provider 不收录；Command Code 为手动补充。

pub struct Template {
    pub name: &'static str,
    pub base_url: &'static str,
    pub protocol_type: i32,
    pub billing_mode: i32,
    pub extra: &'static str,
}

mod chunk_1;
mod chunk_2;
mod chunk_3;
mod chunk_4;

/// 全部模板条目（顺序与原数据文件一致）。
pub fn all() -> impl Iterator<Item = &'static Template> {
    chunk_1::TEMPLATES
        .iter()
        .chain(chunk_2::TEMPLATES)
        .chain(chunk_3::TEMPLATES)
        .chain(chunk_4::TEMPLATES)
}
