//! 内嵌的模型目录（models.dev models.json）与尾段匹配。
//!
//! 数据以 minified JSON vendor 在 `data/models.json`，编译期打包进二进制
//! （见 docs/adr/0001），首次访问时解析并建立「尾段 → 条目」索引。

use std::collections::{BTreeMap, HashMap};
use std::sync::OnceLock;

use serde::Deserialize;

const MODELS_JSON: &str = include_str!("data/models.json");

/// 目录中单个模型的元数据（只保留智能填充所需字段）。
#[derive(Debug, Clone, PartialEq)]
pub struct CatalogEntry {
    pub context_length: Option<i64>,
    pub max_output_tokens: Option<i64>,
    pub reasoning: bool,
    pub tool_use: bool,
    pub image_understand: bool,
    pub video_understand: bool,
}

/// 关键词搜索返回的候选（目录条目 + 展示用字段）。
#[derive(Debug, Clone)]
pub struct CatalogCandidate {
    pub id: String,
    pub name: String,
    pub family: String,
    pub entry: CatalogEntry,
}

impl CatalogEntry {
    /// 上下文与最大输出是否齐全（不齐全的命中记「信息不完整」）。
    pub fn is_complete(&self) -> bool {
        self.context_length.is_some() && self.max_output_tokens.is_some()
    }
}

#[derive(Deserialize)]
struct RawModel {
    #[serde(default)]
    reasoning: bool,
    #[serde(default)]
    tool_call: bool,
    #[serde(default)]
    modalities: Option<RawModalities>,
    #[serde(default)]
    limit: Option<RawLimit>,
}

/// 原始目录条目（含关键词搜索所需的展示字段）。
#[derive(Deserialize)]
struct RawModelFull {
    #[serde(default)]
    name: Option<String>,
    #[serde(default)]
    description: Option<String>,
    #[serde(default)]
    family: Option<String>,
    reasoning: bool,
    tool_call: bool,
    modalities: Option<RawModalities>,
    limit: Option<RawLimit>,
}

#[derive(Deserialize)]
struct RawModalities {
    #[serde(default)]
    input: Vec<String>,
}

#[derive(Deserialize)]
struct RawLimit {
    context: Option<i64>,
    output: Option<i64>,
}

/// 尾段索引：key 为目录条目 key 的最后一段（小写）。
/// 顶层 key 排序后先到先得，保证未来出现同尾段冲突时结果确定。
static CATALOG: OnceLock<HashMap<String, CatalogEntry>> = OnceLock::new();

/// 原始目录（搜索用）：key → 展示字段。懒加载。
static RAW: OnceLock<Vec<(String, RawModelFull)>> = OnceLock::new();

/// 从原始目录模型字段构造 CatalogEntry（catalog() 与 search() 共用同一映射）。
fn entry_from(
    limit: Option<&RawLimit>,
    modalities: &[String],
    reasoning: bool,
    tool_call: bool,
) -> CatalogEntry {
    CatalogEntry {
        context_length: limit.and_then(|l| l.context),
        max_output_tokens: limit.and_then(|l| l.output),
        reasoning,
        tool_use: tool_call,
        image_understand: modalities.iter().any(|s| s == "image"),
        video_understand: modalities.iter().any(|s| s == "video"),
    }
}

fn catalog() -> &'static HashMap<String, CatalogEntry> {
    CATALOG.get_or_init(|| {
        let raw: BTreeMap<String, RawModel> =
            serde_json::from_str(MODELS_JSON).expect("embedded models.json must be valid JSON");
        let mut index = HashMap::with_capacity(raw.len());
        for (key, model) in raw {
            let modalities = model.modalities.map(|m| m.input).unwrap_or_default();
            let entry = entry_from(
                model.limit.as_ref(),
                &modalities,
                model.reasoning,
                model.tool_call,
            );
            index
                .entry(last_segment(&key).to_lowercase())
                .or_insert(entry);
        }
        index
    })
}

fn raw() -> &'static [(String, RawModelFull)] {
    RAW.get_or_init(|| {
        let raw: BTreeMap<String, RawModelFull> =
            serde_json::from_str(MODELS_JSON).expect("embedded models.json must be valid JSON");
        raw.into_iter().collect()
    })
}

/// 关键词搜索目录：匹配 id/name/family/description（小写子串），
/// 按匹配位置（id 前缀 > id 子串 > name > 其他）排序，返回前 `limit` 条。
/// 关键词为空返回空列表。
pub fn search(q: &str, limit: usize) -> Vec<CatalogCandidate> {
    let query = q.trim().to_lowercase();
    if query.is_empty() {
        return Vec::new();
    }
    let mut scored: Vec<(u8, CatalogCandidate)> = raw()
        .iter()
        .filter_map(|(key, model)| {
            let id = key.clone();
            let name = model.name.clone().unwrap_or_default();
            let family = model.family.clone().unwrap_or_default();
            let description = model.description.clone().unwrap_or_default();
            let id_lower = id.to_lowercase();
            let score = if id_lower.starts_with(&query) {
                0
            } else if id_lower.contains(&query) {
                1
            } else if name.to_lowercase().contains(&query) {
                2
            } else if family.to_lowercase().contains(&query)
                || description.to_lowercase().contains(&query)
            {
                3
            } else {
                return None;
            };
            let modalities = model
                .modalities
                .as_ref()
                .map(|m| m.input.clone())
                .unwrap_or_default();
            Some((
                score,
                CatalogCandidate {
                    id,
                    name,
                    family,
                    entry: entry_from(
                        model.limit.as_ref(),
                        &modalities,
                        model.reasoning,
                        model.tool_call,
                    ),
                },
            ))
        })
        .collect::<Vec<_>>();

    scored.sort_by(|a, b| a.0.cmp(&b.0).then_with(|| a.1.id.cmp(&b.1.id)));
    scored.into_iter().take(limit).map(|(_, c)| c).collect()
}

/// 目录条目总数（测试与可观测性用）。
pub fn entry_count() -> usize {
    catalog().len()
}

/// 尾段匹配：两边模型 ID 各按 `/` 分割取最后一段，忽略大小写精确相等。
pub fn find_by_suffix(model_id: &str) -> Option<&'static CatalogEntry> {
    catalog().get(&last_segment(model_id).to_lowercase())
}

/// 取 `/` 分割后的最后一段。
fn last_segment(value: &str) -> &str {
    value.rsplit('/').next().unwrap_or(value)
}

/// 待确认候选的相似度阈值：归一化后字符相似度须超过 50%。
const SIMILARITY_THRESHOLD: f64 = 0.5;

/// 归一化：去掉 `/`、`-`、`_`、`.` 后转小写（相似度匹配用）。
fn normalize_for_similarity(value: &str) -> String {
    value
        .chars()
        .filter(|c| !matches!(c, '/' | '-' | '_' | '.'))
        .flat_map(char::to_lowercase)
        .collect()
}

/// Levenshtein 编辑距离（滚动数组，输入为归一化后的短串）。
fn levenshtein(a: &[char], b: &[char]) -> usize {
    let (long, short) = if a.len() >= b.len() { (a, b) } else { (b, a) };
    let mut prev: Vec<usize> = (0..=short.len()).collect();
    let mut curr: Vec<usize> = vec![0; short.len() + 1];
    for (i, lc) in long.iter().enumerate() {
        curr[0] = i + 1;
        for (j, sc) in short.iter().enumerate() {
            let cost = usize::from(lc != sc);
            curr[j + 1] = (prev[j + 1] + 1).min(curr[j] + 1).min(prev[j] + cost);
        }
        std::mem::swap(&mut prev, &mut curr);
    }
    prev[short.len()]
}

/// 字符相似度 = 1 - 编辑距离 / 较长串长度；任一为空返回 0。
fn similarity(a: &str, b: &str) -> f64 {
    let ac: Vec<char> = a.chars().collect();
    let bc: Vec<char> = b.chars().collect();
    let longest = ac.len().max(bc.len());
    if longest == 0 {
        return 0.0;
    }
    1.0 - levenshtein(&ac, &bc) as f64 / longest as f64
}

/// 待确认建议：目录 key + 参数条目。
pub type SimilarEntry = (String, CatalogEntry);

/// 目录中与 `model_id` 归一化后相似度超过 50% 的条目，
/// 按相似度降序、同分按 id 字典序取前 `limit` 条（待确认候选的建议来源）。
pub fn similar_entries(model_id: &str, limit: usize) -> Vec<SimilarEntry> {
    let target = normalize_for_similarity(model_id);
    if target.is_empty() {
        return Vec::new();
    }
    let target_len = target.chars().count();
    let mut scored: Vec<(f64, String, CatalogEntry)> = raw()
        .iter()
        .filter_map(|(key, model)| {
            let norm = normalize_for_similarity(key);
            let norm_len = norm.chars().count();
            let longest = target_len.max(norm_len);
            if longest == 0 {
                return None;
            }
            // 编辑距离相似度上界 = 1 - 长度差/较长长度，先用它粗筛再算距离。
            let bound = 1.0 - target_len.abs_diff(norm_len) as f64 / longest as f64;
            if bound <= SIMILARITY_THRESHOLD {
                return None;
            }
            let score = similarity(&target, &norm);
            if score <= SIMILARITY_THRESHOLD {
                return None;
            }
            let modalities = model
                .modalities
                .as_ref()
                .map(|m| m.input.clone())
                .unwrap_or_default();
            Some((
                score,
                key.clone(),
                entry_from(
                    model.limit.as_ref(),
                    &modalities,
                    model.reasoning,
                    model.tool_call,
                ),
            ))
        })
        .collect();
    scored.sort_by(|a, b| {
        b.0.partial_cmp(&a.0)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| a.1.cmp(&b.1))
    });
    scored
        .into_iter()
        .take(limit)
        .map(|(_, key, entry)| (key, entry))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_catalog_parses_all_entries() {
        // 2026-08-29 抓取的 models.json 共 363 条。
        assert_eq!(entry_count(), 363);
    }

    #[test]
    fn test_search_by_id_prefix_first() {
        // "gpt" 命中 id 的排在前面（如 openai/gpt-4o 的尾段）。
        let hits = search("gpt", 5);
        assert!(!hits.is_empty());
        assert!(hits.iter().any(|c| c.id.to_lowercase().contains("gpt")));
    }

    #[test]
    fn test_search_by_name_and_family() {
        // name 含 "claude" 的也能命中。
        let hits = search("claude", 10);
        assert!(!hits.is_empty());
        assert!(
            hits.iter()
                .any(|c| c.name.to_lowercase().contains("claude"))
        );
    }

    #[test]
    fn test_search_empty_or_miss() {
        assert!(search("", 5).is_empty());
        assert!(search("  ", 5).is_empty());
        assert!(search("zzzz-nothing-here", 5).is_empty());
    }

    #[test]
    fn test_search_respects_limit() {
        assert!(search("gpt", 3).len() <= 3);
    }

    #[test]
    fn test_find_by_suffix_matches_last_segment() {
        let entry = find_by_suffix("anthropic/claude-sonnet-4-5").expect("catalog hit");
        assert!(entry.context_length.is_some());
        assert!(entry.max_output_tokens.is_some());
        assert!(entry.is_complete());
    }

    #[test]
    fn test_find_by_suffix_ignores_case() {
        assert!(find_by_suffix("GPT-4O").is_some() || find_by_suffix("TENCENT/HY3").is_some());
    }

    #[test]
    fn test_find_by_suffix_handles_models_prefix() {
        // Gemini 远端返回 `models/gemini-2.5-flash` 这类 ID，取尾段后应能命中。
        let direct = find_by_suffix("gemini-2.5-flash");
        let prefixed = find_by_suffix("models/gemini-2.5-flash");
        assert_eq!(direct.is_some(), prefixed.is_some());
        if let Some(e) = prefixed {
            assert!(e.is_complete());
        }
    }

    #[test]
    fn test_find_by_suffix_miss_returns_none() {
        assert!(find_by_suffix("totally-unknown-model").is_none());
        assert!(find_by_suffix("").is_none());
    }

    #[test]
    fn test_incomplete_entries_exist() {
        // 363 条中有 8 条缺 limit，用于验证 partial 状态的判定来源。
        let incomplete = catalog()
            .values()
            .filter(|entry| !entry.is_complete())
            .count();
        assert_eq!(incomplete, 8);
    }

    #[test]
    fn test_normalize_strips_separators_and_lowercases() {
        assert_eq!(
            normalize_for_similarity("Anthropic/Claude-Sonnet_4.5"),
            "anthropicclaudesonnet45"
        );
        assert_eq!(normalize_for_similarity(""), "");
    }

    #[test]
    fn test_similarity_full_and_empty() {
        assert_eq!(similarity("abc", "abc"), 1.0);
        assert_eq!(similarity("", "abc"), 0.0);
        assert_eq!(similarity("", ""), 0.0);
    }

    #[test]
    fn test_similar_entries_hits_variant() {
        // 目录条目的近亲变体（多一段日期后缀）应命中且参数齐全。
        let hits = similar_entries("openai/gpt-4o-mini-2029-99-99", 3);
        assert!(!hits.is_empty(), "gpt-4o-mini 变体应命中目录");
        assert!(
            hits.iter()
                .any(|(id, entry)| id.ends_with("gpt-4o-mini") && entry.is_complete())
        );
    }

    #[test]
    fn test_similar_entries_desc_and_limit() {
        let hits = similar_entries("openai/gpt-4o-mini-suffix", 2);
        assert!(hits.len() <= 2);
        // 最相近的目录条目排第一（自身同尾段相似度最高）。
        if let Some((top, _)) = hits.first() {
            assert!(top.ends_with("gpt-4o-mini"));
        }
    }

    #[test]
    fn test_similar_entries_miss_returns_empty() {
        // 与目录条目均无 50% 以上相似度的 ID 返回空。
        assert!(similar_entries("qqqqwwwwzzzzxxxx", 3).is_empty());
        assert!(similar_entries("", 3).is_empty());
    }
}
