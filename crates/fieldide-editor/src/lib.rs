//! UTF-8 text buffer backed by a rope.

use ropey::Rope;
use std::fs::File;
use std::io::{self, Read, Write};
use std::path::Path;
use tempfile::NamedTempFile;
use unicode_segmentation::UnicodeSegmentation;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Cursor {
    pub line: usize,
    /// Grapheme index within the current line.
    pub grapheme: usize,
}

#[derive(Debug, Clone)]
pub struct TextBuffer {
    rope: Rope,
    cursor: Cursor,
    revision: u64,
    dirty: bool,
}

impl TextBuffer {
    #[must_use]
    pub fn from_text(text: &str) -> Self {
        Self {
            rope: Rope::from_str(text),
            cursor: Cursor::default(),
            revision: 0,
            dirty: false,
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

    pub fn move_right(&mut self) {
        let count = self.current_line_grapheme_count();
        self.cursor.grapheme = self.cursor.grapheme.saturating_add(1).min(count);
    }

    pub fn move_left(&mut self) {
        self.cursor.grapheme = self.cursor.grapheme.saturating_sub(1);
    }

    pub fn move_down(&mut self) {
        if self.cursor.line + 1 < self.rope.len_lines() {
            self.cursor.line += 1;
            self.clamp_cursor();
        }
    }

    pub fn move_up(&mut self) {
        self.cursor.line = self.cursor.line.saturating_sub(1);
        self.clamp_cursor();
    }

    pub fn insert(&mut self, text: &str) {
        let char_index = self.cursor_char_index();
        self.rope.insert(char_index, text);
        self.cursor.grapheme += text.graphemes(true).count();
        self.changed();
    }

    pub fn save_atomic(&mut self, path: &Path) -> io::Result<()> {
        let parent = path
            .parent()
            .filter(|parent| !parent.as_os_str().is_empty())
            .unwrap_or(Path::new("."));
        let mut temporary = NamedTempFile::new_in(parent)?;
        temporary.write_all(self.text().as_bytes())?;
        temporary.as_file_mut().sync_all()?;
        temporary.persist(path).map_err(|error| error.error)?;
        self.dirty = false;
        Ok(())
    }

    fn current_line_grapheme_count(&self) -> usize {
        let line = self.rope.line(self.cursor.line).to_string();
        line.trim_end_matches(['\r', '\n']).graphemes(true).count()
    }

    fn cursor_char_index(&self) -> usize {
        let line_start = self.rope.line_to_char(self.cursor.line);
        let line = self.rope.line(self.cursor.line).to_string();
        let byte_offset = line
            .grapheme_indices(true)
            .nth(self.cursor.grapheme)
            .map_or_else(|| line.len(), |(offset, _)| offset);
        line_start + line[..byte_offset].chars().count()
    }

    fn clamp_cursor(&mut self) {
        let line = self.rope.line(self.cursor.line).to_string();
        let count = line.trim_end_matches(['\r', '\n']).graphemes(true).count();
        self.cursor.grapheme = self.cursor.grapheme.min(count);
    }

    fn changed(&mut self) {
        self.revision = self.revision.saturating_add(1);
        self.dirty = true;
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
}
