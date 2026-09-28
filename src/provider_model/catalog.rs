//! 模型目录（models.dev models.json）：外置文件优先、内嵌快照兜底。
//!
//! 内嵌数据以 minified JSON vendor 在 `data/models.json`，编译期打包进二进制
//! （见 docs/adr/0001）。匹配时优先读「外置模型目录」——db 目录父目录下的
//! `models.json`；按 mtime/size 判断是否需要重新解析，缺失/读不到/解析失败
//! 一律回退内嵌快照（ADR-0025）。

use std::collections::{BTreeMap, HashMap};
use std::path::{Path, PathBuf};
use std::sync::{Arc, OnceLock, RwLock};
use std::time::SystemTime;

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

/// 原始目录条目（含关键词搜索所需的展示字段）。14-09：唯一的反序列化目标
/// （`CatalogData` 的尾段索引与搜索列表都由它派生）。
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

/// 已解析的目录内容。
struct CatalogData {
    /// 尾段索引：key 为目录条目 key 的最后一段（小写）。
    /// 顶层 key 排序后先到先得，保证未来出现同尾段冲突时结果确定。
    index: HashMap<String, CatalogEntry>,
    /// 原始目录（搜索用）：key → 展示字段，按 key 排序。
    raw: Vec<(String, RawModelFull)>,
}

/// 观测到的目录来源：外置文件的 (mtime, size)，或内嵌快照。
#[derive(PartialEq, Eq)]
enum SourceKey {
    Embedded,
    External {
        modified: Option<SystemTime>,
        len: u64,
    },
}

/// 目录读取器：外置文件优先，缺失/读不到/解析失败回退内嵌快照（ADR-0025）。
///
/// 可实例化以便测试注入临时外置路径；生产用进程级全局实例（`init` 注入）。
pub struct Catalog {
    external: Option<PathBuf>,
    cached: RwLock<Option<(SourceKey, Arc<CatalogData>)>>,
}

impl Catalog {
    pub fn new(external: Option<PathBuf>) -> Self {
        Self {
            external,
            cached: RwLock::new(None),
        }
    }

    /// 仅用内嵌快照的目录（无外置覆盖）。
    pub fn embedded() -> Self {
        Self::new(None)
    }

    /// 外置目录文件路径（若配置）。
    pub fn external_path(&self) -> Option<PathBuf> {
        self.external.clone()
    }

    fn current_key(&self) -> SourceKey {
        if let Some(path) = &self.external
            && let Ok(meta) = std::fs::metadata(path)
        {
            return SourceKey::External {
                modified: meta.modified().ok(),
                len: meta.len(),
            };
        }
        SourceKey::Embedded
    }

    /// 确保缓存与当前来源一致；不一致则重新解析并替换。
    fn ensure(&self) {
        let key = self.current_key();
        if let Some((cached_key, _)) = self.cached.read().unwrap().as_ref()
            && *cached_key == key
        {
            return;
        }
        let data = self.load(&key);
        *self.cached.write().unwrap() = Some((key, data));
    }

    /// 解析当前来源的内容；外置读取/解析失败时回退内嵌，并把该来源键一并缓存
    /// （同一坏文件在变更前不再重复读取与告警）。
    fn load(&self, key: &SourceKey) -> Arc<CatalogData> {
        let embedded = || parse_data(MODELS_JSON).expect("embedded models.json must be valid JSON");
        match key {
            SourceKey::Embedded => embedded(),
            SourceKey::External { .. } => {
                let Some(path) = &self.external else {
                    return embedded();
                };
                match std::fs::read_to_string(path) {
                    Ok(text) => match parse_data(&text) {
                        Ok(data) => data,
                        Err(e) => {
                            tracing::warn!(path = %path.display(), "外置模型目录解析失败，回退内嵌快照：{e}");
                            embedded()
                        }
                    },
                    Err(e) => {
                        tracing::warn!(path = %path.display(), "外置模型目录读取失败，回退内嵌快照：{e}");
                        embedded()
                    }
                }
            }
        }
    }

    fn with_data<R>(&self, f: impl FnOnce(&CatalogData) -> R) -> R {
        self.ensure();
        let guard = self.cached.read().unwrap();
        let (_, data) = guard.as_ref().expect("catalog cache populated by ensure");
        f(data)
    }

    /// 目录条目总数。
    pub fn entry_count(&self) -> usize {
        self.with_data(|data| data.index.len())
    }

    /// 尾段匹配：两边模型 ID 各按 `/` 分割取最后一段，忽略大小写精确相等。
    pub fn find_by_suffix(&self, model_id: &str) -> Option<CatalogEntry> {
        self.with_data(|data| {
            data.index
                .get(&last_segment(model_id).to_lowercase())
                .cloned()
        })
    }

    /// 关键词搜索目录：匹配 id/name/family/description（小写子串），
    /// 按匹配位置（id 前缀 > id 子串 > name > 其他）排序，返回前 `limit` 条。
    /// 关键词为空返回空列表。
    pub fn search(&self, q: &str, limit: usize) -> Vec<CatalogCandidate> {
        let query = q.trim().to_lowercase();
        if query.is_empty() {
            return Vec::new();
        }
        let mut scored: Vec<(u8, CatalogCandidate)> = self.with_data(|data| {
            data.raw
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
                    Some((
                        score,
                        CatalogCandidate {
                            id,
                            name,
                            family,
                            entry: entry_from_model(model),
                        },
                    ))
                })
                .collect()
        });

        scored.sort_by(|a, b| a.0.cmp(&b.0).then_with(|| a.1.id.cmp(&b.1.id)));
        scored.into_iter().take(limit).map(|(_, c)| c).collect()
    }

    /// 目录中与 `model_id` 归一化后相似度超过 50% 的条目，
    /// 按相似度降序、同分按 id 字典序取前 `limit` 条（待确认候选的建议来源）。
    pub fn similar_entries(&self, model_id: &str, limit: usize) -> Vec<SimilarEntry> {
        let target = normalize_for_similarity(model_id);
        if target.is_empty() {
            return Vec::new();
        }
        let target_len = target.chars().count();
        let mut scored: Vec<(f64, String, CatalogEntry)> = self.with_data(|data| {
            data.raw
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
                    Some((score, key.clone(), entry_from_model(model)))
                })
                .collect()
        });
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
}

/// 进程级全局目录实例。
static GLOBAL: OnceLock<Catalog> = OnceLock::new();

/// 注入进程级全局目录的外置路径（启动期调用一次；重复调用忽略）。
pub fn init(external: Option<PathBuf>) {
    let _ = GLOBAL.set(Catalog::new(external));
}

fn global() -> &'static Catalog {
    GLOBAL.get_or_init(Catalog::embedded)
}

/// 进程级全局目录的外置文件路径（若配置）。
pub fn external_path() -> Option<PathBuf> {
    global().external_path()
}

/// 尾段匹配（进程级全局目录）。
pub fn find_by_suffix(model_id: &str) -> Option<CatalogEntry> {
    global().find_by_suffix(model_id)
}

/// 关键词搜索目录（进程级全局目录）。
pub fn search(q: &str, limit: usize) -> Vec<CatalogCandidate> {
    global().search(q, limit)
}

/// 待确认建议（进程级全局目录）。
pub fn similar_entries(model_id: &str, limit: usize) -> Vec<SimilarEntry> {
    global().similar_entries(model_id, limit)
}

/// 由 DATABASE_URL 推导外置目录文件路径：db 目录的父目录下的 `models.json`。
///
/// dev（`sqlite://db/app.db`）→ 工作目录下的 `models.json`；
/// prod（`sqlite:///config/db/app.db`）→ `/config/models.json`；
/// 内存库无文件路径 → `None`。
pub fn external_path_for_database_url(database_url: &str) -> Option<PathBuf> {
    let file = crate::db::sqlite_file_path(database_url)?;
    let db_dir = file.parent().unwrap_or_else(|| Path::new(""));
    let parent = db_dir.parent().unwrap_or_else(|| Path::new(""));
    Some(parent.join("models.json"))
}

/// 校验目录文本（刷新侧复用同一 serde 结构），返回条目数；空目录视为非法。
pub(crate) fn validate(text: &str) -> Result<usize, String> {
    let data = parse_data(text)?;
    Ok(data.raw.len())
}

/// 解析目录文本为索引 + 原始列表（内嵌与刷新校验共用）。
fn parse_data(text: &str) -> Result<Arc<CatalogData>, String> {
    let raw: BTreeMap<String, RawModelFull> =
        serde_json::from_str(text).map_err(|e| format!("目录 JSON 解析失败：{e}"))?;
    if raw.is_empty() {
        return Err("目录为空".to_string());
    }
    let mut index = HashMap::with_capacity(raw.len());
    let mut list = Vec::with_capacity(raw.len());
    for (key, model) in raw {
        let entry = entry_from_model(&model);
        index
            .entry(last_segment(&key).to_lowercase())
            .or_insert(entry);
        list.push((key, model));
    }
    Ok(Arc::new(CatalogData { index, raw: list }))
}

/// 从原始目录模型字段构造 CatalogEntry（索引与搜索共用同一映射）。
fn entry_from_model(model: &RawModelFull) -> CatalogEntry {
    let modalities = model
        .modalities
        .as_ref()
        .map(|m| m.input.clone())
        .unwrap_or_default();
    entry_from(
        model.limit.as_ref(),
        &modalities,
        model.reasoning,
        model.tool_call,
    )
}

/// 由拆解后的字段构造条目（`entry_from_model` 的底层实现）。
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

#[cfg(test)]
mod tests {
    use super::*;

    fn write_catalog(path: &Path, body: &str) {
        std::fs::write(path, body).unwrap();
    }

    /// 构造含单个 `gpt-4o` 尾段条目的外置目录文本（上下文与内嵌不同以便区分）。
    fn external_gpt4o(context: i64) -> String {
        format!(
            "{{\"acme/gpt-4o\":{{\"name\":\"X\",\"reasoning\":true,\"tool_call\":true,\
             \"limit\":{{\"context\":{context},\"output\":1}}}}}}"
        )
    }

    #[test]
    fn test_catalog_parses_all_entries() {
        // 2026-09-29 抓取的 models.json 共 432 条。
        assert_eq!(Catalog::embedded().entry_count(), 432);
    }

    #[test]
    fn test_search_by_id_prefix_first() {
        // "gpt" 命中 id 的排在前面（如 openai/gpt-4o 的尾段）。
        let hits = Catalog::embedded().search("gpt", 5);
        assert!(!hits.is_empty());
        assert!(hits.iter().any(|c| c.id.to_lowercase().contains("gpt")));
    }

    #[test]
    fn test_search_by_name_and_family() {
        // name 含 "claude" 的也能命中。
        let hits = Catalog::embedded().search("claude", 10);
        assert!(!hits.is_empty());
        assert!(
            hits.iter()
                .any(|c| c.name.to_lowercase().contains("claude"))
        );
    }

    #[test]
    fn test_search_empty_or_miss() {
        let catalog = Catalog::embedded();
        assert!(catalog.search("", 5).is_empty());
        assert!(catalog.search("  ", 5).is_empty());
        assert!(catalog.search("zzzz-nothing-here", 5).is_empty());
    }

    #[test]
    fn test_search_respects_limit() {
        assert!(Catalog::embedded().search("gpt", 3).len() <= 3);
    }

    #[test]
    fn test_find_by_suffix_matches_last_segment() {
        let entry = Catalog::embedded()
            .find_by_suffix("anthropic/claude-sonnet-4-5")
            .expect("catalog hit");
        assert!(entry.context_length.is_some());
        assert!(entry.max_output_tokens.is_some());
        assert!(entry.is_complete());
    }

    #[test]
    fn test_find_by_suffix_ignores_case() {
        let catalog = Catalog::embedded();
        assert!(
            catalog.find_by_suffix("GPT-4O").is_some()
                || catalog.find_by_suffix("TENCENT/HY3").is_some()
        );
    }

    #[test]
    fn test_find_by_suffix_handles_models_prefix() {
        // Gemini 远端返回 `models/gemini-2.5-flash` 这类 ID，取尾段后应能命中。
        let catalog = Catalog::embedded();
        let direct = catalog.find_by_suffix("gemini-2.5-flash");
        let prefixed = catalog.find_by_suffix("models/gemini-2.5-flash");
        assert_eq!(direct.is_some(), prefixed.is_some());
        if let Some(e) = prefixed {
            assert!(e.is_complete());
        }
    }

    #[test]
    fn test_find_by_suffix_miss_returns_none() {
        let catalog = Catalog::embedded();
        assert!(catalog.find_by_suffix("totally-unknown-model").is_none());
        assert!(catalog.find_by_suffix("").is_none());
    }

    #[test]
    fn test_incomplete_entries_exist() {
        // 432 条中有 9 条缺 limit.output，用于验证 partial 状态的判定来源。
        let data = parse_data(MODELS_JSON).unwrap();
        let incomplete = data
            .index
            .values()
            .filter(|entry| !entry.is_complete())
            .count();
        assert_eq!(incomplete, 9);
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
        let hits = Catalog::embedded().similar_entries("openai/gpt-4o-mini-2029-99-99", 3);
        assert!(!hits.is_empty(), "gpt-4o-mini 变体应命中目录");
        assert!(
            hits.iter()
                .any(|(id, entry)| id.ends_with("gpt-4o-mini") && entry.is_complete())
        );
    }

    #[test]
    fn test_similar_entries_desc_and_limit() {
        let hits = Catalog::embedded().similar_entries("openai/gpt-4o-mini-suffix", 2);
        assert!(hits.len() <= 2);
        // 最相近的目录条目排第一（自身同尾段相似度最高）。
        if let Some((top, _)) = hits.first() {
            assert!(top.ends_with("gpt-4o-mini"));
        }
    }

    #[test]
    fn test_similar_entries_miss_returns_empty() {
        let catalog = Catalog::embedded();
        // 与目录条目均无 50% 以上相似度的 ID 返回空。
        assert!(catalog.similar_entries("qqqqwwwwzzzzxxxx", 3).is_empty());
        assert!(catalog.similar_entries("", 3).is_empty());
    }

    #[test]
    fn test_external_file_preferred_over_embedded() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("models.json");
        write_catalog(&path, &external_gpt4o(999));
        let catalog = Catalog::new(Some(path));

        let entry = catalog.find_by_suffix("gpt-4o").expect("external hit");
        assert_eq!(entry.context_length, Some(999), "应优先用外置文件");
        assert_eq!(catalog.entry_count(), 1);
    }

    #[test]
    fn test_external_change_is_reparsed() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("models.json");
        write_catalog(&path, &external_gpt4o(111));
        let catalog = Catalog::new(Some(path.clone()));
        assert_eq!(
            catalog.find_by_suffix("gpt-4o").unwrap().context_length,
            Some(111)
        );

        // 内容变化（长度不同，mtime/size 必变）→ 无需重启即生效。
        write_catalog(&path, &external_gpt4o(2222));
        assert_eq!(
            catalog.find_by_suffix("gpt-4o").unwrap().context_length,
            Some(2222)
        );
    }

    #[test]
    fn test_missing_external_falls_back_to_embedded() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("models.json");
        let catalog = Catalog::new(Some(path));
        assert_eq!(catalog.entry_count(), 432);
        assert_eq!(
            catalog.find_by_suffix("gpt-4o").unwrap().context_length,
            Some(128000)
        );
    }

    #[test]
    fn test_invalid_external_falls_back_to_embedded() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("models.json");
        write_catalog(&path, "{ not json");
        let catalog = Catalog::new(Some(path));
        assert_eq!(catalog.entry_count(), 432);
        assert!(catalog.find_by_suffix("gpt-4o").is_some());
    }

    #[test]
    fn test_removed_external_returns_to_embedded() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("models.json");
        write_catalog(&path, &external_gpt4o(999));
        let catalog = Catalog::new(Some(path.clone()));
        assert_eq!(catalog.entry_count(), 1);

        std::fs::remove_file(&path).unwrap();
        assert_eq!(catalog.entry_count(), 432, "删除外置文件后回退内嵌");
    }

    #[test]
    fn test_external_path_for_database_url() {
        assert_eq!(
            external_path_for_database_url("sqlite://db/app.db?mode=rwc"),
            Some(PathBuf::from("models.json"))
        );
        assert_eq!(
            external_path_for_database_url("sqlite:///config/db/app.db?mode=rwc"),
            Some(PathBuf::from("/config/models.json"))
        );
        assert_eq!(external_path_for_database_url("sqlite::memory:"), None);
    }

    #[test]
    fn test_validate_rejects_invalid_and_empty() {
        assert!(validate("{ not json").is_err());
        assert!(validate("{}").is_err());
        assert_eq!(validate(&external_gpt4o(1)).unwrap(), 1);
    }
}
