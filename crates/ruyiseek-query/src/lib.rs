//! Bootstrap in-memory query engine.
//!
//! This intentionally avoids edit distance over the full corpus. The native index will
//! replace candidate recall while preserving the public ranking boundary.

use ruyiseek_core::{ItemKind, SearchHit, SearchItem};
use std::cmp::Ordering;

#[derive(Clone, Debug, Default)]
pub struct SearchEngine {
    items: Vec<SearchItem>,
}

impl SearchEngine {
    #[must_use]
    pub fn new(items: Vec<SearchItem>) -> Self {
        Self { items }
    }

    #[must_use]
    pub fn len(&self) -> usize {
        self.items.len()
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }

    /// Return the best `limit` results for a query.
    #[must_use]
    pub fn search(&self, query: &str, limit: usize) -> Vec<SearchHit> {
        if limit == 0 {
            return Vec::new();
        }

        let normalized_query = normalize(query.trim());
        if normalized_query.is_empty() {
            return Vec::new();
        }

        // 把整条 query 切成 token：多关键词时按 AND 求交。
        // 比如 "report 2024" 必须同时在 name 或 path 里出现；
        // 单 token 行为与之前完全一致。
        let tokens: Vec<&str> = normalized_query
            .split_whitespace()
            .filter(|piece| !piece.is_empty())
            .collect();
        if tokens.is_empty() {
            return Vec::new();
        }

        // acronym 检测必须在 normalize 之前做 —— 用户的 query "RSU"
        // 经 normalize 后变成 "rsu"，再判断就全小写了；同时存原 query
        // 用于 acronym 分支。
        let raw_query_uppercase = query.trim().chars().all(|c| !c.is_ascii_lowercase())
            && query.trim().chars().any(|c| c.is_ascii_uppercase());

        let mut hits: Vec<_> = self
            .items
            .iter()
            .filter_map(|item| {
                let value = score(item, &tokens, raw_query_uppercase)?;
                Some(SearchHit {
                    item: item.clone(),
                    score: value,
                })
            })
            .collect();

        hits.sort_by(|left, right| {
            right
                .score
                .partial_cmp(&left.score)
                .unwrap_or(Ordering::Equal)
                .then_with(|| left.item.name.len().cmp(&right.item.name.len()))
                .then_with(|| left.item.path.cmp(&right.item.path))
        });
        hits.truncate(limit);
        hits
    }
}

/// 全部 token 都要命中 name 或 path 的某一段（AND 语义）；命中的
/// "位置质量"决定基础分（完全相等 > 前缀 > 子串 > 跨段子串 > 子序列），
/// 再叠加 acronym / 连续命中 / 段位加权等加成项。
fn score(item: &SearchItem, tokens: &[&str], raw_query_uppercase: bool) -> Option<f32> {
    let name = normalize(&item.name);
    let path_str = normalize(&item.path.to_string_lossy());
    let path_segments: Vec<&str> = path_str
        .split('/')
        .filter(|piece| !piece.is_empty())
        .collect();

    // 每个 token 独立打分：多 token 时所有 token 都必须有命中点（AND
    // 语义），任一 token 没命中整体返回 None；单 token 时允许
    // acronym 兜底，但若连 acronym 都没救回来也返回 None。
    let mut base = f32::MAX;
    let mut any_matched = false;
    let mut any_failed = false;
    for token in tokens {
        if let Some(token_score) = token_score(token, &name, &path_str, &path_segments) {
            any_matched = true;
            if token_score < base {
                base = token_score;
            }
        } else {
            any_failed = true;
        }
    }
    if any_failed && tokens.len() > 1 {
        // 多 token AND 没凑齐；acronym 也救不回来（它只能命中首字母，
        // 不会替代掉缺失的子串）。直接返回 None。
        return None;
    }

    let joined: String = tokens.join(" ");
    let mut value = base;
    let mut acronym_caught = false;

    // acronym 匹配：原 query 全是大写字母（无小写字母），且至少 2 个
    // 字母，那么把 query 当成"首字母拼接"重试一次；命中给个小加成。
    // 这里一定要放在主流程之后，因为 name 子串命中更重要。
    if raw_query_uppercase && tokens.iter().map(|t| t.chars().count()).sum::<usize>() >= 2 {
        let initials = path_segments
            .iter()
            .filter_map(|seg| seg.chars().find(char::is_ascii_alphanumeric))
            .collect::<String>();
        let query_initials: String = tokens.iter().copied().collect();
        if !initials.is_empty() && initials.contains(&query_initials) {
            value += 0.10;
        }
        // 也对 name 做一次：name 里按 `_-/ ` 切词再取首字母
        let name_initials: String = name
            .split(|c: char| c == '_' || c == '-' || c == ' ' || c == '/')
            .filter_map(|seg| seg.chars().find(char::is_ascii_alphanumeric))
            .collect::<String>();
        if !name_initials.is_empty() && name_initials.contains(&query_initials) {
            value += 0.10;
        }
    }

    // 整条 query 当作一整个子序列紧凑命中 → 典型场景："rsdd" 命中
    // "ruyiseek-development-design.md"。给个小加成。
    if tokens.len() >= 2 && subsequence_density(&name, &joined).is_some() {
        value += 0.05;
    }

    // 整条 query 在 name 里连续子串命中（即把 join 后的串是 name 子串）
    // 直接给 0.74 的基础上加成，因为这是非常强的信号。
    if tokens.len() >= 2 && name.contains(&joined) {
        value = value.max(0.82);
    }

    // acronym 兜底：单 token 且原 query 全大写（≥2 个）时，如果该
    // token 没命中 name/path 子串，把它当首字母拼接重试一次；命中给
    // 个中等分。这里必须直接赋值（不是 max），因为 base 可能是 MAX：
    // MAX.max(0.46) 还是 MAX，后续 clamp 会变成 1.0 误导排序。
    if tokens.len() == 1
        && raw_query_uppercase
        && tokens[0].chars().count() >= 2
        && base >= 1.0 - f32::EPSILON
    {
        let query_initials = tokens[0];
        let path_initials: String = path_segments
            .iter()
            .filter_map(|seg| seg.chars().find(char::is_ascii_alphanumeric))
            .collect();
        let name_initials: String = name
            .split(|c: char| c == '_' || c == '-' || c == ' ' || c == '/')
            .filter_map(|seg| seg.chars().find(char::is_ascii_alphanumeric))
            .collect();
        if (!path_initials.is_empty() && path_initials.contains(query_initials))
            || (!name_initials.is_empty() && name_initials.contains(query_initials))
        {
            value = 0.46;
            acronym_caught = true;
        }
    }

    if !value.is_finite()
        || (value <= 0.0 && !acronym_caught)
        || (base >= 1.0 - f32::EPSILON && !acronym_caught && !any_matched)
    {
        // base 是 MAX（所有 token 都没命中 name/path 子串），
        // acronym 也没救回来 → 整体不命中。
        return None;
    }

    if item.kind == ItemKind::Directory {
        value += 0.02;
    }
    if item.hidden {
        value -= 0.12;
    }
    Some(value.clamp(0.0, 1.0))
}

/// 单个 token 的基础分：name 完全相等 > name 前缀 > name 子串 >
/// 路径某段前缀 > 路径某段子串 > 子序列密度（最后兜底）。NULL 表示
/// 这个 token 在任何位置都没命中 → 整体 AND 不通过。
fn token_score(token: &str, name: &str, path: &str, segments: &[&str]) -> Option<f32> {
    if name == token {
        return Some(1.0);
    }
    if name.starts_with(token) {
        return Some(0.88);
    }
    if name.contains(token) {
        return Some(0.74);
    }
    // 路径段位匹配：每个段独立打分，取最高；段前缀比段子串更高。
    let mut best_segment_score: Option<f32> = None;
    for segment in segments {
        if let Some(score) = segment_match_score(segment, token) {
            best_segment_score = Some(best_segment_score.map_or(score, |cur| cur.max(score)));
        }
    }
    if let Some(score) = best_segment_score {
        return Some(score);
    }
    // 子序列密度：兜底匹配，避免用户拼错中间字符也能找到。
    if let Some(density) = subsequence_density(name, token) {
        return Some(0.48 + 0.18 * density);
    }
    // 最后再在完整路径字符串里做段子串（给非常低分，避免误中）。
    if path.contains(token) {
        return Some(0.30);
    }
    None
}

/// 路径段位匹配分数：段前缀 0.62，段子串 0.50。比 name 匹配低，
/// 但比纯子序列高，因为路径段是用户常用的"目录关键词"。
fn segment_match_score(segment: &str, token: &str) -> Option<f32> {
    if segment.starts_with(token) {
        Some(0.62)
    } else if segment.contains(token) {
        Some(0.50)
    } else if subsequence_density(segment, token).is_some() {
        // 段位子序列也是合法信号，给个中等分。
        Some(0.42)
    } else {
        None
    }
}

fn normalize(value: &str) -> String {
    value.chars().flat_map(char::to_lowercase).collect()
}

fn subsequence_density(candidate: &str, query: &str) -> Option<f32> {
    let mut candidate_chars = candidate.char_indices();
    let mut first = None;
    let mut last = 0;

    for needle in query.chars() {
        let (index, _) = candidate_chars.find(|(_, value)| *value == needle)?;
        first.get_or_insert(index);
        last = index;
    }

    let span = last.saturating_sub(first.unwrap_or(0)) + 1;
    let query_length = u16::try_from(query.chars().count()).unwrap_or(u16::MAX);
    let span_length = u16::try_from(span).unwrap_or(u16::MAX);
    Some((f32::from(query_length) / f32::from(span_length)).min(1.0))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn item(id: u64, name: &str, path: &str, kind: ItemKind) -> SearchItem {
        SearchItem {
            id,
            name: name.to_owned(),
            path: PathBuf::from(path),
            kind,
            hidden: false,
        }
    }

    #[test]
    fn exact_name_beats_prefix_and_path() {
        let engine = SearchEngine::new(vec![
            item(1, "report", "/work/report", ItemKind::File),
            item(2, "report-final", "/work/report-final", ItemKind::File),
            item(3, "notes", "/work/report/notes", ItemKind::File),
        ]);

        let hits = engine.search("report", 10);
        assert_eq!(
            hits.iter().map(|hit| hit.item.id).collect::<Vec<_>>(),
            [1, 2, 3]
        );
    }

    #[test]
    fn unicode_and_ascii_matching_are_supported() {
        let engine = SearchEngine::new(vec![
            item(1, "年度报告.md", "/工作/年度报告.md", ItemKind::File),
            item(2, "README.MD", "/repo/README.MD", ItemKind::File),
        ]);

        assert_eq!(engine.search("年度", 5)[0].item.id, 1);
        assert_eq!(engine.search("readme", 5)[0].item.id, 2);
    }

    #[test]
    fn top_k_is_enforced() {
        let engine = SearchEngine::new(vec![
            item(1, "a", "/a", ItemKind::File),
            item(2, "aa", "/aa", ItemKind::File),
            item(3, "aaa", "/aaa", ItemKind::File),
        ]);

        assert_eq!(engine.search("a", 2).len(), 2);
        assert!(engine.search("a", 0).is_empty());
    }

    #[test]
    fn subsequence_matches_without_edit_distance() {
        let engine = SearchEngine::new(vec![item(
            1,
            "ruyiseek-development-design.md",
            "/docs/ruyiseek-development-design.md",
            ItemKind::File,
        )]);

        assert_eq!(engine.search("rsdd", 5)[0].item.id, 1);
    }

    #[test]
    fn multi_token_intersects() {
        // "report 2024" 必须 name 含 report 且 path 含 2024 才匹配。
        let engine = SearchEngine::new(vec![
            item(1, "report", "/work/report", ItemKind::File),
            item(2, "report", "/archive/2024/report", ItemKind::File),
            item(3, "report", "/work/2024/notes", ItemKind::File),
        ]);

        let hits = engine.search("report 2024", 10);
        // item2: name=report + path段"2024"在；item3: name=report + path段"2024"在
        // item1: path 没有 2024 → 不命中
        let ids: Vec<u64> = hits.iter().map(|hit| hit.item.id).collect();
        assert!(ids.contains(&2), "should match item with 2024 in path");
        assert!(ids.contains(&3), "should match item with 2024 in path");
        assert!(!ids.contains(&1), "should not match item without 2024");
    }

    #[test]
    fn acronym_matches_path_initials() {
        // RSU 应能匹配到 ruyiseek-services-ui 这种路径。
        let engine = SearchEngine::new(vec![
            item(1, "x.txt", "/ruyiseek/services/ui/x.txt", ItemKind::File),
            item(2, "y.txt", "/work/notes/y.txt", ItemKind::File),
        ]);

        let hits = engine.search("RSU", 10);
        assert_eq!(hits[0].item.id, 1);
        // item2 没有任何 RSU 含义，不应出现
        assert!(hits.iter().all(|hit| hit.item.id != 2));
    }

    #[test]
    fn segment_prefix_beats_distant_path_substring() {
        // "rep" 在 "/long/path/reports" 里是段前缀（0.62），
        // 而 "rep" 在 "/report-something-entirely-different" 里只是子串。
        let engine = SearchEngine::new(vec![
            item(1, "x.txt", "/long/path/reports/x.txt", ItemKind::File),
            item(
                2,
                "y.txt",
                "/report-something-entirely-different/y.txt",
                ItemKind::File,
            ),
        ]);

        let hits = engine.search("rep", 10);
        // 段前缀应该胜出
        assert_eq!(hits[0].item.id, 1);
    }
}
