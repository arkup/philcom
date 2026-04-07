use anyhow::Result;
use std::fs;
use std::path::PathBuf;

pub struct EditorState {
    pub path: PathBuf,
    pub lines: Vec<String>,
    pub cursor_row: usize,
    pub cursor_col: usize,
    pub scroll: usize,
    pub dirty: bool,
    pub message: Option<String>,
    pub confirm_close: bool,  // Esc pressed while dirty — ask to discard?
    pub internal_clipboard: Option<String>, // fallback when arboard fails
}

impl EditorState {
    pub fn open(path: PathBuf) -> Result<Self> {
        let content = fs::read_to_string(&path).unwrap_or_default();
        let lines: Vec<String> = if content.is_empty() {
            vec![String::new()]
        } else {
            let mut v: Vec<String> = content.lines().map(|l| l.to_string()).collect();
            // Preserve trailing newline as empty last line
            if content.ends_with('\n') {
                v.push(String::new());
            }
            v
        };
        Ok(Self {
            path,
            lines,
            cursor_row: 0,
            cursor_col: 0,
            scroll: 0,
            dirty: false,
            message: None,
            confirm_close: false,
            internal_clipboard: None,
        })
    }

    pub fn save(&mut self) {
        let content = self.lines.join("\n");
        match fs::write(&self.path, &content) {
            Ok(_) => {
                self.dirty = false;
                self.message = Some("Saved.".to_string());
            }
            Err(e) => {
                self.message = Some(format!("Save error: {}", e));
            }
        }
    }

    // ── Cursor movement ───────────────────────────────────────────────────────

    pub fn move_up(&mut self) {
        if self.cursor_row > 0 {
            self.cursor_row -= 1;
            self.clamp_col();
        }
    }

    pub fn move_down(&mut self) {
        if self.cursor_row + 1 < self.lines.len() {
            self.cursor_row += 1;
            self.clamp_col();
        }
    }

    pub fn move_left(&mut self) {
        if self.cursor_col > 0 {
            self.cursor_col -= 1;
        } else if self.cursor_row > 0 {
            self.cursor_row -= 1;
            self.cursor_col = self.lines[self.cursor_row].len();
        }
    }

    pub fn move_right(&mut self) {
        let line_len = self.lines[self.cursor_row].len();
        if self.cursor_col < line_len {
            self.cursor_col += 1;
        } else if self.cursor_row + 1 < self.lines.len() {
            self.cursor_row += 1;
            self.cursor_col = 0;
        }
    }

    pub fn move_home(&mut self) {
        self.cursor_col = 0;
    }

    pub fn move_end(&mut self) {
        self.cursor_col = self.lines[self.cursor_row].len();
    }

    pub fn page_up(&mut self, page_height: usize) {
        self.cursor_row = self.cursor_row.saturating_sub(page_height);
        self.clamp_col();
    }

    pub fn page_down(&mut self, page_height: usize) {
        self.cursor_row = (self.cursor_row + page_height).min(self.lines.len().saturating_sub(1));
        self.clamp_col();
    }

    // ── Editing ───────────────────────────────────────────────────────────────

    pub fn insert_char(&mut self, c: char) {
        self.dirty = true;
        let col = self.cursor_col;
        self.lines[self.cursor_row].insert(col, c);
        self.cursor_col += 1;
    }

    pub fn insert_newline(&mut self) {
        self.dirty = true;
        let col = self.cursor_col;
        let rest = self.lines[self.cursor_row].split_off(col);
        self.cursor_row += 1;
        self.lines.insert(self.cursor_row, rest);
        self.cursor_col = 0;
    }

    pub fn backspace(&mut self) {
        if self.cursor_col > 0 {
            self.dirty = true;
            self.cursor_col -= 1;
            let col = self.cursor_col;
            self.lines[self.cursor_row].remove(col);
        } else if self.cursor_row > 0 {
            self.dirty = true;
            let current = self.lines.remove(self.cursor_row);
            self.cursor_row -= 1;
            self.cursor_col = self.lines[self.cursor_row].len();
            self.lines[self.cursor_row].push_str(&current);
        }
    }

    pub fn delete_char(&mut self) {
        let line_len = self.lines[self.cursor_row].len();
        if self.cursor_col < line_len {
            self.dirty = true;
            self.lines[self.cursor_row].remove(self.cursor_col);
        } else if self.cursor_row + 1 < self.lines.len() {
            self.dirty = true;
            let next = self.lines.remove(self.cursor_row + 1);
            self.lines[self.cursor_row].push_str(&next);
        }
    }

    // ── Clipboard ─────────────────────────────────────────────────────────────

    /// Copy current line to system clipboard (falls back to internal clipboard).
    pub fn copy_line(&mut self) {
        let text = self.lines[self.cursor_row].clone();
        self.internal_clipboard = Some(text.clone());
        self.set_system_clipboard(&text);
        self.message = Some("Line copied.".to_string());
    }

    /// Cut current line: copies it and replaces with empty string (keeps line count stable if only one line, otherwise removes).
    pub fn cut_line(&mut self) {
        let text = self.lines[self.cursor_row].clone();
        self.internal_clipboard = Some(text.clone());
        self.set_system_clipboard(&text);
        if self.lines.len() > 1 {
            self.lines.remove(self.cursor_row);
            if self.cursor_row >= self.lines.len() {
                self.cursor_row = self.lines.len().saturating_sub(1);
            }
        } else {
            self.lines[0] = String::new();
        }
        self.cursor_col = 0;
        self.dirty = true;
        self.message = Some("Line cut.".to_string());
    }

    /// Paste from system clipboard (falls back to internal clipboard).
    pub fn paste(&mut self) {
        let text = self.get_system_clipboard()
            .or_else(|| self.internal_clipboard.clone());
        if let Some(text) = text {
            self.dirty = true;
            // Insert each character, treating newlines as Enter
            for ch in text.chars() {
                if ch == '\n' {
                    self.insert_newline();
                } else if ch != '\r' {
                    self.insert_char(ch);
                }
            }
            self.message = Some("Pasted.".to_string());
        }
    }

    fn set_system_clipboard(&self, text: &str) {
        if let Ok(mut board) = arboard::Clipboard::new() {
            let _ = board.set_text(text);
        }
    }

    fn get_system_clipboard(&self) -> Option<String> {
        arboard::Clipboard::new().ok()?.get_text().ok()
    }

    // ── Scroll adjustment ─────────────────────────────────────────────────────

    pub fn adjust_scroll(&mut self, height: usize) {
        if height == 0 { return; }
        if self.cursor_row < self.scroll {
            self.scroll = self.cursor_row;
        } else if self.cursor_row >= self.scroll + height {
            self.scroll = self.cursor_row + 1 - height;
        }
    }

    // ── Helpers ───────────────────────────────────────────────────────────────

    fn clamp_col(&mut self) {
        let line_len = self.lines[self.cursor_row].len();
        if self.cursor_col > line_len {
            self.cursor_col = line_len;
        }
    }
}
