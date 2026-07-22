use std::{
    fs, io,
    path::{Path, PathBuf},
};

use gnil_fs::fuzzy_match_score;
use gpui::Entity;

use crate::text_input::TextInput;

const HISTORY_LIMIT: usize = 50;

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct PathSuggestion {
    pub(crate) input: String,
    pub(crate) label: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum PathTarget {
    Directory(PathBuf),
    File { path: PathBuf, parent: PathBuf },
}

pub(crate) struct PathInputState {
    pub(crate) input: Entity<TextInput>,
    pub(crate) editing: bool,
    pub(crate) base_path: PathBuf,
    pub(crate) suggestions: Vec<PathSuggestion>,
    pub(crate) focused_suggestion: Option<usize>,
    pub(crate) error: Option<String>,
    pub(crate) checking: bool,
    pub(crate) generation: u64,
    suppress_change: bool,
    history: PathHistory,
}

impl PathInputState {
    pub(crate) fn new(input: Entity<TextInput>, base_path: PathBuf) -> Self {
        Self {
            input,
            editing: false,
            base_path,
            suggestions: Vec::new(),
            focused_suggestion: None,
            error: None,
            checking: false,
            generation: 0,
            suppress_change: false,
            history: PathHistory::default(),
        }
    }

    pub(crate) fn begin(&mut self, base_path: PathBuf) {
        self.editing = true;
        self.base_path = base_path;
        self.clear_transient();
        self.history.reset_cursor();
    }

    pub(crate) fn dismiss(&mut self) {
        self.editing = false;
        self.generation = self.generation.wrapping_add(1);
        self.clear_transient();
        self.history.reset_cursor();
    }

    pub(crate) fn input_changed(&mut self) -> bool {
        if self.suppress_change {
            self.suppress_change = false;
            return false;
        }
        self.generation = self.generation.wrapping_add(1);
        self.clear_transient();
        self.history.reset_cursor();
        true
    }

    pub(crate) fn expect_programmatic_change(&mut self) {
        self.suppress_change = true;
    }

    pub(crate) fn begin_request(&mut self) -> u64 {
        self.generation = self.generation.wrapping_add(1);
        self.checking = true;
        self.error = None;
        self.suggestions.clear();
        self.focused_suggestion = None;
        self.generation
    }

    pub(crate) fn apply_suggestions(
        &mut self,
        generation: u64,
        suggestions: Vec<PathSuggestion>,
        reverse: bool,
    ) -> bool {
        if generation != self.generation || !self.editing {
            return false;
        }
        self.checking = false;
        self.error = None;
        self.suggestions = suggestions;
        self.focused_suggestion = if self.suggestions.is_empty() {
            None
        } else if reverse {
            Some(self.suggestions.len() - 1)
        } else {
            Some(0)
        };
        true
    }

    pub(crate) fn apply_error(&mut self, generation: u64, error: String) -> bool {
        if generation != self.generation || !self.editing {
            return false;
        }
        self.checking = false;
        self.suggestions.clear();
        self.focused_suggestion = None;
        self.error = Some(error);
        true
    }

    pub(crate) fn set_error(&mut self, error: String) {
        self.generation = self.generation.wrapping_add(1);
        self.checking = false;
        self.suggestions.clear();
        self.focused_suggestion = None;
        self.error = Some(error);
    }

    pub(crate) fn move_suggestion(&mut self, reverse: bool) -> bool {
        if self.suggestions.is_empty() {
            return false;
        }
        let len = self.suggestions.len();
        let current = self.focused_suggestion.unwrap_or(0);
        self.focused_suggestion = Some(if reverse {
            current.checked_sub(1).unwrap_or(len - 1)
        } else {
            (current + 1) % len
        });
        true
    }

    pub(crate) fn focus_suggestion(&mut self, index: usize) {
        if index < self.suggestions.len() {
            self.focused_suggestion = Some(index);
        }
    }

    pub(crate) fn focused_suggestion(&self) -> Option<PathSuggestion> {
        self.focused_suggestion
            .and_then(|index| self.suggestions.get(index))
            .cloned()
    }

    pub(crate) fn history_previous(&mut self, draft: &str) -> Option<String> {
        self.clear_completion();
        self.history.previous(draft)
    }

    pub(crate) fn history_next(&mut self) -> Option<String> {
        self.clear_completion();
        self.history.next()
    }

    pub(crate) fn record_success(&mut self, value: String) {
        self.history.record(value);
    }

    fn clear_transient(&mut self) {
        self.checking = false;
        self.error = None;
        self.clear_completion();
    }

    fn clear_completion(&mut self) {
        self.suggestions.clear();
        self.focused_suggestion = None;
    }
}

#[derive(Default)]
struct PathHistory {
    entries: Vec<String>,
    cursor: Option<usize>,
    draft: Option<String>,
}

impl PathHistory {
    fn record(&mut self, value: String) {
        if self.entries.last() != Some(&value) {
            self.entries.push(value);
            if self.entries.len() > HISTORY_LIMIT {
                self.entries.remove(0);
            }
        }
        self.reset_cursor();
    }

    fn previous(&mut self, current_draft: &str) -> Option<String> {
        if self.entries.is_empty() {
            return None;
        }
        let index = if let Some(index) = self.cursor {
            index.saturating_sub(1)
        } else {
            self.draft = Some(current_draft.to_owned());
            self.entries.len() - 1
        };
        self.cursor = Some(index);
        self.entries.get(index).cloned()
    }

    fn next(&mut self) -> Option<String> {
        let index = self.cursor?;
        if index + 1 < self.entries.len() {
            self.cursor = Some(index + 1);
            self.entries.get(index + 1).cloned()
        } else {
            self.cursor = None;
            self.draft.take()
        }
    }

    fn reset_cursor(&mut self) {
        self.cursor = None;
        self.draft = None;
    }
}

pub(crate) fn resolve_path_input(
    input: &str,
    base_path: &Path,
    home_dir: Option<&Path>,
) -> Result<PathBuf, String> {
    if input.is_empty() {
        return Err("Enter a path".into());
    }
    let expanded = if input == "~" {
        home_dir
            .map(Path::to_path_buf)
            .ok_or_else(|| "Home directory is unavailable".to_owned())?
    } else if let Some(relative) = input.strip_prefix("~/") {
        home_dir
            .map(|home| home.join(relative))
            .ok_or_else(|| "Home directory is unavailable".to_owned())?
    } else if input.starts_with('~') {
        return Err("Only ~ and ~/… home paths are supported".into());
    } else {
        PathBuf::from(input)
    };
    Ok(if expanded.is_absolute() {
        expanded
    } else {
        base_path.join(expanded)
    })
}

pub(crate) fn validate_path(path: PathBuf) -> Result<PathTarget, String> {
    let metadata = fs::metadata(&path).map_err(|error| path_error(&path, &error))?;
    if metadata.is_dir() {
        fs::read_dir(&path).map_err(|error| path_error(&path, &error))?;
        return Ok(PathTarget::Directory(path));
    }
    let parent = path
        .parent()
        .map(Path::to_path_buf)
        .ok_or_else(|| "The file has no parent directory".to_owned())?;
    fs::read_dir(&parent).map_err(|error| path_error(&parent, &error))?;
    Ok(PathTarget::File { path, parent })
}

pub(crate) fn completion_candidates(
    input: &str,
    base_path: &Path,
    home_dir: Option<&Path>,
    show_hidden: bool,
) -> Result<Vec<PathSuggestion>, String> {
    if input == "~" {
        home_dir.ok_or_else(|| "Home directory is unavailable".to_owned())?;
        return Ok(vec![PathSuggestion {
            input: "~/".into(),
            label: "~/".into(),
        }]);
    }
    if input.starts_with('~') && !input.starts_with("~/") {
        return Err("Only ~ and ~/… home paths are supported".into());
    }
    let (prefix, fragment) = input.rfind('/').map_or(("", input), |separator| {
        (&input[..=separator], &input[separator + 1..])
    });
    let parent = if prefix.is_empty() {
        base_path.to_path_buf()
    } else {
        resolve_path_input(prefix, base_path, home_dir)?
    };
    let entries = fs::read_dir(&parent).map_err(|error| path_error(&parent, &error))?;
    let mut candidates = Vec::new();
    for entry in entries.filter_map(Result::ok) {
        let name = entry.file_name();
        let Some(name) = name.to_str() else {
            continue;
        };
        if !entry.path().is_dir() {
            continue;
        }
        if name.starts_with('.') && !show_hidden && !fragment.starts_with('.') {
            continue;
        }
        let prefix_match = name.starts_with(fragment);
        let Some(score) = fuzzy_match_score(name, fragment) else {
            continue;
        };
        candidates.push((prefix_match, score, name.to_owned()));
    }
    let has_prefix_matches = candidates.iter().any(|(prefix_match, _, _)| *prefix_match);
    candidates.retain(|(prefix_match, _, _)| !has_prefix_matches || *prefix_match);
    candidates.sort_by(|left, right| {
        right
            .1
            .cmp(&left.1)
            .then_with(|| left.2.to_lowercase().cmp(&right.2.to_lowercase()))
            .then_with(|| left.2.cmp(&right.2))
    });
    Ok(candidates
        .into_iter()
        .map(|(_, _, name)| PathSuggestion {
            input: format!("{prefix}{name}/"),
            label: name,
        })
        .collect())
}

pub(crate) fn single_pasted_path(text: &str) -> Result<String, String> {
    let text = text
        .strip_suffix("\r\n")
        .or_else(|| text.strip_suffix('\n'))
        .unwrap_or(text);
    if text
        .chars()
        .any(|character| matches!(character, '\r' | '\n'))
    {
        return Err("Paste contains multiple paths".into());
    }
    Ok(text.to_owned())
}

fn path_error(path: &Path, error: &io::Error) -> String {
    match error.kind() {
        io::ErrorKind::NotFound => format!("Path does not exist: {}", path.display()),
        io::ErrorKind::PermissionDenied => format!("Permission denied: {}", path.display()),
        io::ErrorKind::NotADirectory => format!("Not a directory: {}", path.display()),
        _ => format!("Cannot access {}: {error}", path.display()),
    }
}

#[cfg(test)]
mod tests {
    use std::{
        fs,
        time::{SystemTime, UNIX_EPOCH},
    };

    use super::*;

    struct TestDirectory(PathBuf);

    impl TestDirectory {
        fn new() -> Self {
            let nonce = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos();
            let path = std::env::temp_dir()
                .join(format!("gnil-path-input-{}-{nonce}", std::process::id()));
            fs::create_dir_all(&path).unwrap();
            Self(path)
        }

        fn path(&self) -> &Path {
            &self.0
        }
    }

    impl Drop for TestDirectory {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn resolves_home_and_relative_paths_without_canonicalizing() {
        let base = Path::new("/work/project");
        let home = Path::new("/home/tester");
        assert_eq!(
            resolve_path_input("~/src", base, Some(home)).unwrap(),
            PathBuf::from("/home/tester/src")
        );
        assert_eq!(
            resolve_path_input("../linked/./src", base, Some(home)).unwrap(),
            PathBuf::from("/work/project/../linked/./src")
        );
        assert!(resolve_path_input("~other/src", base, Some(home)).is_err());
    }

    #[test]
    fn completion_prefers_prefix_then_falls_back_to_fuzzy() {
        let root = TestDirectory::new();
        for directory in ["source", "scripts", "server", ".secret"] {
            fs::create_dir(root.path().join(directory)).unwrap();
        }
        fs::write(root.path().join("src.txt"), b"").unwrap();

        let prefix = completion_candidates("s", root.path(), None, false).unwrap();
        assert!(prefix.iter().all(|item| item.label.starts_with('s')));
        assert!(prefix.iter().all(|item| item.label != ".secret"));
        assert!(prefix.iter().all(|item| item.label != "src.txt"));

        let fuzzy = completion_candidates("scs", root.path(), None, false).unwrap();
        assert_eq!(fuzzy[0].label, "scripts");
        let hidden = completion_candidates(".", root.path(), None, false).unwrap();
        assert_eq!(hidden[0].label, ".secret");
    }

    #[cfg(unix)]
    #[test]
    fn completion_includes_symlink_directories_and_validation_keeps_typed_path() {
        let root = TestDirectory::new();
        fs::create_dir(root.path().join("target")).unwrap();
        std::os::unix::fs::symlink("target", root.path().join("link")).unwrap();
        let candidates = completion_candidates("l", root.path(), None, false).unwrap();
        assert_eq!(candidates[0].label, "link");
        let typed = root.path().join("link");
        assert_eq!(
            validate_path(typed.clone()).unwrap(),
            PathTarget::Directory(typed)
        );
    }

    #[test]
    fn history_restores_draft_and_suppresses_consecutive_duplicates() {
        let mut history = PathHistory::default();
        history.record("/one".into());
        history.record("/one".into());
        history.record("/two".into());
        assert_eq!(history.entries.len(), 2);
        assert_eq!(history.previous("/draft").as_deref(), Some("/two"));
        assert_eq!(history.previous("ignored").as_deref(), Some("/one"));
        assert_eq!(history.next().as_deref(), Some("/two"));
        assert_eq!(history.next().as_deref(), Some("/draft"));
    }

    #[test]
    fn paste_accepts_one_logical_line_and_rejects_multiple() {
        assert_eq!(single_pasted_path("/tmp/item\n").unwrap(), "/tmp/item");
        assert_eq!(single_pasted_path("/tmp/item\r\n").unwrap(), "/tmp/item");
        assert!(single_pasted_path("/one\n/two").is_err());
    }

    #[test]
    fn path_errors_are_specific() {
        assert!(
            path_error(
                Path::new("/missing"),
                &io::Error::from(io::ErrorKind::NotFound)
            )
            .starts_with("Path does not exist")
        );
        assert!(
            path_error(
                Path::new("/private"),
                &io::Error::from(io::ErrorKind::PermissionDenied)
            )
            .starts_with("Permission denied")
        );
    }
}
