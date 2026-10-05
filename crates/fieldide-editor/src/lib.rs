//! UTF-8 text buffers, document management, and workspace path safety.

use ropey::Rope;
use std::fmt;
use std::fs::File;
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};
use tempfile::NamedTempFile;
use unicode_segmentation::UnicodeSegmentation;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Cursor {
    pub line: usize,
    /// Grapheme index within the current line.
    pub grapheme: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Selection {
    /// Character offsets, not byte offsets.
    pub start: usize,
    pub end: usize,
}

#[derive(Debug, Clone)]
struct EditTransaction {
    start: usize,
    removed: String,
    inserted: String,
    before_cursor: Cursor,
    before_selection_anchor: Option<usize>,
    after_cursor: Cursor,
    before_state: u64,
    after_state: u64,
}

#[derive(Debug, Clone)]
pub struct TextBuffer {
    rope: Rope,
    cursor: Cursor,
    selection_anchor: Option<usize>,
    revision: u64,
    current_state: u64,
    saved_state: u64,
    next_state: u64,
    dirty: bool,
    undo: Vec<EditTransaction>,
    redo: Vec<EditTransaction>,
}

impl TextBuffer {
    #[must_use]
    pub fn from_text(text: &str) -> Self {
        Self {
            rope: Rope::from_str(text),
            cursor: Cursor::default(),
            selection_anchor: None,
            revision: 0,
            current_state: 0,
            saved_state: 0,
            next_state: 1,
            dirty: false,
            undo: Vec::new(),
            redo: Vec::new(),
        }
    }

    pub fn open(path: &Path) -> io::Result<Self> {
        let mut bytes = Vec::new();
        File::open(path)?.read_to_end(&mut bytes)?;
        let text = String::from_utf8(bytes)
            .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?;
        Ok(Self::from_text(&text))
    }

    #[must_use]
    pub fn text(&self) -> String {
        self.rope.to_string()
    }

    #[must_use]
    pub const fn cursor(&self) -> Cursor {
        self.cursor
    }

    #[must_use]
    pub const fn revision(&self) -> u64 {
        self.revision
    }

    #[must_use]
    pub const fn is_dirty(&self) -> bool {
        self.dirty
    }

    #[must_use]
    pub fn selection(&self) -> Option<Selection> {
        let anchor = self.selection_anchor?;
        let cursor = self.cursor_char_index();
        (anchor != cursor).then_some(Selection {
            start: anchor.min(cursor),
            end: anchor.max(cursor),
        })
    }

    pub fn move_right(&mut self) {
        self.move_right_with_selection(false);
    }

    pub fn move_right_with_selection(&mut self, selecting: bool) {
        self.prepare_motion(selecting);
        let index = self.cursor_char_index();
        if index < self.rope.len_chars() {
            self.set_cursor_from_char(self.next_grapheme_boundary(index));
        }
        self.finish_motion(selecting);
    }

    pub fn move_left(&mut self) {
        self.move_left_with_selection(false);
    }

    pub fn move_left_with_selection(&mut self, selecting: bool) {
        self.prepare_motion(selecting);
        let index = self.cursor_char_index();
        if index > 0 {
            self.set_cursor_from_char(self.previous_grapheme_boundary(index));
        }
        self.finish_motion(selecting);
    }

    pub fn move_down(&mut self) {
        self.move_down_with_selection(false);
    }

    pub fn move_down_with_selection(&mut self, selecting: bool) {
        self.prepare_motion(selecting);
        if self.cursor.line + 1 < self.rope.len_lines() {
            self.cursor.line += 1;
            self.clamp_cursor();
        }
        self.finish_motion(selecting);
    }

    pub fn move_up(&mut self) {
        self.move_up_with_selection(false);
    }

    pub fn move_up_with_selection(&mut self, selecting: bool) {
        self.prepare_motion(selecting);
        self.cursor.line = self.cursor.line.saturating_sub(1);
        self.clamp_cursor();
        self.finish_motion(selecting);
    }

    pub fn move_home_with_selection(&mut self, selecting: bool) {
        self.prepare_motion(selecting);
        self.cursor.grapheme = 0;
        self.finish_motion(selecting);
    }

    pub fn move_end_with_selection(&mut self, selecting: bool) {
        self.prepare_motion(selecting);
        self.cursor.grapheme = self.current_line_grapheme_count();
        self.finish_motion(selecting);
    }

    pub fn select_all(&mut self) {
        self.selection_anchor = Some(0);
        self.set_cursor_from_char(self.rope.len_chars());
    }

    pub fn insert(&mut self, text: &str) {
        if text.is_empty() {
            return;
        }
        let selection = self.selection();
        let start = selection.map_or_else(|| self.cursor_char_index(), |range| range.start);
        let end = selection.map_or(start, |range| range.end);
        self.replace_range(start, end, text);
    }

    pub fn backspace(&mut self) {
        if self.selection().is_some() {
            let selection = self.selection().expect("selection checked");
            self.replace_range(selection.start, selection.end, "");
            return;
        }
        let end = self.cursor_char_index();
        if end == 0 {
            return;
        }
        let start = self.previous_grapheme_boundary(end);
        self.replace_range(start, end, "");
    }

    pub fn delete_forward(&mut self) {
        if self.selection().is_some() {
            let selection = self.selection().expect("selection checked");
            self.replace_range(selection.start, selection.end, "");
            return;
        }
        let start = self.cursor_char_index();
        if start == self.rope.len_chars() {
            return;
        }
        let end = self.next_grapheme_boundary(start);
        self.replace_range(start, end, "");
    }

    pub fn undo(&mut self) -> bool {
        let Some(transaction) = self.undo.pop() else {
            return false;
        };
        let inserted_end = transaction.start + transaction.inserted.chars().count();
        if transaction.start < inserted_end {
            self.rope.remove(transaction.start..inserted_end);
        }
        if !transaction.removed.is_empty() {
            self.rope.insert(transaction.start, &transaction.removed);
        }
        self.cursor = transaction.before_cursor;
        self.selection_anchor = transaction.before_selection_anchor;
        self.current_state = transaction.before_state;
        self.redo.push(transaction);
        self.changed_after_history();
        true
    }

    pub fn redo(&mut self) -> bool {
        let Some(transaction) = self.redo.pop() else {
            return false;
        };
        let removed_end = transaction.start + transaction.removed.chars().count();
        if transaction.start < removed_end {
            self.rope.remove(transaction.start..removed_end);
        }
        if !transaction.inserted.is_empty() {
            self.rope.insert(transaction.start, &transaction.inserted);
        }
        self.cursor = transaction.after_cursor;
        self.selection_anchor = None;
        self.current_state = transaction.after_state;
        self.undo.push(transaction);
        self.changed_after_history();
        true
    }

    /// Selects the next literal match, wrapping once at end of file.
    pub fn find_next(&mut self, query: &str) -> Option<Selection> {
        if query.is_empty() {
            return None;
        }
        let text = self.text();
        let cursor_byte = self.char_to_byte(self.cursor_char_index());
        let found_byte = text[cursor_byte..]
            .find(query)
            .map(|offset| cursor_byte + offset)
            .or_else(|| text[..cursor_byte].find(query))?;
        let start = text[..found_byte].chars().count();
        let end = start + query.chars().count();
        self.selection_anchor = Some(start);
        self.set_cursor_from_char(end);
        self.selection()
    }

    pub fn save_atomic(&mut self, path: &Path) -> io::Result<()> {
        let parent = path
            .parent()
            .filter(|parent| !parent.as_os_str().is_empty())
            .unwrap_or(Path::new("."));
        let mut temporary = NamedTempFile::new_in(parent)?;
        let text = self.text();
        temporary.write_all(text.as_bytes())?;
        temporary.as_file_mut().sync_all()?;
        temporary.persist(path).map_err(|error| error.error)?;
        self.saved_state = self.current_state;
        self.dirty = false;
        Ok(())
    }

    fn prepare_motion(&mut self, selecting: bool) {
        if selecting && self.selection_anchor.is_none() {
            self.selection_anchor = Some(self.cursor_char_index());
        }
    }

    fn finish_motion(&mut self, selecting: bool) {
        if !selecting || self.selection_anchor == Some(self.cursor_char_index()) {
            self.selection_anchor = None;
        }
    }

    fn current_line_grapheme_count(&self) -> usize {
        let line = self.rope.line(self.cursor.line).to_string();
        visible_line(&line).graphemes(true).count()
    }

    fn cursor_char_index(&self) -> usize {
        let line_start = self.rope.line_to_char(self.cursor.line);
        let line = self.rope.line(self.cursor.line).to_string();
        let visible = visible_line(&line);
        let byte_offset = visible
            .grapheme_indices(true)
            .nth(self.cursor.grapheme)
            .map_or_else(|| visible.len(), |(offset, _)| offset);
        line_start + visible[..byte_offset].chars().count()
    }

    fn set_cursor_from_char(&mut self, char_index: usize) {
        let index = char_index.min(self.rope.len_chars());
        let line = self.rope.char_to_line(index);
        let line_start = self.rope.line_to_char(line);
        let prefix: String = self.rope.slice(line_start..index).chars().collect();
        self.cursor = Cursor {
            line,
            grapheme: visible_line(&prefix).graphemes(true).count(),
        };
    }

    fn clamp_cursor(&mut self) {
        self.cursor.grapheme = self.cursor.grapheme.min(self.current_line_grapheme_count());
    }

    fn previous_grapheme_boundary(&self, char_index: usize) -> usize {
        let text = self.text();
        let byte = self.char_to_byte(char_index);
        text[..byte]
            .grapheme_indices(true)
            .next_back()
            .map_or(0, |(offset, _)| text[..offset].chars().count())
    }

    fn next_grapheme_boundary(&self, char_index: usize) -> usize {
        let text = self.text();
        let byte = self.char_to_byte(char_index);
        let Some(grapheme) = text[byte..].graphemes(true).next() else {
            return char_index;
        };
        char_index + grapheme.chars().count()
    }

    fn char_to_byte(&self, char_index: usize) -> usize {
        self.rope
            .char_to_byte(char_index.min(self.rope.len_chars()))
    }

    fn replace_range(&mut self, start: usize, end: usize, inserted: &str) {
        let removed = self.rope.slice(start..end).to_string();
        let before_cursor = self.cursor;
        let before_selection_anchor = self.selection_anchor;
        let before_state = self.current_state;
        let after_state = self.next_state;
        self.next_state = self.next_state.saturating_add(1);
        if start < end {
            self.rope.remove(start..end);
        }
        if !inserted.is_empty() {
            self.rope.insert(start, inserted);
        }
        self.set_cursor_from_char(start + inserted.chars().count());
        self.selection_anchor = None;
        self.undo.push(EditTransaction {
            start,
            removed,
            inserted: inserted.to_owned(),
            before_cursor,
            before_selection_anchor,
            after_cursor: self.cursor,
            before_state,
            after_state,
        });
        self.current_state = after_state;
        self.redo.clear();
        self.changed_after_history();
    }

    fn changed_after_history(&mut self) {
        self.revision = self.revision.saturating_add(1);
        self.dirty = self.current_state != self.saved_state;
    }
}

fn visible_line(line: &str) -> &str {
    line.trim_end_matches(['\r', '\n'])
}

#[derive(Debug)]
pub enum WorkspaceError {
    Io(io::Error),
    OutsideWorkspace(PathBuf),
    NoParent(PathBuf),
}

impl fmt::Display for WorkspaceError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(error) => error.fmt(formatter),
            Self::OutsideWorkspace(path) => {
                write!(formatter, "workspace 밖의 경로입니다: {}", path.display())
            }
            Self::NoParent(path) => write!(formatter, "부모 경로가 없습니다: {}", path.display()),
        }
    }
}

impl std::error::Error for WorkspaceError {}

impl From<io::Error> for WorkspaceError {
    fn from(error: io::Error) -> Self {
        Self::Io(error)
    }
}

#[derive(Debug, Clone)]
pub struct Workspace {
    root: PathBuf,
}

impl Workspace {
    pub fn new(root: &Path) -> Result<Self, WorkspaceError> {
        Ok(Self {
            root: root.canonicalize()?,
        })
    }

    #[must_use]
    pub fn root(&self) -> &Path {
        &self.root
    }

    pub fn resolve_existing(&self, path: &Path) -> Result<PathBuf, WorkspaceError> {
        let candidate = self.candidate(path).canonicalize()?;
        self.ensure_inside(candidate)
    }

    pub fn resolve_for_save(&self, path: &Path) -> Result<PathBuf, WorkspaceError> {
        let candidate = self.candidate(path);
        if candidate.exists() {
            return self.resolve_existing(&candidate);
        }
        let parent = candidate
            .parent()
            .ok_or_else(|| WorkspaceError::NoParent(candidate.clone()))?
            .canonicalize()?;
        let name = candidate
            .file_name()
            .ok_or_else(|| WorkspaceError::NoParent(candidate.clone()))?;
        self.ensure_inside(parent.join(name))
    }

    fn candidate(&self, path: &Path) -> PathBuf {
        if path.is_absolute() {
            path.to_owned()
        } else {
            self.root.join(path)
        }
    }

    fn ensure_inside(&self, path: PathBuf) -> Result<PathBuf, WorkspaceError> {
        if path.starts_with(&self.root) {
            Ok(path)
        } else {
            Err(WorkspaceError::OutsideWorkspace(path))
        }
    }
}

#[derive(Debug)]
pub struct Document {
    pub path: Option<PathBuf>,
    pub buffer: TextBuffer,
}

#[derive(Debug, Default)]
pub struct DocumentSet {
    documents: Vec<Document>,
    active: usize,
}

impl DocumentSet {
    #[must_use]
    pub fn with_untitled(text: &str) -> Self {
        Self {
            documents: vec![Document {
                path: None,
                buffer: TextBuffer::from_text(text),
            }],
            active: 0,
        }
    }

    #[must_use]
    pub fn len(&self) -> usize {
        self.documents.len()
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.documents.is_empty()
    }

    #[must_use]
    pub fn has_dirty_documents(&self) -> bool {
        self.documents
            .iter()
            .any(|document| document.buffer.is_dirty())
    }

    #[must_use]
    pub const fn active_index(&self) -> usize {
        self.active
    }

    pub fn open(&mut self, workspace: &Workspace, path: &Path) -> Result<usize, WorkspaceError> {
        let path = workspace.resolve_existing(path)?;
        if let Some(index) = self
            .documents
            .iter()
            .position(|document| document.path.as_deref() == Some(path.as_path()))
        {
            self.active = index;
            return Ok(index);
        }
        let buffer = TextBuffer::open(&path)?;
        self.documents.push(Document {
            path: Some(path),
            buffer,
        });
        self.active = self.documents.len() - 1;
        Ok(self.active)
    }

    #[must_use]
    pub fn active(&self) -> Option<&Document> {
        self.documents.get(self.active)
    }

    pub fn active_mut(&mut self) -> Option<&mut Document> {
        self.documents.get_mut(self.active)
    }

    pub fn next(&mut self) {
        if !self.documents.is_empty() {
            self.active = (self.active + 1) % self.documents.len();
        }
    }

    pub fn previous(&mut self) {
        if !self.documents.is_empty() {
            self.active = (self.active + self.documents.len() - 1) % self.documents.len();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cursor_moves_by_grapheme_not_scalar_or_byte() {
        let mut buffer = TextBuffer::from_text("가e\u{301}👨‍👩‍👧‍👦\n");
        buffer.move_right();
        buffer.move_right();
        buffer.insert("한");
        assert_eq!(buffer.text(), "가e\u{301}한👨‍👩‍👧‍👦\n");
        assert_eq!(buffer.cursor().grapheme, 3);
    }

    #[test]
    fn open_and_atomic_save_preserve_crlf_and_unicode() {
        let directory = tempfile::tempdir().expect("temp directory");
        let path = directory.path().join("한글.txt");
        std::fs::write(&path, "첫 줄\r\nemoji 🚀\r\n").expect("fixture write");
        let mut buffer = TextBuffer::open(&path).expect("open");
        buffer.move_right();
        buffer.insert("새");
        buffer.save_atomic(&path).expect("save");
        assert_eq!(
            std::fs::read_to_string(path).expect("read saved"),
            "첫새 줄\r\nemoji 🚀\r\n"
        );
        assert!(!buffer.is_dirty());
    }

    #[test]
    fn vertical_motion_clamps_to_shorter_line() {
        let mut buffer = TextBuffer::from_text("가나다\n끝\n");
        for _ in 0..3 {
            buffer.move_right();
        }
        buffer.move_down();
        assert_eq!(
            buffer.cursor(),
            Cursor {
                line: 1,
                grapheme: 1
            }
        );
    }

    #[test]
    fn selection_replacement_and_grapheme_deletion_are_undoable() {
        let mut buffer = TextBuffer::from_text("가e\u{301}👨‍👩‍👧‍👦");
        buffer.move_right_with_selection(true);
        buffer.move_right_with_selection(true);
        buffer.insert("한");
        assert_eq!(buffer.text(), "한👨‍👩‍👧‍👦");
        assert!(buffer.undo());
        assert_eq!(buffer.text(), "가e\u{301}👨‍👩‍👧‍👦");
        assert!(buffer.redo());
        buffer.delete_forward();
        assert_eq!(buffer.text(), "한");
        buffer.backspace();
        assert_eq!(buffer.text(), "");
    }

    #[test]
    fn literal_search_wraps_and_selects_match() {
        let mut buffer = TextBuffer::from_text("alpha 한글 alpha");
        let first = buffer.find_next("alpha").expect("first match");
        assert_eq!(first, Selection { start: 0, end: 5 });
        let second = buffer.find_next("alpha").expect("second match");
        assert_eq!(second, Selection { start: 9, end: 14 });
        let wrapped = buffer.find_next("alpha").expect("wrapped match");
        assert_eq!(wrapped, first);
    }

    #[test]
    fn workspace_blocks_parent_traversal() {
        let directory = tempfile::tempdir().expect("temp directory");
        let root = directory.path().join("workspace");
        std::fs::create_dir(&root).expect("workspace");
        let outside = directory.path().join("secret.txt");
        std::fs::write(&outside, "secret").expect("outside fixture");
        let workspace = Workspace::new(&root).expect("workspace root");
        assert!(matches!(
            workspace.resolve_existing(Path::new("../secret.txt")),
            Err(WorkspaceError::OutsideWorkspace(_))
        ));
    }

    #[cfg(unix)]
    #[test]
    fn workspace_blocks_symlink_escape() {
        use std::os::unix::fs::symlink;
        let directory = tempfile::tempdir().expect("temp directory");
        let root = directory.path().join("workspace");
        std::fs::create_dir(&root).expect("workspace");
        let outside = directory.path().join("secret.txt");
        std::fs::write(&outside, "secret").expect("outside fixture");
        symlink(&outside, root.join("link.txt")).expect("symlink");
        let workspace = Workspace::new(&root).expect("workspace root");
        assert!(matches!(
            workspace.resolve_existing(Path::new("link.txt")),
            Err(WorkspaceError::OutsideWorkspace(_))
        ));
    }

    #[test]
    fn document_set_switches_between_three_files() {
        let directory = tempfile::tempdir().expect("temp directory");
        for name in ["a.txt", "b.txt", "c.txt"] {
            std::fs::write(directory.path().join(name), name).expect("fixture");
        }
        let workspace = Workspace::new(directory.path()).expect("workspace");
        let mut documents = DocumentSet::default();
        for name in ["a.txt", "b.txt", "c.txt"] {
            documents.open(&workspace, Path::new(name)).expect("open");
        }
        assert_eq!(documents.len(), 3);
        assert!(documents
            .active()
            .expect("active")
            .path
            .as_deref()
            .is_some_and(|path| path.ends_with("c.txt")));
        documents.next();
        assert!(documents
            .active()
            .expect("active")
            .path
            .as_deref()
            .is_some_and(|path| path.ends_with("a.txt")));
        documents.previous();
        assert!(documents
            .active()
            .expect("active")
            .path
            .as_deref()
            .is_some_and(|path| path.ends_with("c.txt")));
    }

    #[test]
    fn one_hundred_edits_undo_to_original_and_redo_to_result() {
        let mut buffer = TextBuffer::from_text("처음");
        for _ in 0..100 {
            buffer.insert("🙂");
        }
        let edited = buffer.text();
        for _ in 0..100 {
            assert!(buffer.undo());
        }
        assert_eq!(buffer.text(), "처음");
        assert!(!buffer.is_dirty());
        for _ in 0..100 {
            assert!(buffer.redo());
        }
        assert_eq!(buffer.text(), edited);
    }

    #[test]
    fn searches_a_one_megabyte_document() {
        let mut text = "x".repeat(1024 * 1024);
        text.push_str("찾았다");
        let mut buffer = TextBuffer::from_text(&text);
        assert!(buffer.find_next("찾았다").is_some());
    }
}
