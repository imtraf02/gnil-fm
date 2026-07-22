use std::{
    path::{Path, PathBuf},
    sync::atomic::{AtomicBool, Ordering},
};

use ignore::WalkBuilder;

#[derive(Clone, Copy, Debug)]
pub struct SearchOptions {
    pub show_hidden: bool,
    pub respect_gitignore: bool,
    pub max_results: usize,
}

impl Default for SearchOptions {
    fn default() -> Self {
        Self {
            show_hidden: false,
            respect_gitignore: true,
            max_results: 200,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SearchHit {
    pub path: PathBuf,
    pub score: i64,
}

pub fn search_paths(
    root: &Path,
    query: &str,
    options: SearchOptions,
    cancelled: &AtomicBool,
) -> Vec<SearchHit> {
    let mut builder = WalkBuilder::new(root);
    builder
        .hidden(!options.show_hidden)
        .git_ignore(options.respect_gitignore)
        .git_global(options.respect_gitignore)
        .git_exclude(options.respect_gitignore)
        .follow_links(false);

    let mut hits = Vec::new();
    for item in builder.build().filter_map(Result::ok) {
        if cancelled.load(Ordering::Relaxed) {
            break;
        }
        if item.depth() == 0 {
            continue;
        }
        let relative = item.path().strip_prefix(root).unwrap_or(item.path());
        let candidate = relative.to_string_lossy();
        if let Some(score) = fuzzy_match_score(&candidate, query) {
            hits.push(SearchHit {
                path: item.into_path(),
                score,
            });
        }
    }
    hits.sort_by(|left, right| {
        right.score.cmp(&left.score).then_with(|| {
            left.path
                .as_os_str()
                .len()
                .cmp(&right.path.as_os_str().len())
        })
    });
    hits.truncate(options.max_results);
    hits
}

#[must_use]
pub fn fuzzy_match_score(candidate: &str, query: &str) -> Option<i64> {
    fuzzy_score_normalized(&candidate.to_lowercase(), &query.to_lowercase())
}

fn fuzzy_score_normalized(candidate: &str, query: &str) -> Option<i64> {
    if query.is_empty() {
        return Some(0);
    }
    let mut score = 0_i64;
    let mut query_chars = query.chars().peekable();
    let mut previous_match = None;
    for (index, character) in candidate.chars().enumerate() {
        if query_chars
            .peek()
            .is_some_and(|expected| *expected == character)
        {
            let boundary = index == 0
                || candidate
                    .chars()
                    .nth(index.saturating_sub(1))
                    .is_some_and(|previous| matches!(previous, '/' | '_' | '-' | ' '));
            score += if boundary { 18 } else { 8 };
            if previous_match == Some(index.saturating_sub(1)) {
                score += 12;
            }
            previous_match = Some(index);
            query_chars.next();
        }
    }
    query_chars
        .peek()
        .is_none()
        .then_some(score - i64::try_from(candidate.len()).unwrap_or(i64::MAX))
}

#[cfg(test)]
mod tests {
    use std::{fs, sync::atomic::AtomicBool};

    use super::*;

    #[test]
    fn fuzzy_search_prefers_boundaries_and_respects_hidden() {
        let root = tempfile::tempdir().unwrap();
        fs::create_dir_all(root.path().join("src/components")).unwrap();
        fs::write(root.path().join("src/components/file_row.rs"), b"").unwrap();
        fs::write(root.path().join("src/profile.rs"), b"").unwrap();
        fs::write(root.path().join(".private"), b"").unwrap();
        let hits = search_paths(
            root.path(),
            "fr",
            SearchOptions::default(),
            &AtomicBool::new(false),
        );
        assert_eq!(hits[0].path.file_name().unwrap(), "file_row.rs");
        assert!(
            hits.iter()
                .all(|hit| hit.path.file_name().unwrap() != ".private")
        );
    }

    #[test]
    fn fuzzy_match_is_case_insensitive() {
        assert!(fuzzy_match_score("SourceTree", "st").is_some());
        assert!(fuzzy_match_score("SourceTree", "zz").is_none());
    }
}
