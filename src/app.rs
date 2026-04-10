use anyhow::Result;
use crossterm::{
    event::{
        self, DisableMouseCapture, EnableMouseCapture, Event, KeyCode, KeyEventKind,
        KeyModifiers, MouseButton, MouseEventKind,
    },
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::{backend::Backend, layout::Rect, Terminal};
use sha2::{Digest, Sha256};
use std::fs;
use std::io::{self, Read as IoRead, Write as IoWrite};
use std::path::{Path, PathBuf};
use std::sync::{Arc, atomic::{AtomicBool, Ordering}};
use std::time::Duration;

use crate::{config::{Config, Session}, editor::EditorState, panel::Panel, theme::Theme, ui};

pub enum Dialog {
    Mkdir   { input: String },
    NewFile { input: String, dir: PathBuf },
    Copy    { sources: Vec<PathBuf>, dest_input: String },
    Move    { sources: Vec<PathBuf>, dest_input: String },
    Delete  { targets: Vec<PathBuf> },
    Rename  { path: PathBuf, input: String, cursor: usize },
    Goto    { input: String, cursor: usize, panel: PanelSide },
    Sha256Result { filename: String, hash: String },
}

#[derive(Clone)]
pub enum ContextAction {
    CopyPath(PathBuf),
    ToggleMark(String),
    DeleteItem(PathBuf),
    CalcSha256(PathBuf),
    CopyFilename(PathBuf),
    NewFile(PathBuf),
    Rename(PathBuf),
    ViewFile(PathBuf, Option<usize>),  // path, optional line number
    GoToFolder(PathBuf),               // navigate active panel to parent dir
    CopyToPanel(PathBuf),              // open copy dialog for this file
    CopyViewerSelection,               // copy selected text/bytes from viewer
    RevealInFileManager(PathBuf),      // open system file manager and reveal path
}

pub struct ContextMenu {
    pub items: Vec<(String, ContextAction)>,
    pub selected: usize,
    pub x: u16,
    pub y: u16,
    pub rect: Rect,
}

pub struct ConfigDialog {
    pub theme_index:      usize,
    pub restore_session:  bool,
    // rects for mouse hit-testing (populated during render)
    pub theme_left_rect:  Rect,
    pub theme_right_rect: Rect,
    pub restore_rect:     Rect,
    pub save_rect:        Rect,
    pub ok_rect:          Rect,
    pub cancel_rect:      Rect,
}

pub struct LogEntry {
    pub time: String,
    pub op: &'static str,
    pub src: String,
    pub dest: Option<String>,
}

pub struct DriveEntry {
    pub root:  String,  // e.g. "C:\" on Windows, "/media/usb" on Linux
    pub label: String,  // volume label or empty
}

pub struct DriveListPopup {
    pub drives:   Vec<DriveEntry>,
    pub selected: usize,
    pub panel:    PanelSide,
    pub rect:     Rect,
}

pub struct LogPopup {
    pub selected: usize,
    pub scroll: usize,
}

#[derive(Clone, PartialEq)]
pub enum SearchField { Path, Name, FindText, FindHex }

pub struct SearchDialog {
    pub path:        String, pub path_cursor: usize,
    pub name:        String, pub name_cursor: usize,
    pub find_text:   String, pub text_cursor: usize,
    pub find_hex:    String, pub hex_cursor:  usize,
    pub focused:     SearchField,
    // rects for mouse hit-testing (set during render)
    pub path_rect:   Rect,
    pub name_rect:   Rect,
    pub text_rect:   Rect,
    pub hex_rect:    Rect,
}

pub enum SearchResultKind {
    NameMatch,
    TextMatch { line_num: usize, line: String },
    HexMatch  { offset: u64 },
}

pub struct SearchResult {
    pub path: PathBuf,
    pub kind: SearchResultKind,
}

pub struct SearchResultsPanel {
    pub results:   Vec<SearchResult>,
    pub marked:    std::collections::HashSet<usize>,
    pub selected:  usize,
    pub scroll:    usize,
    pub side:      PanelSide,
    pub running:   bool,
    pub rx:        Option<std::sync::mpsc::Receiver<SearchResult>>,
    pub summary:   String,
    pub stop_flag: Arc<AtomicBool>,
    pub anim_tick: u8,
}

pub struct HistoryPopup {
    pub entries: Vec<(PathBuf, bool)>, // (path, is_left)
    pub selected: usize,
    pub scroll: usize,
    pub rect: Rect,
}

pub struct BookmarkPopup {
    pub entries: Vec<String>,
    pub selected: usize,
    pub scroll: usize,
    pub rect: Rect,
    pub target_panel: PanelSide,
}

pub struct ViewerState {
    pub path: PathBuf,
    pub lines: Vec<String>,
    pub raw_bytes: Vec<u8>,
    pub hex_mode: bool,
    pub scroll: usize,
    pub wrap: bool,
    pub total_display_lines: usize,
    pub text_width: usize,
    pub select_start: Option<(usize, usize)>,
    pub select_end: Option<(usize, usize)>,
    pub selecting: bool,
    pub message: Option<String>,
}

enum DialogAction {
    Cancel,
    ConfirmMkdir(String),
    ConfirmNewFile(String, PathBuf),
    ConfirmCopy(Vec<PathBuf>, String),
    ConfirmMove(Vec<PathBuf>, String),
    ConfirmDelete(Vec<PathBuf>),
    CopyHash(String, String), // hash, filename
    ConfirmRename(PathBuf, String),
    ConfirmGoto(String, PanelSide),
}

pub struct App {
    pub left_panel: Panel,
    pub right_panel: Panel,
    pub active_panel: PanelSide,
    pub command_line: String,
    pub theme: Theme,
    pub config: Config,
    pub show_menu: bool,
    pub menu_index: usize,
    pub running: bool,
    pub dialog: Option<Dialog>,
    pub viewer: Option<ViewerState>,
    pub editor: Option<EditorState>,
    pub split_percent: u16,
    pub left_panel_rect: Rect,
    pub right_panel_rect: Rect,
    pub left_sort_rect: Rect,
    pub right_sort_rect: Rect,
    pub left_path_rects: Vec<(Rect, PathBuf)>,
    pub right_path_rects: Vec<(Rect, PathBuf)>,
    pub menu_item_rects: Vec<Rect>,
    pub button_rects: Vec<Rect>,
    pub status_msg: Option<String>,
    pub last_click: Option<(u16, u16, std::time::Instant)>,
    pub submenu_open: bool,
    pub submenu_index: usize,
    pub submenu_rect: Rect,
    pub file_submenu_open: bool,
    pub file_submenu_index: usize,
    pub file_submenu_rect: Rect,
    pub panel_submenu_open: bool,
    pub panel_submenu_side: PanelSide,
    pub panel_submenu_index: usize,
    pub panel_submenu_rect: Rect,
    pub viewer_inner_rect: Rect,
    pub context_menu: Option<ContextMenu>,
    pub history_popup: Option<HistoryPopup>,
    pub bookmark_popup: Option<BookmarkPopup>,
    pub drive_list_popup: Option<DriveListPopup>,
    pub log_popup: Option<LogPopup>,
    pub op_log: Vec<LogEntry>,
    pub pending_command: Option<String>,
    pub sha256_copy_btn_rect: Rect,
    pub goto_paste_menu: Option<Rect>,
    pub left_tab_rects: Vec<Rect>,
    pub right_tab_rects: Vec<Rect>,
    pub config_dialog: Option<ConfigDialog>,
    pub search_dialog: Option<SearchDialog>,
    pub search_results: Option<SearchResultsPanel>,
    pub search_stop_confirm: bool,
    pub search_stop_keep_rect: Rect,
    pub search_stop_discard_rect: Rect,
    pub pending_print_results: bool,
    pub pending_shell: bool,
}

#[derive(Clone, PartialEq)]
pub enum PanelSide {
    Left,
    Right,
}

impl App {
    pub fn new(left_dir: &str, right_dir: &str) -> Result<Self> {
        let config = Config::load()?;
        let theme = Theme::by_name(&config.theme);

        let (left_panel, right_panel, active_panel) = if config.restore_session {
            if let Some(session) = Session::load() {
                let lp = Panel::new_from_paths(&session.left_tabs, session.left_active)
                    .unwrap_or_else(|_| Panel::new(left_dir).unwrap_or_else(|_| Panel::new(".").unwrap()));
                let rp = Panel::new_from_paths(&session.right_tabs, session.right_active)
                    .unwrap_or_else(|_| Panel::new(right_dir).unwrap_or_else(|_| Panel::new(".").unwrap()));
                let ap = if session.active_panel == "right" { PanelSide::Right } else { PanelSide::Left };
                (lp, rp, ap)
            } else {
                (Panel::new(left_dir)?, Panel::new(right_dir)?, PanelSide::Left)
            }
        } else {
            (Panel::new(left_dir)?, Panel::new(right_dir)?, PanelSide::Left)
        };

        Ok(Self {
            left_panel,
            right_panel,
            active_panel,
            command_line: String::new(),
            theme,
            config,
            show_menu: false,
            menu_index: 0,
            running: true,
            dialog: None,
            viewer: None,
            editor: None,
            split_percent: 50,
            left_panel_rect: Rect::default(),
            right_panel_rect: Rect::default(),
            left_sort_rect: Rect::default(),
            right_sort_rect: Rect::default(),
            left_path_rects: Vec::new(),
            right_path_rects: Vec::new(),
            menu_item_rects: Vec::new(),
            button_rects: Vec::new(),
            status_msg: None,
            last_click: None,
            submenu_open: false,
            submenu_index: 0,
            submenu_rect: Rect::default(),
            file_submenu_open: false,
            file_submenu_index: 0,
            file_submenu_rect: Rect::default(),
            panel_submenu_open: false,
            panel_submenu_side: PanelSide::Left,
            panel_submenu_index: 0,
            panel_submenu_rect: Rect::default(),
            viewer_inner_rect: Rect::default(),
            context_menu: None,
            history_popup: None,
            bookmark_popup: None,
            drive_list_popup: None,
            log_popup: None,
            op_log: Vec::new(),
            pending_command: None,
            sha256_copy_btn_rect: Rect::default(),
            goto_paste_menu: None,
            left_tab_rects: Vec::new(),
            right_tab_rects: Vec::new(),
            config_dialog: None,
            search_dialog: None,
            search_results: None,
            search_stop_confirm: false,
            search_stop_keep_rect: Rect::default(),
            search_stop_discard_rect: Rect::default(),
            pending_print_results: false,
            pending_shell: false,
        })
    }

    pub fn run<B: Backend>(&mut self, terminal: &mut Terminal<B>) -> Result<()> {
        while self.running {
            // Shell command execution needs terminal access — handle here
            if let Some(cmd) = self.pending_command.take() {
                self.run_shell_command(terminal, &cmd)?;
            }
            if self.pending_shell {
                self.pending_shell = false;
                self.run_interactive_shell(terminal)?;
            }
            if self.pending_print_results {
                self.pending_print_results = false;
                self.print_search_results(terminal)?;
            }

            // Drain search results from background thread
            if let Some(sr) = &mut self.search_results {
                if let Some(rx) = &sr.rx {
                    loop {
                        match rx.try_recv() {
                            Ok(result) => sr.results.push(result),
                            Err(std::sync::mpsc::TryRecvError::Empty) => break,
                            Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                                sr.running = false;
                                sr.rx = None;
                                break;
                            }
                        }
                    }
                }
                if sr.running {
                    sr.anim_tick = sr.anim_tick.wrapping_add(1);
                }
            }

            terminal.draw(|f| ui::render(f, self))?;

            if event::poll(Duration::from_millis(50))? {
                match event::read()? {
                    Event::Key(key) if key.kind == KeyEventKind::Press => {
                        self.handle_key(key.code, key.modifiers);
                    }
                    Event::Mouse(mouse) => self.handle_mouse(mouse),
                    _ => {}
                }
            }
        }
        Ok(())
    }

    // ── Key handling ─────────────────────────────────────────────────────────

    fn handle_key(&mut self, key: KeyCode, modifiers: KeyModifiers) {
        self.status_msg = None;

        // Search dialog
        if self.search_dialog.is_some() {
            self.handle_search_dialog_key(key, modifiers);
            return;
        }

        // Stop-search confirmation overlay
        if self.search_stop_confirm {
            match key {
                KeyCode::Esc => { self.search_stop_confirm = false; }
                KeyCode::Enter | KeyCode::Char('k') | KeyCode::Char('K') => { self.stop_search(true); }
                KeyCode::Char('d') | KeyCode::Char('D') => { self.stop_search(false); }
                _ => {}
            }
            return;
        }

        // Goto paste menu (must be before dialog)
        if self.goto_paste_menu.is_some() {
            match key {
                KeyCode::Enter => { self.paste_into_goto(); }
                _              => { self.goto_paste_menu = None; }
            }
            return;
        }

        // Dialog always has top priority
        if self.dialog.is_some() {
            // Ctrl+V paste into Goto input (handled here because handle_dialog_key has no modifiers)
            if matches!(&self.dialog, Some(Dialog::Goto { .. }))
                && key == KeyCode::Char('v')
                && modifiers.contains(KeyModifiers::CONTROL)
            {
                self.paste_into_goto();
                return;
            }
            self.handle_dialog_key(key);
            return;
        }

        // Context menu
        if self.context_menu.is_some() {
            match key {
                KeyCode::Esc => self.context_menu = None,
                KeyCode::Up => {
                    if let Some(m) = &mut self.context_menu {
                        if m.selected > 0 { m.selected -= 1; }
                    }
                }
                KeyCode::Down => {
                    if let Some(m) = &mut self.context_menu {
                        if m.selected + 1 < m.items.len() { m.selected += 1; }
                    }
                }
                KeyCode::Enter => {
                    if let Some(m) = self.context_menu.take() {
                        let idx = m.selected;
                        if let Some((_, action)) = m.items.into_iter().nth(idx) {
                            self.execute_context_action(action);
                        }
                    }
                }
                _ => { self.context_menu = None; }
            }
            return;
        }

        // History popup
        if self.history_popup.is_some() {
            self.handle_history_popup_key(key);
            return;
        }

        // Bookmark popup
        if self.bookmark_popup.is_some() {
            self.handle_bookmark_popup_key(key);
            return;
        }

        // Drive list popup
        if self.drive_list_popup.is_some() {
            self.handle_drive_list_popup_key(key);
            return;
        }

        // Log popup
        if self.log_popup.is_some() {
            let len = self.op_log.len();
            match key {
                KeyCode::Esc => { self.log_popup = None; }
                KeyCode::Up => {
                    if let Some(p) = &mut self.log_popup {
                        if p.selected > 0 { p.selected -= 1; }
                        if p.selected < p.scroll { p.scroll = p.selected; }
                    }
                }
                KeyCode::Down => {
                    if let Some(p) = &mut self.log_popup {
                        if p.selected + 1 < len { p.selected += 1; }
                    }
                }
                KeyCode::Char('c') | KeyCode::Char('C') => {
                    if let Some(p) = &self.log_popup {
                        if let Some(entry) = self.op_log.get(p.selected) {
                            let text = match &entry.dest {
                                Some(dest) => format!("[{}] {} {} → {}", entry.time, entry.op, entry.src, dest),
                                None       => format!("[{}] {} {}", entry.time, entry.op, entry.src),
                            };
                            if let Ok(mut board) = arboard::Clipboard::new() {
                                let _ = board.set_text(&text);
                            }
                            self.status_msg = Some("Copied to clipboard".to_string());
                        }
                    }
                }
                _ => {}
            }
            return;
        }

        // Viewer mode
        if self.viewer.is_some() {
            if let Some(v) = &mut self.viewer { v.message = None; }
            match key {
                KeyCode::Esc | KeyCode::F(3) | KeyCode::Char('q') => self.viewer = None,
                KeyCode::Char('h') | KeyCode::Char('H') => {
                    if let Some(v) = &mut self.viewer {
                        v.hex_mode = !v.hex_mode;
                        v.scroll = 0;
                        v.select_start = None;
                        v.select_end = None;
                    }
                }
                KeyCode::Char('w') | KeyCode::Char('W') => {
                    if let Some(v) = &mut self.viewer {
                        v.wrap = !v.wrap;
                        v.scroll = 0;
                        v.select_start = None;
                        v.select_end = None;
                    }
                }
                KeyCode::Char('c') | KeyCode::Char('C') => {
                    self.copy_viewer_selection();
                }
                KeyCode::Up | KeyCode::Char('k') => {
                    if let Some(v) = &mut self.viewer { v.scroll = v.scroll.saturating_sub(1); }
                }
                KeyCode::Down | KeyCode::Char('j') => {
                    if let Some(v) = &mut self.viewer {
                        let limit = v.total_display_lines.saturating_sub(1);
                        if v.scroll < limit { v.scroll += 1; }
                    }
                }
                KeyCode::PageUp => {
                    if let Some(v) = &mut self.viewer { v.scroll = v.scroll.saturating_sub(20); }
                }
                KeyCode::PageDown => {
                    if let Some(v) = &mut self.viewer {
                        let limit = v.total_display_lines.saturating_sub(1);
                        v.scroll = (v.scroll + 20).min(limit);
                    }
                }
                _ => {}
            }
            return;
        }

        // Editor mode
        if self.editor.is_some() {
            self.handle_editor_key(key, modifiers);
            return;
        }

        if self.config_dialog.is_some()  { self.handle_config_dialog_key(key); return; }
        if self.panel_submenu_open { self.handle_panel_submenu_key(key); return; }
        if self.file_submenu_open  { self.handle_file_submenu_key(key); return; }
        if self.submenu_open       { self.handle_submenu_key(key);      return; }
        if self.show_menu          { self.handle_menu_key(key);         return; }

        // Search results panel active on current side
        let search_active = self.search_results.as_ref()
            .map(|sr| sr.side == self.active_panel)
            .unwrap_or(false);
        if search_active {
            self.handle_search_results_key(key);
            return;
        }

        // Normal mode
        match key {
            KeyCode::F(10) => self.running = false,
            KeyCode::Tab   => {
                self.context_menu = None;
                self.history_popup = None;
                self.show_menu = false;
                self.submenu_open = false;
                self.toggle_panel();
            }
            KeyCode::Up    => self.active_panel_mut().move_up(),
            KeyCode::Down  => self.active_panel_mut().move_down(),
            KeyCode::PageUp   if modifiers.contains(KeyModifiers::CONTROL) => {
                self.active_panel_mut().prev_tab();
            }
            KeyCode::PageDown if modifiers.contains(KeyModifiers::CONTROL) => {
                self.active_panel_mut().next_tab();
            }
            KeyCode::PageUp   => { for _ in 0..10 { self.active_panel_mut().move_up();   } }
            KeyCode::PageDown => { for _ in 0..10 { self.active_panel_mut().move_down(); } }
            KeyCode::Enter => {
                // Stop typing filter but keep it active
                if self.active_panel().tab().filter_active {
                    self.active_panel_mut().tab_mut().filter_active = false;
                } else if !self.command_line.is_empty() {
                    let cmd = self.command_line.clone();
                    self.command_line.clear();
                    let trimmed = cmd.trim();
                    match trimmed {
                        "q" | "quit" | "exit" => self.running = false,
                        "cd" => self.execute_cd(""),
                        _ if trimmed.starts_with("cd ") => {
                            self.execute_cd(trimmed["cd ".len()..].trim());
                        }
                        // Windows drive switch: "c:", "d:", etc.
                        _ if trimmed.len() == 2
                            && trimmed.as_bytes()[1] == b':'
                            && trimmed.as_bytes()[0].is_ascii_alphabetic() =>
                        {
                            self.execute_cd(&format!("{}\\", trimmed));
                        }
                        _ => self.pending_command = Some(cmd),
                    }
                } else {
                    self.enter_selected_filtered();
                }
            }
            KeyCode::Char(' ') => {
                if self.command_line.is_empty() {
                    self.active_panel_mut().toggle_mark();
                    let count = self.active_panel().tab().marked.len();
                    if count > 0 {
                        self.status_msg = Some(format!("{} item(s) marked", count));
                    }
                } else {
                    self.command_line.push(' ');
                }
            }
            KeyCode::Backspace => {
                let tab = self.active_panel_mut().tab_mut();
                if tab.filter_active {
                    tab.filter.pop();
                    if tab.filter.is_empty() { tab.filter_active = false; }
                } else {
                    self.command_line.pop();
                }
            }
            KeyCode::Esc => {
                let tab = self.active_panel_mut().tab_mut();
                if !tab.filter.is_empty() || tab.filter_active {
                    tab.filter.clear();
                    tab.filter_active = false;
                    tab.selected = 0;
                    tab.scroll = 0;
                } else {
                    self.active_panel_mut().go_back();
                }
            }
            KeyCode::F(1) if modifiers.contains(KeyModifiers::ALT) => {
                self.open_drive_list_popup(PanelSide::Left);
            }
            KeyCode::F(2) if modifiers.contains(KeyModifiers::ALT) => {
                self.open_drive_list_popup(PanelSide::Right);
            }
            KeyCode::F(1) => self.open_history_popup(),
            KeyCode::F(2) => self.show_menu = true,
            KeyCode::F(3) => self.open_viewer(),
            KeyCode::F(4) => self.open_editor(),
            KeyCode::F(5) => self.open_copy_dialog(),
            KeyCode::F(6) => self.open_move_dialog(),
            KeyCode::F(7) => self.open_mkdir_dialog(),
            KeyCode::F(8) => self.open_delete_dialog(),
            KeyCode::F(9) => self.open_bookmark_popup(),
            KeyCode::Char('d') if modifiers.contains(KeyModifiers::CONTROL) => {
                self.add_bookmark();
            }
            KeyCode::Left if modifiers.contains(KeyModifiers::CONTROL) => {
                if self.split_percent > 20 { self.split_percent -= 5; }
            }
            KeyCode::Right if modifiers.contains(KeyModifiers::CONTROL) => {
                if self.split_percent < 80 { self.split_percent += 5; }
            }
            KeyCode::Char('o') if modifiers.contains(KeyModifiers::CONTROL) => {
                self.pending_shell = true;
            }
            KeyCode::Char('s') if modifiers.contains(KeyModifiers::CONTROL) => {
                self.active_panel_mut().cycle_sort();
            }
            KeyCode::Char('t') if modifiers.contains(KeyModifiers::CONTROL) => {
                self.active_panel_mut().new_tab();
            }
            KeyCode::Char('w') if modifiers.contains(KeyModifiers::CONTROL) => {
                self.active_panel_mut().close_tab();
            }
            KeyCode::Left | KeyCode::Right if modifiers.is_empty() && self.command_line.is_empty() => {
                self.toggle_panel();
            }
            KeyCode::Char('h') if modifiers.contains(KeyModifiers::ALT) => {
                self.open_history_popup();
            }
            KeyCode::Char('l') if modifiers.contains(KeyModifiers::ALT) => {
                self.log_popup = Some(LogPopup { selected: 0, scroll: 0 });
            }
            KeyCode::Char('g') | KeyCode::Char('G') if self.command_line.is_empty() && !self.active_panel().tab().filter_active => {
                self.dialog = Some(Dialog::Goto {
                    input: String::new(),
                    cursor: 0,
                    panel: self.active_panel.clone(),
                });
            }
            KeyCode::Char(c) => {
                if modifiers.contains(KeyModifiers::CONTROL) && c == 'c' {
                    self.running = false;
                } else if self.active_panel().tab().filter_active {
                    self.active_panel_mut().tab_mut().filter.push(c);
                    self.active_panel_mut().tab_mut().selected = 0;
                    self.active_panel_mut().tab_mut().scroll = 0;
                } else if c == '/' && self.command_line.is_empty() && modifiers.is_empty() {
                    self.active_panel_mut().tab_mut().filter_active = true;
                } else {
                    self.command_line.push(c);
                }
            }
            _ => {}
        }
    }

    fn handle_dialog_key(&mut self, key: KeyCode) {
        let action: Option<DialogAction> = match &mut self.dialog {
            Some(Dialog::Mkdir { input }) => match key {
                KeyCode::Esc   => Some(DialogAction::Cancel),
                KeyCode::Enter => Some(DialogAction::ConfirmMkdir(input.clone())),
                KeyCode::Backspace => { input.pop(); None }
                KeyCode::Char(c)   => { input.push(c); None }
                _ => None,
            },
            Some(Dialog::NewFile { input, dir }) => match key {
                KeyCode::Esc   => Some(DialogAction::Cancel),
                KeyCode::Enter => Some(DialogAction::ConfirmNewFile(input.clone(), dir.clone())),
                KeyCode::Backspace => { input.pop(); None }
                KeyCode::Char(c)   => { input.push(c); None }
                _ => None,
            },
            Some(Dialog::Copy { sources, dest_input }) => match key {
                KeyCode::Esc   => Some(DialogAction::Cancel),
                KeyCode::Enter => Some(DialogAction::ConfirmCopy(sources.clone(), dest_input.clone())),
                KeyCode::Backspace => { dest_input.pop(); None }
                KeyCode::Char(c)   => { dest_input.push(c); None }
                _ => None,
            },
            Some(Dialog::Move { sources, dest_input }) => match key {
                KeyCode::Esc   => Some(DialogAction::Cancel),
                KeyCode::Enter => Some(DialogAction::ConfirmMove(sources.clone(), dest_input.clone())),
                KeyCode::Backspace => { dest_input.pop(); None }
                KeyCode::Char(c)   => { dest_input.push(c); None }
                _ => None,
            },
            Some(Dialog::Delete { targets }) => match key {
                KeyCode::Esc | KeyCode::Char('n') | KeyCode::Char('N') => Some(DialogAction::Cancel),
                KeyCode::Enter | KeyCode::Char('y') | KeyCode::Char('Y') => {
                    Some(DialogAction::ConfirmDelete(targets.clone()))
                }
                _ => None,
            },
            Some(Dialog::Rename { path, input, cursor }) => {
                let char_len = input.chars().count();
                match key {
                    KeyCode::Esc   => Some(DialogAction::Cancel),
                    KeyCode::Enter => Some(DialogAction::ConfirmRename(path.clone(), input.clone())),
                    KeyCode::Left  => { if *cursor > 0 { *cursor -= 1; } None }
                    KeyCode::Right => { if *cursor < char_len { *cursor += 1; } None }
                    KeyCode::Home  => { *cursor = 0; None }
                    KeyCode::End   => { *cursor = char_len; None }
                    KeyCode::Backspace => {
                        if *cursor > 0 {
                            let byte = char_to_byte(input, *cursor - 1);
                            input.remove(byte);
                            *cursor -= 1;
                        }
                        None
                    }
                    KeyCode::Delete => {
                        if *cursor < char_len {
                            let byte = char_to_byte(input, *cursor);
                            input.remove(byte);
                        }
                        None
                    }
                    KeyCode::Char(c) => {
                        let byte = char_to_byte(input, *cursor);
                        input.insert(byte, c);
                        *cursor += 1;
                        None
                    }
                    _ => None,
                }
            }
            Some(Dialog::Goto { input, cursor, panel }) => {
                let char_len = input.chars().count();
                match key {
                    KeyCode::Esc  => Some(DialogAction::Cancel),
                    KeyCode::Enter => Some(DialogAction::ConfirmGoto(input.clone(), panel.clone())),
                    KeyCode::Tab  => { *panel = match panel { PanelSide::Left => PanelSide::Right, PanelSide::Right => PanelSide::Left }; None }
                    KeyCode::Left  => { if *cursor > 0 { *cursor -= 1; } None }
                    KeyCode::Right => { if *cursor < char_len { *cursor += 1; } None }
                    KeyCode::Home  => { *cursor = 0; None }
                    KeyCode::End   => { *cursor = char_len; None }
                    KeyCode::Backspace => {
                        if *cursor > 0 {
                            let byte = char_to_byte(input, *cursor - 1);
                            input.remove(byte);
                            *cursor -= 1;
                        }
                        None
                    }
                    KeyCode::Delete => {
                        if *cursor < char_len {
                            let byte = char_to_byte(input, *cursor);
                            input.remove(byte);
                        }
                        None
                    }
                    KeyCode::Char(c) => {
                        let byte = char_to_byte(input, *cursor);
                        input.insert(byte, c);
                        *cursor += 1;
                        None
                    }
                    _ => None,
                }
            }
            Some(Dialog::Sha256Result { hash, filename }) => match key {
                KeyCode::Esc | KeyCode::Enter => Some(DialogAction::Cancel),
                KeyCode::Char('c') | KeyCode::Char('C') => {
                    Some(DialogAction::CopyHash(hash.clone(), filename.clone()))
                }
                _ => None,
            },
            None => None,
        };

        match action {
            Some(DialogAction::Cancel)                    => self.dialog = None,
            Some(DialogAction::ConfirmMkdir(name))        => { self.dialog = None; self.execute_mkdir(&name); }
            Some(DialogAction::ConfirmNewFile(name, dir)) => { self.dialog = None; self.execute_new_file(&name, &dir); }
            Some(DialogAction::ConfirmCopy(src, dest))  => { self.dialog = None; self.execute_copy(&src, &dest); }
            Some(DialogAction::ConfirmMove(src, dest))  => { self.dialog = None; self.execute_move(&src, &dest); }
            Some(DialogAction::ConfirmDelete(targets))  => { self.dialog = None; self.execute_delete(&targets); }
            Some(DialogAction::ConfirmRename(path, name)) => { self.dialog = None; self.execute_rename(&path, &name); }
            Some(DialogAction::ConfirmGoto(path, side))   => { self.dialog = None; self.execute_goto(&path, side); }
            Some(DialogAction::CopyHash(hash, filename)) => {
                let text = format!("{}  {}", hash, filename);
                if let Ok(mut board) = arboard::Clipboard::new() {
                    let _ = board.set_text(&text);
                }
                self.status_msg = Some("Hash copied to clipboard".to_string());
            }
            None => {}
        }
    }

    fn handle_menu_key(&mut self, key: KeyCode) {
        match key {
            KeyCode::Esc | KeyCode::F(2) => self.show_menu = false,
            KeyCode::Left  => { if self.menu_index > 0 { self.menu_index -= 1; } }
            KeyCode::Right => { if self.menu_index < 3 { self.menu_index += 1; } }
            KeyCode::Enter | KeyCode::Down => {
                match self.menu_index {
                    0 => { // File submenu
                        self.file_submenu_index = 0;
                        self.file_submenu_open = true;
                    }
                    1 | 3 => { // Left / Right panel submenus
                        self.panel_submenu_side = if self.menu_index == 1 { PanelSide::Left } else { PanelSide::Right };
                        let filter_exec = if self.menu_index == 1 { self.left_panel.filter_exec } else { self.right_panel.filter_exec };
                        self.panel_submenu_index = if filter_exec { 1 } else { 0 };
                        self.panel_submenu_open = true;
                    }
                    2 => { // Options → configuration dialog
                        self.open_config_dialog();
                        self.show_menu = false;
                    }
                    _ => {}
                }
            }
            _ => {}
        }
    }

    fn handle_file_submenu_key(&mut self, key: KeyCode) {
        const FILE_ITEMS: usize = 4; // "Search", "Log", "Clear settings", "Exit"
        match key {
            KeyCode::Esc => self.file_submenu_open = false,
            KeyCode::Up   => { if self.file_submenu_index > 0 { self.file_submenu_index -= 1; } }
            KeyCode::Down => { if self.file_submenu_index + 1 < FILE_ITEMS { self.file_submenu_index += 1; } }
            KeyCode::Enter => {
                let idx = self.file_submenu_index;
                self.file_submenu_open = false;
                self.show_menu = false;
                match idx {
                    0 => { self.open_search_dialog(); }
                    1 => { self.log_popup = Some(LogPopup { selected: 0, scroll: 0 }); }
                    2 => { self.clear_settings(); }
                    3 => { self.running = false; }
                    _ => {}
                }
            }
            _ => { self.file_submenu_open = false; }
        }
    }

    // ── Search ────────────────────────────────────────────────────────────────

    fn open_search_dialog(&mut self) {
        let path = clean_path(&self.active_panel().tab().path);
        let cursor = path.chars().count();
        self.search_dialog = Some(SearchDialog {
            path, path_cursor: cursor,
            name: String::new(), name_cursor: 0,
            find_text: String::new(), text_cursor: 0,
            find_hex:  String::new(), hex_cursor: 0,
            focused: SearchField::Path,
            path_rect: Rect::default(),
            name_rect: Rect::default(),
            text_rect: Rect::default(),
            hex_rect:  Rect::default(),
        });
    }

    fn handle_search_dialog_key(&mut self, key: KeyCode, modifiers: KeyModifiers) {
        let ctrl = modifiers.contains(KeyModifiers::CONTROL);
        match key {
            KeyCode::Esc => { self.search_dialog = None; }
            KeyCode::Tab => {
                if let Some(d) = &mut self.search_dialog {
                    d.focused = match d.focused {
                        SearchField::Path    => SearchField::Name,
                        SearchField::Name    => SearchField::FindText,
                        SearchField::FindText => SearchField::FindHex,
                        SearchField::FindHex => SearchField::Path,
                    };
                }
            }
            KeyCode::Enter => { self.execute_search(); }
            KeyCode::Char('v') if ctrl => { self.paste_into_search_field(); }
            _ => {
                if let Some(d) = &mut self.search_dialog {
                    let (input, cursor) = match d.focused {
                        SearchField::Path     => (&mut d.path,      &mut d.path_cursor),
                        SearchField::Name     => (&mut d.name,      &mut d.name_cursor),
                        SearchField::FindText => (&mut d.find_text, &mut d.text_cursor),
                        SearchField::FindHex  => (&mut d.find_hex,  &mut d.hex_cursor),
                    };
                    match key {
                        KeyCode::Backspace => {
                            if *cursor > 0 {
                                let b = char_to_byte(input, *cursor - 1);
                                input.remove(b);
                                *cursor -= 1;
                            }
                        }
                        KeyCode::Delete => {
                            if *cursor < input.chars().count() {
                                let b = char_to_byte(input, *cursor);
                                input.remove(b);
                            }
                        }
                        KeyCode::Left  => { if *cursor > 0 { *cursor -= 1; } }
                        KeyCode::Right => { let n = input.chars().count(); if *cursor < n { *cursor += 1; } }
                        KeyCode::Home  => { *cursor = 0; }
                        KeyCode::End   => { *cursor = input.chars().count(); }
                        KeyCode::Char(c) if !ctrl => {
                            let b = char_to_byte(input, *cursor);
                            input.insert(b, c);
                            *cursor += 1;
                        }
                        _ => {}
                    }
                }
            }
        }
    }

    fn paste_into_search_field(&mut self) {
        if let Ok(mut board) = arboard::Clipboard::new() {
            if let Ok(text) = board.get_text() {
                let text = text.trim_end_matches('\n').to_string();
                if let Some(d) = &mut self.search_dialog {
                    let (input, cursor) = match d.focused {
                        SearchField::Path     => (&mut d.path,      &mut d.path_cursor),
                        SearchField::Name     => (&mut d.name,      &mut d.name_cursor),
                        SearchField::FindText => (&mut d.find_text, &mut d.text_cursor),
                        SearchField::FindHex  => (&mut d.find_hex,  &mut d.hex_cursor),
                    };
                    let b = char_to_byte(input, *cursor);
                    input.insert_str(b, &text);
                    *cursor += text.chars().count();
                }
            }
        }
    }

    fn execute_search(&mut self) {
        let d = match self.search_dialog.take() { Some(d) => d, None => return };
        let root   = PathBuf::from(d.path.trim());
        let name   = if d.name.trim().is_empty() { "*".to_string() } else { d.name.trim().to_string() };
        let text   = if d.find_text.trim().is_empty() { None } else { Some(d.find_text.trim().to_string()) };
        let hex = if d.find_hex.trim().is_empty() {
            None
        } else {
            match parse_hex_pattern(d.find_hex.trim()) {
                Some(b) => Some(b),
                None => {
                    self.status_msg = Some("Invalid hex pattern".to_string());
                    self.search_dialog = Some(d);
                    return;
                }
            }
        };

        let mut summary = name.clone();
        if let Some(t) = &text { summary.push_str(&format!(" + \"{}\"", t)); }
        if hex.is_some() { summary.push_str(" + hex"); }

        let (tx, rx) = std::sync::mpsc::channel::<SearchResult>();
        let name_c = name.clone();
        let text_c = text.clone();
        let hex_c  = hex.clone();
        let stop_flag = Arc::new(AtomicBool::new(false));
        let stop_c = Arc::clone(&stop_flag);
        std::thread::spawn(move || {
            search_dir_recursive(&root, &name_c, text_c.as_deref(), hex_c.as_deref(), &tx, &stop_c);
        });

        let side = self.active_panel.clone();
        self.search_results = Some(SearchResultsPanel {
            results: Vec::new(),
            marked: std::collections::HashSet::new(),
            selected: 0,
            scroll: 0,
            side,
            running: true,
            rx: Some(rx),
            summary,
            stop_flag,
            anim_tick: 0,
        });
    }

    fn stop_search(&mut self, keep: bool) {
        self.search_stop_confirm = false;
        if let Some(sr) = &self.search_results {
            sr.stop_flag.store(true, Ordering::Relaxed);
        }
        if !keep {
            self.search_results = None;
        }
    }

    fn handle_search_results_key(&mut self, key: KeyCode) {
        let len = self.search_results.as_ref().map(|s| s.results.len()).unwrap_or(0);
        match key {
            KeyCode::Esc => {
                let running = self.search_results.as_ref().map(|s| s.running).unwrap_or(false);
                if running {
                    self.search_stop_confirm = true;
                } else {
                    self.search_results = None;
                }
            }
            KeyCode::Up => {
                if let Some(sr) = &mut self.search_results {
                    if sr.selected > 0 { sr.selected -= 1; }
                    if sr.selected < sr.scroll { sr.scroll = sr.selected; }
                }
            }
            KeyCode::Down => {
                if let Some(sr) = &mut self.search_results {
                    if sr.selected + 1 < len { sr.selected += 1; }
                }
            }
            KeyCode::PageUp => {
                if let Some(sr) = &mut self.search_results {
                    sr.selected = sr.selected.saturating_sub(10);
                    if sr.selected < sr.scroll { sr.scroll = sr.selected; }
                }
            }
            KeyCode::PageDown => {
                if let Some(sr) = &mut self.search_results {
                    sr.selected = (sr.selected + 10).min(len.saturating_sub(1));
                }
            }
            KeyCode::Char(' ') => {
                if let Some(sr) = &mut self.search_results {
                    if sr.selected < len {
                        if !sr.marked.remove(&sr.selected) { sr.marked.insert(sr.selected); }
                    }
                }
            }
            KeyCode::Enter | KeyCode::F(3) => { self.open_search_result(); }
            KeyCode::F(5)  => { self.copy_from_search_results(); }
            KeyCode::Char('p') | KeyCode::Char('P') => { self.pending_print_results = true; }
            _ => {}
        }
    }

    fn open_search_result(&mut self) {
        let (path, line_num) = {
            let sr = match &self.search_results { Some(s) => s, None => return };
            let result = match sr.results.get(sr.selected) { Some(r) => r, None => return };
            let line = match &result.kind {
                SearchResultKind::TextMatch { line_num, .. } => Some(*line_num),
                _ => None,
            };
            (result.path.clone(), line)
        };
        if path.is_dir() {
            let panel = match self.active_panel {
                PanelSide::Left  => &mut self.left_panel,
                PanelSide::Right => &mut self.right_panel,
            };
            panel.tab_mut().path = path;
            panel.tab_mut().selected = 0;
            panel.tab_mut().scroll = 0;
            let _ = panel.load_entries();
            panel.record_visit();
            self.search_results = None;
        } else {
            match fs::read(&path) {
                Ok(raw_bytes) => {
                    let (lines, hex_mode) = match String::from_utf8(raw_bytes.clone()) {
                        Ok(text) => (text.lines().map(|l| l.to_string()).collect(), false),
                        Err(_)   => (Vec::new(), true),
                    };
                    let scroll = line_num.map(|n| n.saturating_sub(1)).unwrap_or(0);
                    self.viewer = Some(ViewerState {
                        path, lines, raw_bytes, hex_mode, scroll,
                        wrap: false, total_display_lines: 0, text_width: 0,
                        select_start: None, select_end: None, selecting: false, message: None,
                    });
                }
                Err(_) => self.status_msg = Some("Cannot read file".to_string()),
            }
        }
    }

    fn open_search_result_context_menu(&mut self, x: u16, y: u16, idx: usize) {
        let (path, line_num) = {
            let sr = match &self.search_results { Some(s) => s, None => return };
            let r  = match sr.results.get(idx)   { Some(r) => r, None => return };
            let ln = match &r.kind {
                SearchResultKind::TextMatch { line_num, .. } => Some(*line_num),
                _ => None,
            };
            (r.path.clone(), ln)
        };
        let items: Vec<(String, ContextAction)> = vec![
            ("View".to_string(),          ContextAction::ViewFile(path.clone(), line_num)),
            ("Copy path".to_string(),     ContextAction::CopyPath(path.clone())),
            ("Copy filename".to_string(), ContextAction::CopyFilename(path.clone())),
            ("Copy to panel".to_string(), ContextAction::CopyToPanel(path.clone())),
            ("Go to folder".to_string(),  ContextAction::GoToFolder(path.clone())),
        ];
        let w = items.iter().map(|(s, _)| s.len() as u16).max().unwrap_or(8) + 4;
        let h = items.len() as u16 + 2;
        let area = { let a = self.left_panel_rect; Rect::new(a.x, a.y, a.width + self.right_panel_rect.width, a.height) };
        let rx = x.min(area.width.saturating_sub(w));
        let ry = y.min(area.height.saturating_sub(h).max(1));
        let rect = Rect::new(rx, ry, w, h);
        self.context_menu = Some(ContextMenu { items, selected: 0, x, y, rect });
    }

    fn copy_from_search_results(&mut self) {
        let sources: Vec<PathBuf> = {
            let sr = match &self.search_results { Some(s) => s, None => return };
            if sr.marked.is_empty() {
                sr.results.get(sr.selected).map(|r| vec![r.path.clone()]).unwrap_or_default()
            } else {
                sr.marked.iter().filter_map(|&i| sr.results.get(i).map(|r| r.path.clone())).collect()
            }
        };
        if sources.is_empty() { return; }
        let dest = self.inactive_panel().tab().path.display().to_string();
        self.dialog = Some(Dialog::Copy { sources, dest_input: dest });
    }

    fn paste_into_goto(&mut self) {
        self.goto_paste_menu = None;
        if let Ok(mut board) = arboard::Clipboard::new() {
            if let Ok(text) = board.get_text() {
                let text = text.trim_end_matches('\n').to_string();
                if let Some(Dialog::Goto { input, cursor, .. }) = &mut self.dialog {
                    let byte = char_to_byte(input, *cursor);
                    input.insert_str(byte, &text);
                    *cursor += text.chars().count();
                }
            }
        }
    }

    fn clear_settings(&mut self) {
        let path = Config::config_path();
        let _ = std::fs::remove_file(&path);
        let session_path = Session::session_path();
        let _ = std::fs::remove_file(&session_path);
        self.config = Config::default();
        self.theme = Theme::by_name(&self.config.theme);
        let path_str = path.display().to_string();
        self.log_op("DEL", path_str, None);
        self.status_msg = Some("Settings cleared — defaults restored.".to_string());
    }

    fn handle_panel_submenu_key(&mut self, key: KeyCode) {
        match key {
            KeyCode::Esc => self.panel_submenu_open = false,
            KeyCode::Up   => { if self.panel_submenu_index > 0 { self.panel_submenu_index -= 1; } }
            KeyCode::Down => { if self.panel_submenu_index < 6 { self.panel_submenu_index += 1; } }
            KeyCode::Enter => {
                let idx = self.panel_submenu_index;
                self.panel_submenu_open = false;
                self.show_menu = false;
                self.execute_panel_submenu_action(idx);
            }
            _ => { self.panel_submenu_open = false; }
        }
    }

    fn execute_panel_submenu_action(&mut self, idx: usize) {
        if idx < 2 {
            let filter_on = idx == 1;
            match self.panel_submenu_side {
                PanelSide::Left  => self.left_panel.set_exec_filter(filter_on),
                PanelSide::Right => self.right_panel.set_exec_filter(filter_on),
            }
        } else if idx == 2 {
            let dir = match self.panel_submenu_side {
                PanelSide::Left  => self.left_panel.tab().path.clone(),
                PanelSide::Right => self.right_panel.tab().path.clone(),
            };
            self.dialog = Some(Dialog::NewFile { input: String::new(), dir });
        } else if idx == 3 {
            self.active_panel = self.panel_submenu_side.clone();
            self.open_history_popup();
        } else if idx == 4 {
            self.navigate_panel_to_downloads();
        } else if idx == 5 {
            match self.panel_submenu_side {
                PanelSide::Left  => self.left_panel.new_tab(),
                PanelSide::Right => self.right_panel.new_tab(),
            }
        } else if idx == 6 {
            match self.panel_submenu_side {
                PanelSide::Left  => self.left_panel.close_tab(),
                PanelSide::Right => self.right_panel.close_tab(),
            }
        }
    }

    fn handle_submenu_key(&mut self, key: KeyCode) {
        let count = Theme::all_names().len();
        match key {
            KeyCode::Esc  => self.submenu_open = false,
            KeyCode::Up   => { if self.submenu_index > 0 { self.submenu_index -= 1; } }
            KeyCode::Down => { if self.submenu_index + 1 < count { self.submenu_index += 1; } }
            KeyCode::Enter => {
                let name = Theme::all_names()[self.submenu_index].to_string();
                self.theme = Theme::by_name(&name);
                self.config.theme = name;
                let _ = self.config.save();
                self.submenu_open = false;
                self.show_menu = false;
            }
            _ => {}
        }
    }

    // ── Configuration dialog ─────────────────────────────────────────────────

    pub fn open_config_dialog(&mut self) {
        let themes = Theme::all_names();
        let theme_index = themes.iter().position(|&n| n == self.config.theme.as_str()).unwrap_or(0);
        self.config_dialog = Some(ConfigDialog {
            theme_index,
            restore_session: self.config.restore_session,
            theme_left_rect:  Rect::default(),
            theme_right_rect: Rect::default(),
            restore_rect:     Rect::default(),
            save_rect:        Rect::default(),
            ok_rect:          Rect::default(),
            cancel_rect:      Rect::default(),
        });
    }

    fn handle_config_dialog_key(&mut self, key: KeyCode) {
        let themes = Theme::all_names();
        let dialog = match self.config_dialog.as_mut() {
            Some(d) => d,
            None => return,
        };
        match key {
            KeyCode::Esc => { self.config_dialog = None; }
            KeyCode::Left  => { if dialog.theme_index > 0 { dialog.theme_index -= 1; } }
            KeyCode::Right => { if dialog.theme_index + 1 < themes.len() { dialog.theme_index += 1; } }
            KeyCode::Char(' ') => { dialog.restore_session = !dialog.restore_session; }
            KeyCode::Enter => { self.apply_config_dialog(); }
            _ => {}
        }
    }

    fn apply_config_dialog(&mut self) {
        if let Some(dialog) = self.config_dialog.take() {
            let name = Theme::all_names()[dialog.theme_index].to_string();
            self.theme = Theme::by_name(&name);
            self.config.theme = name;
            self.config.restore_session = dialog.restore_session;
            let _ = self.config.save();
        }
    }

    pub fn build_session(&self) -> Session {
        Session {
            left_tabs:    self.left_panel.tabs.iter().map(|t| t.path.to_string_lossy().to_string()).collect(),
            left_active:  self.left_panel.active_tab,
            right_tabs:   self.right_panel.tabs.iter().map(|t| t.path.to_string_lossy().to_string()).collect(),
            right_active: self.right_panel.active_tab,
            active_panel: if self.active_panel == PanelSide::Left { "left".to_string() } else { "right".to_string() },
        }
    }

    // ── Mouse handling ───────────────────────────────────────────────────────

    fn handle_mouse(&mut self, event: crossterm::event::MouseEvent) {
        let col = event.column;
        let row = event.row;

        match event.kind {
            MouseEventKind::Down(MouseButton::Left) => {
                // Config dialog
                if self.config_dialog.is_some() {
                    let (tl, tr, res, sav, ok, can) = {
                        let d = self.config_dialog.as_ref().unwrap();
                        (d.theme_left_rect, d.theme_right_rect, d.restore_rect,
                         d.save_rect, d.ok_rect, d.cancel_rect)
                    };
                    let themes = Theme::all_names();
                    if rect_contains(tl, col, row) {
                        if let Some(d) = &mut self.config_dialog {
                            if d.theme_index > 0 { d.theme_index -= 1; }
                        }
                    } else if rect_contains(tr, col, row) {
                        if let Some(d) = &mut self.config_dialog {
                            if d.theme_index + 1 < themes.len() { d.theme_index += 1; }
                        }
                    } else if rect_contains(res, col, row) {
                        if let Some(d) = &mut self.config_dialog {
                            d.restore_session = !d.restore_session;
                        }
                    } else if rect_contains(sav, col, row) {
                        let restore = self.config_dialog.as_ref().map(|d| d.restore_session).unwrap_or(false);
                        if restore {
                            let session = self.build_session();
                            if session.save().is_ok() {
                                self.status_msg = Some(format!("Session saved to: {}", Session::session_path().display()));
                            }
                        }
                    } else if rect_contains(ok, col, row) {
                        self.apply_config_dialog();
                    } else if rect_contains(can, col, row) {
                        self.config_dialog = None;
                    }
                    return;
                }
                // Stop-search confirmation overlay
                if self.search_stop_confirm {
                    if rect_contains(self.search_stop_keep_rect, col, row) {
                        self.stop_search(true);
                    } else if rect_contains(self.search_stop_discard_rect, col, row) {
                        self.stop_search(false);
                    } else {
                        self.search_stop_confirm = false;
                    }
                    return;
                }
                // Search dialog field focus
                if let Some(d) = &mut self.search_dialog {
                    if rect_contains(d.path_rect, col, row) { d.focused = SearchField::Path;     return; }
                    if rect_contains(d.name_rect, col, row) { d.focused = SearchField::Name;     return; }
                    if rect_contains(d.text_rect, col, row) { d.focused = SearchField::FindText; return; }
                    if rect_contains(d.hex_rect,  col, row) { d.focused = SearchField::FindHex;  return; }
                }
                // Goto paste menu
                if let Some(rect) = self.goto_paste_menu {
                    if rect_contains(rect, col, row) {
                        self.paste_into_goto();
                    } else {
                        self.goto_paste_menu = None;
                    }
                    return;
                }
                // SHA-256 copy button
                if matches!(&self.dialog, Some(Dialog::Sha256Result { .. })) {
                    if rect_contains(self.sha256_copy_btn_rect, col, row) {
                        if let Some(Dialog::Sha256Result { hash, filename }) = &self.dialog {
                            let text = format!("{}  {}", hash, filename);
                            if let Ok(mut board) = arboard::Clipboard::new() {
                                let _ = board.set_text(&text);
                            }
                            self.status_msg = Some("Hash copied to clipboard".to_string());
                        }
                        return;
                    }
                }
                // Context menu: click inside executes, outside closes
                if self.context_menu.is_some() {
                    let rect = self.context_menu.as_ref().unwrap().rect;
                    if rect_contains(rect, col, row) {
                        let item_row = row.saturating_sub(rect.y + 1) as usize;
                        if let Some(m) = self.context_menu.take() {
                            if item_row < m.items.len() {
                                if let Some((_, action)) = m.items.into_iter().nth(item_row) {
                                    self.execute_context_action(action);
                                }
                            }
                        }
                    } else {
                        self.context_menu = None;
                    }
                    return;
                }
                // Viewer: start text selection (viewer is full-screen, handle before anything else)
                if self.viewer.is_some() {
                    let rect = self.viewer_inner_rect;
                    if rect_contains(rect, col, row) {
                        let v = self.viewer.as_mut().unwrap();
                        let display_row = v.scroll + (row.saturating_sub(rect.y)) as usize;
                        if v.hex_mode {
                            v.select_start = Some((display_row, 0));
                            v.select_end   = Some((display_row, 0));
                        } else {
                            let char_col = (col.saturating_sub(rect.x + 6)) as usize;
                            v.select_start = Some((display_row, char_col));
                            v.select_end   = Some((display_row, char_col));
                        }
                        v.selecting = true;
                    }
                    return;
                }
                // History popup
                if self.history_popup.is_some() {
                    let rect = self.history_popup.as_ref().unwrap().rect;
                    if rect_contains(rect, col, row) {
                        let inner_row = row.saturating_sub(rect.y + 1) as usize;
                        let scroll = self.history_popup.as_ref().unwrap().scroll;
                        let idx = scroll + inner_row;
                        let len = self.history_popup.as_ref().unwrap().entries.len();
                        if idx < len {
                            if let Some(p) = &mut self.history_popup { p.selected = idx; }
                            self.navigate_to_history();
                        }
                    } else {
                        self.history_popup = None;
                    }
                    return;
                }
                // Bookmark popup
                if self.bookmark_popup.is_some() {
                    let rect = self.bookmark_popup.as_ref().unwrap().rect;
                    if rect_contains(rect, col, row) {
                        // Tab bar row (first inner row after border)
                        // Layout: inner starts at rect.x+1
                        //   " Left "  = cols inner+0..+5  (6 chars)
                        //   " │ "     = cols inner+6..+8  (3 chars)
                        //   " Right " = cols inner+9..+15 (7 chars)
                        if row == rect.y + 1 {
                            let inner_x = rect.x + 1;
                            if let Some(p) = &mut self.bookmark_popup {
                                if col >= inner_x && col < inner_x + 6 {
                                    p.target_panel = PanelSide::Left;
                                } else if col >= inner_x + 9 && col < inner_x + 16 {
                                    p.target_panel = PanelSide::Right;
                                }
                            }
                        } else {
                            // List area starts at row rect.y + 2 (border + tab row)
                            let inner_row = row.saturating_sub(rect.y + 2) as usize;
                            let scroll = self.bookmark_popup.as_ref().unwrap().scroll;
                            let idx = scroll + inner_row;
                            let len = self.bookmark_popup.as_ref().unwrap().entries.len();
                            if idx < len {
                                let is_double = self.is_double_click(col, row);
                                if let Some(p) = &mut self.bookmark_popup { p.selected = idx; }
                                if is_double {
                                    self.navigate_to_bookmark();
                                }
                            }
                        }
                    } else {
                        self.bookmark_popup = None;
                    }
                    return;
                }
                // File submenu
                if self.file_submenu_open {
                    if rect_contains(self.file_submenu_rect, col, row) {
                        let item_row = row.saturating_sub(self.file_submenu_rect.y + 1) as usize;
                        self.file_submenu_open = false;
                        self.show_menu = false;
                        // display: 0=Search,1=Log,2=sep,3=Clear settings,4=sep,5=Exit
                        let logical = if item_row >= 5 { item_row - 2 }
                                      else if item_row >= 3 { item_row - 1 }
                                      else { item_row };
                        match logical {
                            0 => { self.open_search_dialog(); }
                            1 => { self.log_popup = Some(LogPopup { selected: 0, scroll: 0 }); }
                            2 => { self.clear_settings(); }
                            3 => { self.running = false; }
                            _ => {}
                        }
                    } else {
                        self.file_submenu_open = false;
                        self.show_menu = false;
                    }
                    return;
                }
                // Panel filter submenu
                if self.panel_submenu_open {
                    if rect_contains(self.panel_submenu_rect, col, row) {
                        let item_row = row.saturating_sub(self.panel_submenu_rect.y + 1) as usize;
                        self.panel_submenu_open = false;
                        self.show_menu = false;
                        // display row 5 is separator; rows 6+ map to logical idx = row - 1
                        let logical = if item_row >= 6 { item_row - 1 } else { item_row };
                        self.execute_panel_submenu_action(logical);
                    } else {
                        self.panel_submenu_open = false;
                        self.show_menu = false;
                    }
                    return;
                }
                // Theme submenu: click inside selects, outside closes
                if self.submenu_open {
                    if rect_contains(self.submenu_rect, col, row) {
                        let item_row = row.saturating_sub(self.submenu_rect.y + 1) as usize;
                        let themes = Theme::all_names();
                        if item_row < themes.len() {
                            let name = themes[item_row].to_string();
                            self.theme = Theme::by_name(&name);
                            self.config.theme = name;
                            let _ = self.config.save();
                            self.submenu_open = false;
                            self.show_menu = false;
                        }
                    } else {
                        self.submenu_open = false;
                        self.show_menu = false;
                    }
                    return;
                }
                // Menu bar
                for (i, rect) in self.menu_item_rects.clone().iter().enumerate() {
                    if rect_contains(*rect, col, row) {
                        self.show_menu = true;
                        self.menu_index = i;
                        match i {
                            0 => {
                                self.file_submenu_index = 0;
                                self.file_submenu_open = true;
                            }
                            1 | 3 => {
                                self.panel_submenu_side = if i == 1 { PanelSide::Left } else { PanelSide::Right };
                                let filter_exec = if i == 1 { self.left_panel.filter_exec } else { self.right_panel.filter_exec };
                                self.panel_submenu_index = if filter_exec { 1 } else { 0 };
                                self.panel_submenu_open = true;
                            }
                            2 => {
                                self.open_config_dialog();
                                self.show_menu = false;
                            }
                            _ => {}
                        }
                        return;
                    }
                }
                // Bottom buttons
                for (i, rect) in self.button_rects.clone().iter().enumerate() {
                    if rect_contains(*rect, col, row) {
                        self.handle_button_click(i);
                        return;
                    }
                }
                // Tab label click (top border row)
                for (i, rect) in self.left_tab_rects.clone().iter().enumerate() {
                    if rect_contains(*rect, col, row) {
                        self.left_panel.active_tab = i;
                        self.active_panel = PanelSide::Left;
                        return;
                    }
                }
                for (i, rect) in self.right_tab_rects.clone().iter().enumerate() {
                    if rect_contains(*rect, col, row) {
                        self.right_panel.active_tab = i;
                        self.active_panel = PanelSide::Right;
                        return;
                    }
                }
                // Search results panel click
                let sr_rect = self.search_results.as_ref().map(|sr| match sr.side {
                    PanelSide::Left  => self.left_panel_rect,
                    PanelSide::Right => self.right_panel_rect,
                });
                if let Some(rect) = sr_rect {
                    if rect_contains(rect, col, row) {
                        let inner_row = row.saturating_sub(rect.y + 1) as usize;
                        let (scroll, len) = self.search_results.as_ref()
                            .map(|sr| (sr.scroll, sr.results.len())).unwrap_or((0, 0));
                        let idx = scroll + inner_row;
                        if let Some(sr) = &self.search_results { self.active_panel = sr.side.clone(); }
                        if idx < len {
                            if let Some(sr) = &mut self.search_results { sr.selected = idx; }
                            if self.is_double_click(col, row) {
                                self.open_search_result();
                                return;
                            }
                        }
                        self.last_click = Some((col, row, std::time::Instant::now()));
                        return;
                    }
                }
                // Path breadcrumb click (in panel title border)
                let left_path_click = self.left_path_rects.iter().find(|(r, _)| rect_contains(*r, col, row)).map(|(_, p)| p.clone());
                let right_path_click = self.right_path_rects.iter().find(|(r, _)| rect_contains(*r, col, row)).map(|(_, p)| p.clone());
                if let Some(path) = left_path_click {
                    self.active_panel = PanelSide::Left;
                    self.left_panel.navigate_to(path);
                    return;
                } else if let Some(path) = right_path_click {
                    self.active_panel = PanelSide::Right;
                    self.right_panel.navigate_to(path);
                    return;
                }
                // Sort indicator click (in panel title border)
                if rect_contains(self.left_sort_rect, col, row) {
                    self.active_panel = PanelSide::Left;
                    self.left_panel.cycle_sort();
                    return;
                } else if rect_contains(self.right_sort_rect, col, row) {
                    self.active_panel = PanelSide::Right;
                    self.right_panel.cycle_sort();
                    return;
                }
                // Left panel
                if rect_contains(self.left_panel_rect, col, row) {
                    self.active_panel = PanelSide::Left;
                    let panel_row = row.saturating_sub(self.left_panel_rect.y + 1) as usize;
                    let idx = self.left_panel.tab().scroll + panel_row;
                    if idx < self.left_panel.tab().entries.len() {
                        self.left_panel.tab_mut().selected = idx;
                        if self.is_double_click(col, row) { self.enter_selected(); return; }
                    }
                } else if rect_contains(self.right_panel_rect, col, row) {
                    self.active_panel = PanelSide::Right;
                    let panel_row = row.saturating_sub(self.right_panel_rect.y + 1) as usize;
                    let idx = self.right_panel.tab().scroll + panel_row;
                    if idx < self.right_panel.tab().entries.len() {
                        self.right_panel.tab_mut().selected = idx;
                        if self.is_double_click(col, row) { self.enter_selected(); return; }
                    }
                }
                self.last_click = Some((col, row, std::time::Instant::now()));
            }
            MouseEventKind::ScrollDown => {
                if let Some(ed) = &mut self.editor {
                    ed.move_down();
                } else if let Some(v) = &mut self.viewer {
                    let limit = v.total_display_lines.saturating_sub(1);
                    if v.scroll < limit { v.scroll += 1; }
                } else if let Some(sr) = &mut self.search_results {
                    let len = sr.results.len();
                    if sr.selected + 1 < len { sr.selected += 1; }
                } else {
                    self.active_panel_mut().move_down();
                }
            }
            MouseEventKind::ScrollUp => {
                if let Some(ed) = &mut self.editor {
                    ed.move_up();
                } else if let Some(v) = &mut self.viewer {
                    v.scroll = v.scroll.saturating_sub(1);
                } else if let Some(sr) = &mut self.search_results {
                    if sr.selected > 0 { sr.selected -= 1; }
                    if sr.selected < sr.scroll { sr.scroll = sr.selected; }
                } else {
                    self.active_panel_mut().move_up();
                }
            }
            MouseEventKind::Drag(MouseButton::Left) => {
                if let Some(v) = &mut self.viewer {
                    if v.selecting {
                        let rect = self.viewer_inner_rect;
                        let display_row = v.scroll + (row.saturating_sub(rect.y)) as usize;
                        if v.hex_mode {
                            v.select_end = Some((display_row, 0));
                        } else {
                            let char_col = (col.saturating_sub(rect.x + 6)) as usize;
                            v.select_end = Some((display_row, char_col));
                        }
                    }
                }
            }
            MouseEventKind::Up(MouseButton::Left) => {
                if let Some(v) = &mut self.viewer {
                    v.selecting = false;
                }
            }
            MouseEventKind::Down(MouseButton::Right) => {
                self.context_menu = None;
                if matches!(&self.dialog, Some(Dialog::Goto { .. })) {
                    // Show paste-only menu; rect is set during render
                    self.goto_paste_menu = Some(Rect::new(col, row, 9, 3));
                    return;
                }
                if self.viewer.is_some() {
                    let w = 22u16;
                    let h = 3u16;
                    let rect = Rect::new(col, row, w, h);
                    self.context_menu = Some(ContextMenu {
                        items: vec![("Copy".to_string(), ContextAction::CopyViewerSelection)],
                        selected: 0, x: col, y: row, rect,
                    });
                    return;
                }
                if self.editor.is_some() { return; }
                // Search results right-click
                let sr_rect = self.search_results.as_ref().map(|sr| match sr.side {
                    PanelSide::Left  => self.left_panel_rect,
                    PanelSide::Right => self.right_panel_rect,
                });
                if let Some(rect) = sr_rect {
                    if rect_contains(rect, col, row) {
                        let inner_row = row.saturating_sub(rect.y + 1) as usize;
                        let (scroll, len) = self.search_results.as_ref()
                            .map(|sr| (sr.scroll, sr.results.len())).unwrap_or((0, 0));
                        let idx = scroll + inner_row;
                        if idx < len {
                            if let Some(sr) = &mut self.search_results { sr.selected = idx; }
                            if let Some(sr) = &self.search_results { self.active_panel = sr.side.clone(); }
                            self.open_search_result_context_menu(col, row, idx);
                        }
                        return;
                    }
                }
                if rect_contains(self.left_panel_rect, col, row) {
                    self.active_panel = PanelSide::Left;
                    let panel_row = row.saturating_sub(self.left_panel_rect.y + 1) as usize;
                    let idx = self.left_panel.tab().scroll + panel_row;
                    if idx < self.left_panel.tab().entries.len() {
                        self.left_panel.tab_mut().selected = idx;
                    }
                    self.open_context_menu(col, row);
                } else if rect_contains(self.right_panel_rect, col, row) {
                    self.active_panel = PanelSide::Right;
                    let panel_row = row.saturating_sub(self.right_panel_rect.y + 1) as usize;
                    let idx = self.right_panel.tab().scroll + panel_row;
                    if idx < self.right_panel.tab().entries.len() {
                        self.right_panel.tab_mut().selected = idx;
                    }
                    self.open_context_menu(col, row);
                }
            }
            _ => {}
        }
    }

    fn handle_button_click(&mut self, idx: usize) {
        self.status_msg = None;
        let command = match self.config.buttons.get(idx) {
            Some(btn) => btn.command.clone(),
            None => return,
        };
        match command.as_str() {
            "quit"   => self.running = false,
            "menu"   => self.show_menu = !self.show_menu,
            "view"   => self.open_viewer(),
            "copy"   => self.open_copy_dialog(),
            "move"   => self.open_move_dialog(),
            "mkdir"  => self.open_mkdir_dialog(),
            "delete" => self.open_delete_dialog(),
            "history"  => self.open_history_popup(),
            "bookmark" => self.open_bookmark_popup(),
            "search"   => self.open_search_dialog(),
            "edit"     => self.open_editor(),
            _ => {}
        }
    }

    // ── Actions ──────────────────────────────────────────────────────────────

    fn toggle_panel(&mut self) {
        self.active_panel = match self.active_panel {
            PanelSide::Left  => PanelSide::Right,
            PanelSide::Right => PanelSide::Left,
        };
    }

    fn enter_selected(&mut self) {
        self.active_panel_mut().enter_selected();
    }

    fn enter_selected_filtered(&mut self) {
        // When a filter is active, `selected` indexes into the filtered list.
        // Resolve it back to the real entry, then navigate.
        let real_name = {
            let tab = self.active_panel().tab();
            if tab.filter.is_empty() {
                tab.entries.get(tab.selected).map(|e| e.name.clone())
            } else {
                tab.filtered_entries().get(tab.selected).map(|e| e.name.clone())
            }
        };
        if let Some(name) = real_name {
            let is_dir = {
                let tab = self.active_panel().tab();
                tab.entries.iter().find(|e| e.name == name).map(|e| e.is_dir).unwrap_or(false)
            };
            if is_dir {
                // Navigate — clear filter first
                {
                    let tab = self.active_panel_mut().tab_mut();
                    tab.filter.clear();
                    tab.filter_active = false;
                    tab.selected = tab.entries.iter().position(|e| e.name == name).unwrap_or(0);
                }
                self.active_panel_mut().enter_selected();
            } else {
                // File: open viewer/editor via normal path
                self.active_panel_mut().tab_mut().selected =
                    self.active_panel().tab().entries.iter().position(|e| e.name == name).unwrap_or(0);
                self.active_panel_mut().enter_selected();
            }
        }
    }

    fn open_viewer(&mut self) {
        let path = {
            let panel = self.active_panel();
            let entry = match panel.tab().entries.get(panel.tab().selected) { Some(e) => e, None => return };
            if entry.is_dir || entry.name == ".." { return; }
            panel.tab().path.join(&entry.name)
        };
        match fs::read(&path) {
            Ok(raw_bytes) => {
                let (lines, hex_mode) = match String::from_utf8(raw_bytes.clone()) {
                    Ok(text) => (text.lines().map(|l| l.to_string()).collect(), false),
                    Err(_)   => (Vec::new(), true),
                };
                self.viewer = Some(ViewerState {
                    path,
                    lines,
                    raw_bytes,
                    hex_mode,
                    scroll: 0,
                    wrap: false,
                    total_display_lines: 0,
                    text_width: 0,
                    select_start: None,
                    select_end: None,
                    selecting: false,
                    message: None,
                });
            }
            Err(_) => self.status_msg = Some("Cannot read file (no permission)".to_string()),
        }
    }

    fn open_editor(&mut self) {
        let path = {
            let panel = self.active_panel();
            let entry = match panel.tab().entries.get(panel.tab().selected) { Some(e) => e, None => return };
            if entry.is_dir || entry.name == ".." { return; }
            panel.tab().path.join(&entry.name)
        };
        match EditorState::open(path) {
            Ok(state) => self.editor = Some(state),
            Err(e) => self.status_msg = Some(format!("Cannot open file: {}", e)),
        }
    }

    fn handle_editor_key(&mut self, key: KeyCode, modifiers: KeyModifiers) {
        let ctrl = modifiers.contains(KeyModifiers::CONTROL);

        // Confirm-close prompt active (dirty file, Esc was pressed)
        if let Some(ed) = &self.editor {
            if ed.confirm_close {
                match key {
                    KeyCode::Char('y') | KeyCode::Char('Y') => { self.editor = None; return; }
                    _ => {
                        if let Some(ed) = &mut self.editor { ed.confirm_close = false; }
                        return;
                    }
                }
            }
        }

        match key {
            // Save
            KeyCode::Char('s') if ctrl => {
                if let Some(ed) = &mut self.editor { ed.save(); }
            }
            // Copy line
            KeyCode::Char('c') if ctrl => {
                if let Some(ed) = &mut self.editor { ed.copy_line(); }
            }
            // Cut line
            KeyCode::Char('x') if ctrl => {
                if let Some(ed) = &mut self.editor { ed.cut_line(); }
            }
            // Paste
            KeyCode::Char('v') if ctrl => {
                if let Some(ed) = &mut self.editor { ed.paste(); }
            }
            // Close
            KeyCode::Esc | KeyCode::F(4) => {
                if let Some(ed) = &self.editor {
                    if ed.dirty {
                        if let Some(ed) = &mut self.editor { ed.confirm_close = true; }
                    } else {
                        self.editor = None;
                    }
                }
            }
            // Navigation
            KeyCode::Up    => { if let Some(ed) = &mut self.editor { ed.move_up(); } }
            KeyCode::Down  => { if let Some(ed) = &mut self.editor { ed.move_down(); } }
            KeyCode::Left  => { if let Some(ed) = &mut self.editor { ed.move_left(); } }
            KeyCode::Right => { if let Some(ed) = &mut self.editor { ed.move_right(); } }
            KeyCode::Home  => { if let Some(ed) = &mut self.editor { ed.move_home(); } }
            KeyCode::End   => { if let Some(ed) = &mut self.editor { ed.move_end(); } }
            KeyCode::PageUp   => { if let Some(ed) = &mut self.editor { ed.page_up(20); } }
            KeyCode::PageDown => { if let Some(ed) = &mut self.editor { ed.page_down(20); } }
            // Editing
            KeyCode::Enter     => { if let Some(ed) = &mut self.editor { ed.insert_newline(); } }
            KeyCode::Backspace => { if let Some(ed) = &mut self.editor { ed.backspace(); } }
            KeyCode::Delete    => { if let Some(ed) = &mut self.editor { ed.delete_char(); } }
            KeyCode::Tab       => { if let Some(ed) = &mut self.editor { for _ in 0..4 { ed.insert_char(' '); } } }
            KeyCode::Char(c) if !ctrl => {
                if let Some(ed) = &mut self.editor { ed.insert_char(c); }
            }
            _ => {}
        }
    }

    fn open_copy_dialog(&mut self) {
        let sources = self.active_panel().effective_targets();
        if sources.is_empty() { return; }
        let dest = self.inactive_panel().tab().path.display().to_string();
        self.dialog = Some(Dialog::Copy { sources, dest_input: dest });
    }

    fn open_move_dialog(&mut self) {
        let sources = self.active_panel().effective_targets();
        if sources.is_empty() { return; }
        let dest = if sources.len() == 1 {
            let fname = sources[0].file_name().unwrap_or_default().to_string_lossy();
            format!("{}/{}", self.inactive_panel().tab().path.display(), fname)
        } else {
            self.inactive_panel().tab().path.display().to_string()
        };
        self.dialog = Some(Dialog::Move { sources, dest_input: dest });
    }

    fn open_mkdir_dialog(&mut self) {
        self.dialog = Some(Dialog::Mkdir { input: String::new() });
    }

    fn open_delete_dialog(&mut self) {
        let targets = self.active_panel().effective_targets();
        if targets.is_empty() { return; }
        self.dialog = Some(Dialog::Delete { targets });
    }

    fn execute_mkdir(&mut self, name: &str) {
        let name = name.trim();
        if name.is_empty() { self.status_msg = Some("Name cannot be empty".to_string()); return; }
        let path = self.active_panel().tab().path.join(name);
        match fs::create_dir_all(&path) {
            Ok(_)  => {
                let _ = self.active_panel_mut().load_entries();
                self.status_msg = Some(format!("Created: {}", name));
                self.log_op("MKD", clean_path(&path), None);
            }
            Err(e) => self.status_msg = Some(format!("Error: {}", e)),
        }
    }

    fn execute_new_file(&mut self, name: &str, dir: &Path) {
        let name = name.trim();
        if name.is_empty() { self.status_msg = Some("Name cannot be empty".to_string()); return; }
        let path = dir.join(name);
        match fs::File::create(&path) {
            Ok(_) => {
                // Reload whichever panel owns this directory
                for panel in [&mut self.left_panel, &mut self.right_panel] {
                    if panel.tab_mut().path == dir { let _ = panel.load_entries(); }
                }
                // Select the new file in the owning panel
                let name_str = name.to_string();
                for panel in [&mut self.left_panel, &mut self.right_panel] {
                    if panel.tab_mut().path == dir {
                        if let Some(idx) = panel.tab().entries.iter().position(|e| e.name == name_str) {
                            panel.tab_mut().selected = idx;
                        }
                    }
                }
                self.log_op("NEW", clean_path(&path), None);
                self.status_msg = Some(format!("Created: {}", name));
            }
            Err(e) => self.status_msg = Some(format!("Error: {}", e)),
        }
    }

    fn execute_copy(&mut self, sources: &[PathBuf], dest_str: &str) {
        let dest_dir = PathBuf::from(dest_str.trim());
        let mut errors = 0;
        for source in sources {
            let dest = if sources.len() == 1 && !dest_dir.is_dir() {
                dest_dir.clone()
            } else {
                dest_dir.join(source.file_name().unwrap_or_default())
            };
            let r = if source.is_dir() {
                copy_dir_recursive(source, &dest)
            } else {
                fs::copy(source, &dest).map(|_| ()).map_err(anyhow::Error::from)
            };
            if r.is_err() { errors += 1; }
        }
        self.reload_after_op();
        if errors == 0 {
            let src = if sources.len() == 1 {
                clean_path(&sources[0])
            } else {
                format!("{} items", sources.len())
            };
            self.log_op("COPY", src, Some(dest_str.trim().to_string()));
            self.status_msg = Some(format!("Copied {} item(s)", sources.len()));
        } else {
            self.status_msg = Some(format!("Copy: {} error(s)", errors));
        }
    }

    fn execute_move(&mut self, sources: &[PathBuf], dest_str: &str) {
        let dest_dir = PathBuf::from(dest_str.trim());
        let mut errors = 0;
        for source in sources {
            let dest = if sources.len() == 1 && !dest_dir.is_dir() {
                dest_dir.clone()
            } else {
                dest_dir.join(source.file_name().unwrap_or_default())
            };
            if fs::rename(source, &dest).is_err() {
                // Cross-device fallback: copy then delete
                let ok = if source.is_dir() {
                    copy_dir_recursive(source, &dest).is_ok()
                } else {
                    fs::copy(source, &dest).is_ok()
                };
                if ok {
                    let _ = if source.is_dir() { fs::remove_dir_all(source) } else { fs::remove_file(source) };
                } else {
                    errors += 1;
                }
            }
        }
        self.reload_after_op();
        if errors == 0 {
            let src = if sources.len() == 1 {
                clean_path(&sources[0])
            } else {
                format!("{} items", sources.len())
            };
            self.log_op("MOV", src, Some(dest_str.trim().to_string()));
            self.status_msg = Some(format!("Moved {} item(s)", sources.len()));
        } else {
            self.status_msg = Some(format!("Move: {} error(s)", errors));
        }
    }

    fn execute_delete(&mut self, targets: &[PathBuf]) {
        let mut errors = 0;
        for path in targets {
            let r = if path.is_dir() { fs::remove_dir_all(path) } else { fs::remove_file(path) };
            if r.is_err() { errors += 1; }
        }
        self.reload_after_op();
        if errors == 0 {
            let src = if targets.len() == 1 {
                clean_path(&targets[0])
            } else {
                format!("{} items", targets.len())
            };
            self.log_op("DEL", src, None);
            self.status_msg = Some(format!("Deleted {} item(s)", targets.len()));
        } else {
            self.status_msg = Some(format!("Delete: {} error(s)", errors));
        }
    }

    fn execute_goto(&mut self, path_str: &str, side: PanelSide) {
        let path_str = path_str.trim();
        if path_str.is_empty() { return; }
        let path = PathBuf::from(path_str);
        let dir = if path.is_dir() {
            path
        } else if path.is_file() {
            path.parent().map(|p| p.to_path_buf()).unwrap_or(path)
        } else {
            self.status_msg = Some(format!("Path not found: {}", path_str));
            return;
        };
        self.active_panel = side.clone();
        let panel = match side {
            PanelSide::Left  => &mut self.left_panel,
            PanelSide::Right => &mut self.right_panel,
        };
        panel.tab_mut().path = dir;
        panel.tab_mut().selected = 0;
        panel.tab_mut().scroll = 0;
        let _ = panel.load_entries();
        panel.record_visit();
    }

    fn execute_rename(&mut self, path: &Path, new_name: &str) {
        let new_name = new_name.trim();
        if new_name.is_empty() { self.status_msg = Some("Name cannot be empty".to_string()); return; }
        let dest = path.parent().unwrap_or(Path::new(".")).join(new_name);
        match fs::rename(path, &dest) {
            Ok(_) => {
                self.log_op("REN", clean_path(path), Some(new_name.to_string()));
                let _ = self.left_panel.load_entries();
                let _ = self.right_panel.load_entries();
                self.status_msg = Some(format!("Renamed to: {}", new_name));
            }
            Err(e) => self.status_msg = Some(format!("Rename error: {}", e)),
        }
    }

    fn log_op(&mut self, op: &'static str, src: String, dest: Option<String>) {
        self.op_log.insert(0, LogEntry { time: current_time_str(), op, src, dest });
        self.op_log.truncate(250);
    }

    fn reload_after_op(&mut self) {
        self.left_panel.clear_marks();
        self.right_panel.clear_marks();
        let _ = self.left_panel.load_entries();
        let _ = self.right_panel.load_entries();
    }

    fn run_interactive_shell<B: Backend>(&mut self, terminal: &mut Terminal<B>) -> Result<()> {
        disable_raw_mode()?;
        execute!(io::stdout(), LeaveAlternateScreen, DisableMouseCapture)?;

        println!("\x1b[33m");
        println!(r"  ____  _     _ _");
        println!(r" |  _ \| |__ (_) | ___ ___  _ __ ___");
        println!(r" | |_) | '_ \| | |/ __/ _ \| '_ ` _ \");
        println!(r" |  __/| | | | | | (_| (_) | | | | | |");
        println!(r" |_|   |_| |_|_|_|\___\___/|_| |_| |_|");
        println!("\x1b[0m  Type \x1b[1mexit\x1b[0m to return.\n");

        #[cfg(windows)]
        let _ = std::process::Command::new("cmd").status();
        #[cfg(not(windows))]
        {
            let shell = std::env::var("SHELL").unwrap_or_else(|_| "sh".to_string());
            let _ = std::process::Command::new(&shell).status();
        }

        enable_raw_mode()?;
        execute!(io::stdout(), EnterAlternateScreen, EnableMouseCapture)?;
        terminal.clear()?;

        let _ = self.left_panel.load_entries();
        let _ = self.right_panel.load_entries();
        Ok(())
    }

    fn run_shell_command<B: Backend>(&mut self, terminal: &mut Terminal<B>, cmd: &str) -> Result<()> {
        disable_raw_mode()?;
        execute!(io::stdout(), LeaveAlternateScreen, DisableMouseCapture)?;

        #[cfg(windows)]
        let _ = std::process::Command::new("cmd").args(["/C", cmd]).status();
        #[cfg(not(windows))]
        let _ = std::process::Command::new("sh").arg("-c").arg(cmd).status();

        print!("\n\x1b[33m[philcom]\x1b[0m Press Enter to return...");
        io::stdout().flush()?;

        // Re-enable raw mode to reliably capture keypress on all platforms
        // (io::stdin().read_line() is unreliable on Windows after crossterm raw mode changes)
        enable_raw_mode()?;
        loop {
            if let Ok(Event::Key(key)) = event::read() {
                if key.kind == KeyEventKind::Press
                    && matches!(key.code, KeyCode::Enter | KeyCode::Esc)
                {
                    break;
                }
            }
        }

        execute!(io::stdout(), EnterAlternateScreen, EnableMouseCapture)?;
        terminal.clear()?;

        let _ = self.left_panel.load_entries();
        let _ = self.right_panel.load_entries();
        Ok(())
    }

    fn print_search_results<B: Backend>(&mut self, terminal: &mut Terminal<B>) -> Result<()> {
        let (results, summary) = match &self.search_results {
            Some(sr) => (sr.results.iter().map(|r| {
                let path = r.path.display().to_string();
                match &r.kind {
                    SearchResultKind::NameMatch => path,
                    SearchResultKind::TextMatch { line_num, line } =>
                        format!("{}:{}: {}", path, line_num, line.trim()),
                    SearchResultKind::HexMatch { offset } =>
                        format!("{} @ 0x{:X}", path, offset),
                }
            }).collect::<Vec<_>>(), sr.summary.clone()),
            None => return Ok(()),
        };

        disable_raw_mode()?;
        execute!(io::stdout(), LeaveAlternateScreen, DisableMouseCapture)?;

        println!("\x1b[33m[philcom]\x1b[0m Search: {}  ({} result(s))\n", summary, results.len());
        for line in &results {
            println!("{}", line);
        }

        print!("\n\x1b[33m[philcom]\x1b[0m Press Enter to return...");
        io::stdout().flush()?;

        enable_raw_mode()?;
        loop {
            if let Ok(Event::Key(key)) = event::read() {
                if key.kind == KeyEventKind::Press
                    && matches!(key.code, KeyCode::Enter | KeyCode::Esc)
                {
                    break;
                }
            }
        }

        execute!(io::stdout(), EnterAlternateScreen, EnableMouseCapture)?;
        terminal.clear()?;
        Ok(())
    }

    // ── Panel accessors ──────────────────────────────────────────────────────

    pub fn active_panel(&self) -> &Panel {
        match self.active_panel { PanelSide::Left => &self.left_panel, PanelSide::Right => &self.right_panel }
    }

    pub fn active_panel_mut(&mut self) -> &mut Panel {
        match self.active_panel { PanelSide::Left => &mut self.left_panel, PanelSide::Right => &mut self.right_panel }
    }

    pub fn inactive_panel(&self) -> &Panel {
        match self.active_panel { PanelSide::Left => &self.right_panel, PanelSide::Right => &self.left_panel }
    }

    fn execute_cd(&mut self, arg: &str) {
        let arg = arg.trim();

        // Resolve path: empty → home, ~ prefix → expand, relative → join with panel path
        let raw: PathBuf = if arg.is_empty() {
            dirs::home_dir().unwrap_or_else(|| PathBuf::from("."))
        } else if arg.starts_with('~') {
            let rest = arg[1..].trim_start_matches('/').trim_start_matches('\\');
            let home = dirs::home_dir().unwrap_or_else(|| PathBuf::from("."));
            if rest.is_empty() { home } else { home.join(rest) }
        } else {
            let p = PathBuf::from(arg);
            if p.is_absolute() { p } else { self.active_panel().tab().path.join(p) }
        };

        // Decide: file → go to parent and select it; dir → go there; missing → try parent
        let (dir, select): (PathBuf, Option<String>) = if raw.is_file() {
            let parent = raw.parent().unwrap_or(Path::new(".")).to_path_buf();
            let name = raw.file_name().map(|n| n.to_string_lossy().into_owned());
            (parent, name)
        } else if raw.is_dir() {
            (raw, None)
        } else {
            // Not found — try treating the last segment as a filename
            match raw.parent() {
                Some(parent) if parent.is_dir() => {
                    let name = raw.file_name().map(|n| n.to_string_lossy().into_owned());
                    (parent.to_path_buf(), name)
                }
                _ => {
                    self.status_msg = Some(format!("Not found: {}", clean_path(&raw)));
                    return;
                }
            }
        };

        let dir = dir.canonicalize().unwrap_or(dir);
        let panel = self.active_panel_mut();
        panel.tab_mut().path = dir;
        let _ = panel.load_entries();
        panel.record_visit();
        panel.tab_mut().scroll = 0;
        panel.tab_mut().selected = select
            .and_then(|name| panel.tab().entries.iter().position(|e| e.name == name))
            .unwrap_or(0);
    }

    fn navigate_panel_to_downloads(&mut self) {
        match dirs::download_dir() {
            Some(path) => {
                let panel = match self.panel_submenu_side {
                    PanelSide::Left  => &mut self.left_panel,
                    PanelSide::Right => &mut self.right_panel,
                };
                self.active_panel = self.panel_submenu_side.clone();
                panel.tab_mut().path = path;
                panel.tab_mut().selected = 0;
                panel.tab_mut().scroll = 0;
                let _ = panel.load_entries();
                panel.record_visit();
            }
            None => self.status_msg = Some("Downloads folder not found on this system".to_string()),
        }
    }

    fn open_history_popup(&mut self) {
        self.context_menu = None;
        let left  = &self.left_panel.tab().visited;
        let right = &self.right_panel.tab().visited;
        let mut entries: Vec<(PathBuf, bool)> = Vec::new();
        let mut seen = std::collections::HashSet::new();
        let max = left.len().max(right.len());
        for i in 0..max {
            if let Some(p) = left.get(i) {
                if seen.insert(p.clone()) { entries.push((p.clone(), true)); }
            }
            if let Some(p) = right.get(i) {
                if seen.insert(p.clone()) { entries.push((p.clone(), false)); }
            }
        }
        self.history_popup = Some(HistoryPopup {
            entries,
            selected: 0,
            scroll: 0,
            rect: Rect::default(),
        });
    }

    fn handle_history_popup_key(&mut self, key: KeyCode) {
        let len = match &self.history_popup { Some(p) => p.entries.len(), None => return };
        match key {
            KeyCode::Esc => { self.history_popup = None; }
            KeyCode::Up => {
                if let Some(p) = &mut self.history_popup {
                    if p.selected > 0 { p.selected -= 1; }
                    if p.selected < p.scroll { p.scroll = p.selected; }
                }
            }
            KeyCode::Down => {
                if let Some(p) = &mut self.history_popup {
                    if p.selected + 1 < len { p.selected += 1; }
                }
            }
            KeyCode::Enter => { self.navigate_to_history(); }
            _ => {}
        }
    }

    fn navigate_to_history(&mut self) {
        let path = match self.history_popup.take() {
            Some(p) => p.entries.into_iter().nth(p.selected).map(|(path, _)| path),
            None => return,
        };
        if let Some(path) = path {
            let panel = match self.active_panel {
                PanelSide::Left  => &mut self.left_panel,
                PanelSide::Right => &mut self.right_panel,
            };
            panel.tab_mut().path = path;
            panel.tab_mut().selected = 0;
            panel.tab_mut().scroll = 0;
            let _ = panel.load_entries();
            panel.record_visit();
        }
    }

    fn open_bookmark_popup(&mut self) {
        self.context_menu = None;
        self.history_popup = None;
        let entries = self.config.bookmarks.clone();
        if entries.is_empty() {
            self.status_msg = Some("No bookmarks. Use Ctrl+D to add current directory.".to_string());
            return;
        }
        self.bookmark_popup = Some(BookmarkPopup {
            entries,
            selected: 0,
            target_panel: self.active_panel.clone(),
            scroll: 0,
            rect: Rect::default(),
        });
    }

    fn handle_bookmark_popup_key(&mut self, key: KeyCode) {
        let len = match &self.bookmark_popup { Some(p) => p.entries.len(), None => return };
        match key {
            KeyCode::Esc => { self.bookmark_popup = None; }
            KeyCode::Up => {
                if let Some(p) = &mut self.bookmark_popup {
                    if p.selected > 0 { p.selected -= 1; }
                    if p.selected < p.scroll { p.scroll = p.selected; }
                }
            }
            KeyCode::Down => {
                if let Some(p) = &mut self.bookmark_popup {
                    if p.selected + 1 < len { p.selected += 1; }
                }
            }
            KeyCode::Tab => {
                if let Some(p) = &mut self.bookmark_popup {
                    p.target_panel = match p.target_panel {
                        PanelSide::Left  => PanelSide::Right,
                        PanelSide::Right => PanelSide::Left,
                    };
                }
            }
            KeyCode::Delete => { self.remove_selected_bookmark(); }
            KeyCode::Enter  => { self.navigate_to_bookmark(); }
            _ => {}
        }
    }

    fn navigate_to_bookmark(&mut self) {
        let (path_str, target) = match self.bookmark_popup.take() {
            Some(p) => (p.entries.into_iter().nth(p.selected), p.target_panel),
            None => return,
        };
        if let Some(s) = path_str {
            let panel = match target {
                PanelSide::Left  => &mut self.left_panel,
                PanelSide::Right => &mut self.right_panel,
            };
            panel.tab_mut().path = PathBuf::from(s);
            panel.tab_mut().selected = 0;
            panel.tab_mut().scroll = 0;
            let _ = panel.load_entries();
            panel.record_visit();
        }
    }

    fn add_bookmark(&mut self) {
        let path = clean_path(&self.active_panel().tab().path);
        if !self.config.bookmarks.contains(&path) {
            self.config.bookmarks.push(path.clone());
            let _ = self.config.save();
            self.status_msg = Some(format!("Bookmarked: {}", path));
        } else {
            self.status_msg = Some(format!("Already bookmarked: {}", path));
        }
    }

    fn remove_selected_bookmark(&mut self) {
        let idx = match &self.bookmark_popup { Some(p) => p.selected, None => return };
        if idx < self.config.bookmarks.len() {
            self.config.bookmarks.remove(idx);
            let _ = self.config.save();
        }
        if let Some(p) = &mut self.bookmark_popup {
            let new_len = self.config.bookmarks.len();
            p.entries = self.config.bookmarks.clone();
            if new_len == 0 { self.bookmark_popup = None; return; }
            if p.selected >= new_len { p.selected = new_len - 1; }
        }
    }

    fn open_drive_list_popup(&mut self, panel: PanelSide) {
        self.context_menu  = None;
        self.history_popup = None;
        self.bookmark_popup = None;
        let drives = list_drives();
        if drives.is_empty() {
            self.status_msg = Some("No drives found".to_string());
            return;
        }
        self.drive_list_popup = Some(DriveListPopup {
            drives,
            selected: 0,
            panel,
            rect: Rect::default(),
        });
    }

    fn handle_drive_list_popup_key(&mut self, key: KeyCode) {
        let len = match &self.drive_list_popup { Some(p) => p.drives.len(), None => return };
        match key {
            KeyCode::Esc => { self.drive_list_popup = None; }
            KeyCode::Up => {
                if let Some(p) = &mut self.drive_list_popup {
                    if p.selected > 0 { p.selected -= 1; }
                }
            }
            KeyCode::Down => {
                if let Some(p) = &mut self.drive_list_popup {
                    if p.selected + 1 < len { p.selected += 1; }
                }
            }
            KeyCode::Enter => { self.navigate_to_drive(); }
            _ => {}
        }
    }

    fn navigate_to_drive(&mut self) {
        let (root, panel_side) = match self.drive_list_popup.take() {
            Some(p) => {
                let root = p.drives.into_iter().nth(p.selected).map(|d| d.root);
                (root, p.panel)
            }
            None => return,
        };
        if let Some(root) = root {
            let panel = match panel_side {
                PanelSide::Left  => &mut self.left_panel,
                PanelSide::Right => &mut self.right_panel,
            };
            panel.tab_mut().path = PathBuf::from(&root);
            panel.tab_mut().selected = 0;
            panel.tab_mut().scroll = 0;
            let _ = panel.load_entries();
            panel.record_visit();
        }
    }

    fn open_context_menu(&mut self, x: u16, y: u16) {
        self.show_menu = false;
        self.submenu_open = false;
        self.file_submenu_open = false;
        self.panel_submenu_open = false;
        self.log_popup = None;
        self.history_popup = None;
        self.bookmark_popup = None;
        let (path, mark_item, is_file) = {
            let panel = self.active_panel();
            match panel.tab().entries.get(panel.tab().selected) {
                Some(e) if e.name != ".." => {
                    let is_marked = panel.tab().marked.contains(&e.name);
                    let label = if is_marked { "Unmark" } else { "Mark" };
                    (panel.tab().path.join(&e.name), Some((label.to_string(), e.name.clone())), !e.is_dir)
                }
                _ => (panel.tab().path.to_path_buf(), None, false),
            }
        };
        let delete_path = if mark_item.is_some() { Some(path.clone()) } else { None };
        let rename_path = if mark_item.is_some() { Some(path.clone()) } else { None };
        let sha256_path = if is_file { Some(path.clone()) } else { None };
        let panel_dir = {
            let panel = self.active_panel();
            panel.tab().path.clone()
        };
        let reveal_path = path.clone();
        let mut items = vec![
            ("Copy path".to_string(),     ContextAction::CopyPath(path.clone())),
            ("Copy filename".to_string(), ContextAction::CopyFilename(path)),
        ];
        if let Some((label, name)) = mark_item {
            items.push((label, ContextAction::ToggleMark(name)));
        }
        if let Some(p) = rename_path {
            items.push(("Rename".to_string(), ContextAction::Rename(p)));
        }
        if let Some(p) = delete_path {
            items.push(("Delete".to_string(), ContextAction::DeleteItem(p)));
        }
        if let Some(p) = sha256_path {
            items.push(("SHA-256".to_string(), ContextAction::CalcSha256(p)));
        }
        items.push(("New file".to_string(), ContextAction::NewFile(panel_dir)));
        items.push((reveal_in_file_manager_label(), ContextAction::RevealInFileManager(reveal_path)));
        self.context_menu = Some(ContextMenu {
            items,
            selected: 0,
            x,
            y,
            rect: Rect::default(),
        });
    }

    fn execute_context_action(&mut self, action: ContextAction) {
        match action {
            ContextAction::CopyPath(path) => {
                let s = clean_path(&path);
                if let Ok(mut board) = arboard::Clipboard::new() {
                    let _ = board.set_text(&s);
                }
                self.status_msg = Some(format!("Copied: {}", s));
            }
            ContextAction::ToggleMark(name) => {
                let panel = self.active_panel_mut();
                if panel.tab().marked.contains(&name) {
                    panel.tab_mut().marked.remove(&name);
                } else {
                    panel.tab_mut().marked.insert(name);
                }
                let count = panel.tab().marked.len();
                self.status_msg = if count > 0 {
                    Some(format!("{} item(s) marked", count))
                } else {
                    None
                };
            }
            ContextAction::DeleteItem(path) => {
                self.dialog = Some(Dialog::Delete { targets: vec![path] });
            }
            ContextAction::CopyFilename(path) => {
                let name = path.file_name()
                    .map(|n| n.to_string_lossy().to_string())
                    .unwrap_or_else(|| clean_path(&path));
                if let Ok(mut board) = arboard::Clipboard::new() {
                    let _ = board.set_text(&name);
                }
                self.status_msg = Some(format!("Copied: {}", name));
            }
            ContextAction::NewFile(dir) => {
                self.dialog = Some(Dialog::NewFile { input: String::new(), dir });
            }
            ContextAction::Rename(path) => {
                let current = path.file_name()
                    .map(|n| n.to_string_lossy().to_string())
                    .unwrap_or_default();
                let cursor = current.chars().count();
                self.dialog = Some(Dialog::Rename { path, input: current, cursor });
            }
            ContextAction::CalcSha256(path) => {
                let filename = path.file_name()
                    .map(|n| n.to_string_lossy().to_string())
                    .unwrap_or_else(|| path.display().to_string());
                match fs::File::open(&path) {
                    Ok(mut file) => {
                        let mut hasher = Sha256::new();
                        let mut buf = [0u8; 65536];
                        loop {
                            match file.read(&mut buf) {
                                Ok(0) => break,
                                Ok(n) => hasher.update(&buf[..n]),
                                Err(e) => {
                                    self.status_msg = Some(format!("SHA-256 error: {}", e));
                                    return;
                                }
                            }
                        }
                        let hash = format!("{:x}", hasher.finalize());
                        self.dialog = Some(Dialog::Sha256Result { filename, hash });
                    }
                    Err(e) => {
                        self.status_msg = Some(format!("SHA-256 error: {}", e));
                    }
                }
            }
            ContextAction::ViewFile(path, line_num) => {
                match fs::read(&path) {
                    Ok(raw_bytes) => {
                        let (lines, hex_mode) = match String::from_utf8(raw_bytes.clone()) {
                            Ok(text) => (text.lines().map(|l| l.to_string()).collect(), false),
                            Err(_)   => (Vec::new(), true),
                        };
                        let scroll = line_num.map(|n| n.saturating_sub(1)).unwrap_or(0);
                        self.viewer = Some(ViewerState {
                            path, lines, raw_bytes, hex_mode, scroll,
                            wrap: false, total_display_lines: 0, text_width: 0,
                            select_start: None, select_end: None, selecting: false, message: None,
                        });
                    }
                    Err(_) => self.status_msg = Some("Cannot read file".to_string()),
                }
            }
            ContextAction::GoToFolder(path) => {
                let dir = path.parent().map(|p| p.to_path_buf()).unwrap_or(path);
                let panel = match self.active_panel {
                    PanelSide::Left  => &mut self.left_panel,
                    PanelSide::Right => &mut self.right_panel,
                };
                panel.tab_mut().path = dir;
                panel.tab_mut().selected = 0;
                panel.tab_mut().scroll = 0;
                let _ = panel.load_entries();
                panel.record_visit();
                self.search_results = None;
            }
            ContextAction::CopyToPanel(path) => {
                let dest = self.inactive_panel().tab().path.display().to_string();
                self.dialog = Some(Dialog::Copy { sources: vec![path], dest_input: dest });
            }
            ContextAction::CopyViewerSelection => {
                self.copy_viewer_selection();
            }
            ContextAction::RevealInFileManager(path) => {
                reveal_in_file_manager(&path);
            }
        }
    }

    fn copy_viewer_selection(&mut self) {
        let text = {
            let v = match &self.viewer { Some(v) => v, None => return };
            let (start, end) = match (v.select_start, v.select_end) {
                (Some(s), Some(e)) => (s.min(e), s.max(e)),
                _ => return,
            };
            if v.hex_mode {
                const BPR: usize = 16;
                let byte_start = (start.0 * BPR).min(v.raw_bytes.len());
                let byte_end   = ((end.0 + 1) * BPR).min(v.raw_bytes.len());
                if byte_start >= byte_end { return; }
                v.raw_bytes[byte_start..byte_end]
                    .iter()
                    .map(|b| format!("{:02X}", b))
                    .collect::<Vec<_>>()
                    .join(" ")
            } else {
                let width = if v.text_width > 0 { v.text_width } else { 80 };
                let display = build_display_lines(&v.lines, v.wrap, width);
                let r0 = start.0;
                let r1 = end.0;
                let mut result = String::new();
                for (i, (_, _, chunk)) in display.iter().enumerate().skip(r0).take(r1 - r0 + 1) {
                    let chars: Vec<char> = chunk.chars().collect();
                    let n = chars.len();
                    let from = if i == r0 { start.1.min(n) } else { 0 };
                    let to   = if i == r1 { end.1.min(n)   } else { n };
                    if !result.is_empty() { result.push('\n'); }
                    result.extend(chars[from..to].iter());
                }
                result
            }
        };
        if text.is_empty() {
            if let Some(v) = &mut self.viewer {
                v.message = Some("No selection — drag to select".to_string());
            }
            return;
        }
        let count = text.chars().count();
        if let Ok(mut board) = arboard::Clipboard::new() {
            let _ = board.set_text(&text);
        }
        if let Some(v) = &mut self.viewer {
            v.message = Some(format!("Copied {} char(s)", count));
        }
    }

    fn is_double_click(&mut self, col: u16, row: u16) -> bool {
        let now = std::time::Instant::now();
        let is_double = self.last_click
            .as_ref()
            .map(|(lc, lr, lt)| *lc == col && *lr == row && lt.elapsed().as_millis() < 400)
            .unwrap_or(false);
        self.last_click = Some((col, row, now));
        is_double
    }
}

fn rect_contains(rect: Rect, col: u16, row: u16) -> bool {
    col >= rect.x && col < rect.x + rect.width && row >= rect.y && row < rect.y + rect.height
}

// ── Search helpers ────────────────────────────────────────────────────────────

pub fn parse_hex_pattern(input: &str) -> Option<Vec<u8>> {
    let mut bytes = Vec::new();
    for token in input.split(|c: char| c.is_whitespace() || c == ',') {
        if token.is_empty() { continue; }
        let hex = token.strip_prefix("0x").or_else(|| token.strip_prefix("0X")).unwrap_or(token);
        match u8::from_str_radix(hex, 16) {
            Ok(b) => bytes.push(b),
            Err(_) => return None,
        }
    }
    if bytes.is_empty() { None } else { Some(bytes) }
}

fn wildcard_match(pattern: &str, name: &str) -> bool {
    let p: Vec<char> = pattern.to_lowercase().chars().collect();
    let n: Vec<char> = name.to_lowercase().chars().collect();
    wm(&p, &n)
}

fn wm(p: &[char], n: &[char]) -> bool {
    match (p.first(), n.first()) {
        (None, None)      => true,
        (Some(&'*'), _)   => wm(&p[1..], n) || (!n.is_empty() && wm(p, &n[1..])),
        (Some(&'?'), Some(_)) => wm(&p[1..], &n[1..]),
        (Some(pc), Some(nc)) if pc == nc => wm(&p[1..], &n[1..]),
        _ => false,
    }
}

fn find_bytes(haystack: &[u8], needle: &[u8]) -> Vec<u64> {
    let mut out = Vec::new();
    if needle.is_empty() || haystack.len() < needle.len() { return out; }
    for i in 0..=(haystack.len() - needle.len()) {
        if haystack[i..i + needle.len()] == *needle {
            out.push(i as u64);
        }
    }
    out
}

fn search_dir_recursive(
    dir: &Path,
    name_pattern: &str,
    find_text: Option<&str>,
    find_hex: Option<&[u8]>,
    tx: &std::sync::mpsc::Sender<SearchResult>,
    stop: &Arc<AtomicBool>,
) {
    let entries = match std::fs::read_dir(dir) { Ok(e) => e, Err(_) => return };
    for entry in entries.flatten() {
        if stop.load(Ordering::Relaxed) { return; }
        let path = entry.path();
        let fname = entry.file_name().to_string_lossy().to_string();
        if path.is_dir() {
            if fname != "." && fname != ".." {
                search_dir_recursive(&path, name_pattern, find_text, find_hex, tx, stop);
            }
        } else if wildcard_match(name_pattern, &fname) {
            if let Some(text) = find_text {
                match std::fs::read_to_string(&path) {
                    Ok(content) => {
                        for (i, line) in content.lines().enumerate() {
                            if line.contains(text) {
                                let _ = tx.send(SearchResult {
                                    path: path.clone(),
                                    kind: SearchResultKind::TextMatch { line_num: i + 1, line: line.to_string() },
                                });
                            }
                        }
                    }
                    Err(_) => {
                        // binary file — search text bytes
                        if let Ok(data) = std::fs::read(&path) {
                            for offset in find_bytes(&data, text.as_bytes()) {
                                let _ = tx.send(SearchResult { path: path.clone(), kind: SearchResultKind::HexMatch { offset } });
                            }
                        }
                    }
                }
            } else if let Some(hex) = find_hex {
                if let Ok(data) = std::fs::read(&path) {
                    for offset in find_bytes(&data, hex) {
                        let _ = tx.send(SearchResult { path: path.clone(), kind: SearchResultKind::HexMatch { offset } });
                    }
                }
            } else {
                let _ = tx.send(SearchResult { path: path.clone(), kind: SearchResultKind::NameMatch });
            }
        }
    }
}

fn char_to_byte(s: &str, char_idx: usize) -> usize {
    s.char_indices().nth(char_idx).map(|(b, _)| b).unwrap_or(s.len())
}

fn current_time_str() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    let secs = SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_secs();
    let h = (secs % 86400) / 3600;
    let m = (secs % 3600) / 60;
    let s = secs % 60;
    format!("{:02}:{:02}:{:02}", h, m, s)
}

/// Strip Windows extended-length path prefix `\\?\` for clean display/copy.
pub fn clean_path(path: &std::path::Path) -> String {
    let s = path.display().to_string();
    s.strip_prefix(r"\\?\").map(|s| s.to_string()).unwrap_or(s)
}

/// Build wrapped display lines from file lines.
/// Returns Vec of (orig_line_idx, is_first_chunk, chunk_text).
pub fn build_display_lines(lines: &[String], wrap: bool, width: usize) -> Vec<(usize, bool, String)> {
    let mut out = Vec::new();
    for (orig_idx, line) in lines.iter().enumerate() {
        let chars: Vec<char> = line.chars().collect();
        if !wrap || width == 0 || chars.len() <= width {
            out.push((orig_idx, true, line.clone()));
        } else {
            let mut pos = 0;
            let mut first = true;
            while pos < chars.len() {
                let end = (pos + width).min(chars.len());
                let chunk: String = chars[pos..end].iter().collect();
                out.push((orig_idx, first, chunk));
                pos = end;
                first = false;
            }
        }
    }
    if out.is_empty() {
        out.push((0, true, String::new()));
    }
    out
}

fn list_drives() -> Vec<DriveEntry> {
    #[cfg(windows)]
    {
        extern "system" {
            fn GetLogicalDrives() -> u32;
            fn GetVolumeInformationW(
                lpRootPathName:        *const u16,
                lpVolumeNameBuffer:    *mut u16,
                nVolumeNameSize:       u32,
                lpVolumeSerialNumber:  *mut u32,
                lpMaxComponentLength:  *mut u32,
                lpFileSystemFlags:     *mut u32,
                lpFileSystemNameBuffer: *mut u16,
                nFileSystemNameSize:   u32,
            ) -> i32;
        }

        fn volume_label(root: &str) -> String {
            use std::os::windows::ffi::OsStrExt;
            let wide: Vec<u16> = std::ffi::OsStr::new(root)
                .encode_wide()
                .chain(std::iter::once(0))
                .collect();
            let mut buf = vec![0u16; 256];
            let ok = unsafe {
                GetVolumeInformationW(
                    wide.as_ptr(), buf.as_mut_ptr(), buf.len() as u32,
                    std::ptr::null_mut(), std::ptr::null_mut(),
                    std::ptr::null_mut(), std::ptr::null_mut(), 0,
                )
            };
            if ok != 0 {
                let end = buf.iter().position(|&c| c == 0).unwrap_or(0);
                String::from_utf16_lossy(&buf[..end]).to_string()
            } else {
                String::new()
            }
        }

        let mask = unsafe { GetLogicalDrives() };
        (0..26u32)
            .filter(|i| mask & (1 << i) != 0)
            .map(|i| {
                let letter = (b'A' + i as u8) as char;
                let root   = format!("{}:\\", letter);
                let label  = volume_label(&root);
                DriveEntry { root, label }
            })
            .collect()
    }
    #[cfg(target_os = "macos")]
    {
        let mut drives = vec![DriveEntry { root: "/".to_string(), label: "Macintosh HD".to_string() }];
        if let Ok(entries) = std::fs::read_dir("/Volumes") {
            for entry in entries.flatten() {
                let name = entry.file_name().to_string_lossy().to_string();
                let root = format!("/Volumes/{}", name);
                drives.push(DriveEntry { root, label: name });
            }
        }
        drives
    }
    #[cfg(not(any(windows, target_os = "macos")))]
    {
        // Linux: parse /proc/mounts, skip virtual filesystems
        const SKIP: &[&str] = &[
            "proc", "sysfs", "devtmpfs", "tmpfs", "devpts", "cgroup", "cgroup2",
            "pstore", "bpf", "tracefs", "debugfs", "securityfs", "configfs",
            "fusectl", "hugetlbfs", "mqueue", "sunrpc", "efivarfs", "autofs",
            "squashfs", "overlay", "nsfs",
        ];
        let content = std::fs::read_to_string("/proc/mounts").unwrap_or_default();
        content
            .lines()
            .filter_map(|line| {
                let mut parts = line.split_whitespace();
                let _device = parts.next()?;
                let mount   = parts.next()?;
                let fstype  = parts.next()?;
                if SKIP.contains(&fstype) { return None; }
                Some(DriveEntry { root: mount.replace("\\040", " "), label: String::new() })
            })
            .collect()
    }
}

fn reveal_in_file_manager_label() -> String {
    #[cfg(target_os = "macos")]
    return "Reveal in Finder".to_string();
    #[cfg(target_os = "windows")]
    return "Show in Explorer".to_string();
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    return "Open in file manager".to_string();
}

fn reveal_in_file_manager(path: &Path) {
    #[cfg(target_os = "macos")]
    {
        let _ = std::process::Command::new("open")
            .arg("-R")
            .arg(path)
            .spawn();
    }
    #[cfg(target_os = "windows")]
    {
        let arg = format!("/select,{}", path.to_string_lossy());
        let _ = std::process::Command::new("explorer")
            .arg(arg)
            .spawn();
    }
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    {
        // Try common file managers that support --select, fall back to xdg-open on parent dir
        let dir = if path.is_dir() { path } else { path.parent().unwrap_or(path) };
        let path_str = path.to_string_lossy().into_owned();
        let dir_str  = dir.to_string_lossy().into_owned();
        let candidates: &[(&str, &[&str])] = &[
            ("nautilus", &["--select", &path_str]),
            ("dolphin",  &["--select", &path_str]),
            ("thunar",   &[&dir_str]),
        ];
        for (bin, args) in candidates {
            if std::process::Command::new(bin).args(*args).spawn().is_ok() {
                return;
            }
        }
        let _ = std::process::Command::new("xdg-open").arg(dir).spawn();
    }
}

fn copy_dir_recursive(src: &PathBuf, dst: &PathBuf) -> Result<()> {
    fs::create_dir_all(dst)?;
    for entry in fs::read_dir(src)? {
        let entry = entry?;
        let dest = dst.join(entry.file_name());
        if entry.file_type()?.is_dir() {
            copy_dir_recursive(&entry.path(), &dest)?;
        } else {
            fs::copy(entry.path(), &dest)?;
        }
    }
    Ok(())
}
