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

        let mut hits: Vec<_> = self
            .items
            .iter()
            .filter_map(|item| {
                score(item, &normalized_query).map(|value| SearchHit {
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

fn score(item: &SearchItem, query: &str) -> Option<f32> {
    let name = normalize(&item.name);
    let path = normalize(&item.path.to_string_lossy());

    let mut value = if name == query {
        1.0
    } else if name.starts_with(query) {
        0.88
    } else if name.contains(query) {
        0.74
    } else if let Some(density) = subsequence_density(&name, query) {
        0.48 + 0.18 * density
    } else if path.contains(query) {
        0.34
    } else {
        return None;
    };

    if item.kind == ItemKind::Directory {
        value += 0.02;
    }
    if item.hidden {
        value -= 0.12;
    }
    Some(value.clamp(0.0, 1.0))
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
}
