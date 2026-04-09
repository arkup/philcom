use ratatui::{
    Frame,
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Clear, List, ListItem, Paragraph},
};

use crate::app::{App, Dialog, PanelSide, SearchField, SearchResult, SearchResultKind, clean_path};
use crate::editor::EditorState;
use crate::highlight::Highlighter;
use crate::panel::{FileKind, Panel};
use crate::theme::Theme;

const MENU_ITEMS: &[&str] = &["File", "Left", "Options", "Right"];

pub fn render(f: &mut Frame, app: &mut App) {
    let area = f.area();

    // Editor: full-screen takeover
    if app.editor.is_some() {
        let theme = app.theme.clone();
        let editor = app.editor.as_mut().unwrap();
        render_editor(f, editor, &theme, area);
        return;
    }

    // Viewer: full-screen takeover
    if app.viewer.is_some() {
        render_viewer(f, app, area);
        if app.context_menu.is_some() { render_context_menu(f, app); }
        return;
    }

    // Log: full-screen takeover
    if app.log_popup.is_some() {
        render_log_view(f, app, area);
        return;
    }

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1), // menu bar
            Constraint::Min(0),    // panels
            Constraint::Length(1), // command line / status
            Constraint::Length(1), // function buttons
        ])
        .split(area);

    render_menu(f, app, chunks[0]);
    render_panels(f, app, chunks[1]);
    render_cmdline(f, app, chunks[2]);
    render_buttons(f, app, chunks[3]);


    // Search dialog overlay
    if app.search_dialog.is_some() {
        let theme = app.theme.clone();
        render_search_dialog(f, app, &theme, area);
    }

    // Stop-search confirmation overlay
    if app.search_stop_confirm {
        let theme = app.theme.clone();
        render_search_stop_dialog(f, app, area, &theme);
    }

    // Dialog overlay (rendered on top)
    if app.dialog.is_some() {
        let theme = app.theme.clone();
        render_dialog(f, app, &theme, area);
    }

    // Theme submenu dropdown (rendered on top of menu bar)
    if app.show_menu && app.submenu_open {
        let theme = app.theme.clone();
        render_theme_submenu(f, app, &theme);
    }

    // File submenu dropdown
    if app.show_menu && app.file_submenu_open {
        let theme = app.theme.clone();
        render_file_submenu(f, app, &theme);
    }

    // Panel filter submenu
    if app.show_menu && app.panel_submenu_open {
        let theme = app.theme.clone();
        render_panel_submenu(f, app, &theme);
    }

    // History popup
    if app.history_popup.is_some() {
        let theme = app.theme.clone();
        render_history_popup(f, app, &theme);
    }

    // Bookmark popup
    if app.bookmark_popup.is_some() {
        let theme = app.theme.clone();
        render_bookmark_popup(f, app, &theme);
    }

    // Goto paste menu
    if let Some(rect) = app.goto_paste_menu {
        let theme = app.theme.clone();
        render_goto_paste_menu(f, rect, &theme);
    }

    // Context menu (topmost overlay)
    if app.context_menu.is_some() {
        render_context_menu(f, app);
    }
}

// ── Viewer ────────────────────────────────────────────────────────────────────

fn render_viewer(f: &mut Frame, app: &mut App, area: Rect) {
    use crate::app::build_display_lines;

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Min(0), Constraint::Length(1)])
        .split(area);

    let hex_mode = app.viewer.as_ref().map(|v| v.hex_mode).unwrap_or(false);

    // --- Immutable read pass ---
    let (filename, wrap, scroll, sel, message) = {
        let v = app.viewer.as_ref().unwrap();
        let fname = v.path.file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_else(|| v.path.display().to_string());
        let sel = match (v.select_start, v.select_end) {
            (Some(s), Some(e)) => Some((s.min(e), s.max(e))),
            _ => None,
        };
        (fname, v.wrap, v.scroll, sel, v.message.clone())
    };

    let theme = app.theme.clone();

    let mode_ind = if hex_mode { " [hex]" } else if wrap { " [wrap]" } else { "" };
    let block = Block::default()
        .borders(Borders::ALL)
        .title(Span::styled(
            format!(" {}{} ", filename, mode_ind),
            Style::default().fg(theme.panel_title),
        ))
        .border_style(Style::default().fg(theme.panel_border))
        .style(theme.panel_style());

    let inner = block.inner(chunks[0]);
    f.render_widget(block, chunks[0]);

    let height = inner.height as usize;
    let normal_bg = theme.panel_bg;
    let normal_fg = theme.panel_fg;

    let total_display;
    let text_width;

    if hex_mode {
        // ── Hex view ──────────────────────────────────────────────────────────
        const BYTES_PER_ROW: usize = 16;
        let (raw, hex_sel) = {
            let v = app.viewer.as_ref().unwrap();
            let sel = match (v.select_start, v.select_end) {
                (Some(s), Some(e)) => Some((s.0.min(e.0), s.0.max(e.0))),
                _ => None,
            };
            (v.raw_bytes.clone(), sel)
        };
        let total_rows = (raw.len() + BYTES_PER_ROW - 1).max(1) / BYTES_PER_ROW;
        total_display = total_rows;
        text_width = inner.width as usize;

        let sel_bg = theme.selected_bg;
        let sel_fg = theme.selected_fg;

        let lines: Vec<Line> = (scroll..total_rows.min(scroll + height))
            .map(|row| {
                let start = row * BYTES_PER_ROW;
                let end   = (start + BYTES_PER_ROW).min(raw.len());
                let chunk = &raw[start..end];
                let selected = hex_sel.map(|(r0, r1)| row >= r0 && row <= r1).unwrap_or(false);
                let (bg, fg_off, fg_hex, fg_asc) = if selected {
                    (sel_bg, sel_fg, sel_fg, sel_fg)
                } else {
                    (normal_bg, Color::DarkGray, Color::Rgb(160, 200, 160), normal_fg)
                };

                // offset
                let offset = Span::styled(
                    format!("{:08X}  ", start),
                    Style::default().fg(fg_off).bg(bg),
                );

                // hex bytes — two groups of 8 separated by extra space
                let mut hex_str = String::new();
                for (i, b) in chunk.iter().enumerate() {
                    if i == 8 { hex_str.push(' '); }
                    hex_str.push_str(&format!("{:02X} ", b));
                }
                // pad so ASCII column stays aligned
                let expected_hex_len = 3 * 16 + 1; // 49
                while hex_str.len() < expected_hex_len { hex_str.push(' '); }
                let hex_span = Span::styled(hex_str, Style::default().fg(fg_hex).bg(bg));

                // ASCII
                let ascii: String = chunk.iter().map(|&b| {
                    if b >= 0x20 && b < 0x7f { b as char } else { '.' }
                }).collect();
                let ascii_span = Span::styled(
                    format!(" |{}|", ascii),
                    Style::default().fg(fg_asc).bg(bg),
                );

                Line::from(vec![offset, hex_span, ascii_span])
            })
            .collect();

        f.render_widget(Paragraph::new(lines).style(theme.panel_style()), inner);
        app.viewer_inner_rect = inner;
        if let Some(v) = &mut app.viewer {
            v.total_display_lines = total_display;
            v.text_width = text_width;
        }

        let has_sel = hex_sel.is_some();
        let footer = if let Some(msg) = message {
            format!(" {} ", msg)
        } else {
            format!(
                " row {}/{} \u{2502} [h] text view \u{2502} {}drag\u{2192}select [c] copy \u{2502} ESC/F3/q close ",
                scroll + 1, total_rows.max(1),
                if has_sel { "\u{2713} " } else { "" },
            )
        };
        f.render_widget(Paragraph::new(footer).style(theme.cmdline_style()), chunks[1]);
    } else {
        // ── Text view ─────────────────────────────────────────────────────────
        // gutter is "XXXX│ " = 6 display columns
        text_width = (inner.width as usize).saturating_sub(6);

        let (display, hl_spans) = {
            let v = app.viewer.as_ref().unwrap();
            let display = build_display_lines(&v.lines, wrap, text_width);
            // Pre-compute syntax highlighting for all original lines
            let ext = v.path.extension()
                .and_then(|e| e.to_str())
                .unwrap_or("")
                .to_lowercase();
            let mut hl = Highlighter::new(&ext);
            let spans_per_line: Vec<Vec<crate::highlight::HighlightSpan>> = if hl.is_active() {
                v.lines.iter().map(|line| hl.highlight(line)).collect()
            } else {
                Vec::new()
            };
            (display, spans_per_line)
        };
        total_display = display.len();

        let sel_bg = theme.selected_bg;
        let sel_fg = theme.selected_fg;
        let has_hl = !hl_spans.is_empty();

        let lines: Vec<Line> = display
            .iter()
            .enumerate()
            .skip(scroll)
            .take(height)
            .map(|(abs_i, (orig_idx, is_first, chunk))| {
                let gutter_text = if *is_first {
                    format!("{:>4}\u{2502} ", orig_idx + 1)
                } else {
                    "    \u{2502} ".to_string()
                };
                let gutter = Span::styled(
                    gutter_text,
                    Style::default().fg(Color::DarkGray).bg(normal_bg),
                );

                // Build text spans — with selection override if needed
                let in_sel = sel.map(|((sr, _), (er, _))| abs_i >= sr && abs_i <= er).unwrap_or(false);

                let text_spans: Vec<Span> = if in_sel {
                    // Selection highlight takes priority over syntax colors
                    if let Some(((sr, sc), (er, ec))) = sel {
                        let chars: Vec<char> = chunk.chars().collect();
                        let n = chars.len();
                        let from = if abs_i == sr { sc.min(n) } else { 0 };
                        let to   = if abs_i == er { ec.min(n) } else { n };
                        if from < to {
                            let before: String = chars[..from].iter().collect();
                            let selected: String = chars[from..to].iter().collect();
                            let after: String = chars[to..].iter().collect();
                            let mut sp = Vec::new();
                            if !before.is_empty() { sp.push(Span::styled(before, Style::default().fg(normal_fg).bg(normal_bg))); }
                            sp.push(Span::styled(selected, Style::default().fg(sel_fg).bg(sel_bg)));
                            if !after.is_empty() { sp.push(Span::styled(after, Style::default().fg(normal_fg).bg(normal_bg))); }
                            sp
                        } else {
                            vec![Span::styled(chunk.clone(), Style::default().fg(normal_fg).bg(normal_bg))]
                        }
                    } else {
                        vec![Span::styled(chunk.clone(), Style::default().fg(normal_fg).bg(normal_bg))]
                    }
                } else if has_hl {
                    // Syntax highlighted — use pre-computed spans for this original line
                    // (for wrapped chunks we highlight the chunk independently)
                    let raw_spans = if *is_first {
                        hl_spans.get(*orig_idx)
                    } else {
                        None
                    };
                    if let Some(hspans) = raw_spans {
                        hspans.iter().map(|s| {
                            let fg = if s.color == Color::Reset { normal_fg } else { s.color };
                            Span::styled(s.text.clone(), Style::default().fg(fg).bg(normal_bg))
                        }).collect()
                    } else {
                        // wrapped continuation or no highlight data — plain
                        vec![Span::styled(chunk.clone(), Style::default().fg(normal_fg).bg(normal_bg))]
                    }
                } else {
                    vec![Span::styled(chunk.clone(), Style::default().fg(normal_fg).bg(normal_bg))]
                };

                let mut all_spans = vec![gutter];
                all_spans.extend(text_spans);
                Line::from(all_spans)
            })
            .collect();

        f.render_widget(Paragraph::new(lines).style(theme.panel_style()), inner);

        app.viewer_inner_rect = inner;
        if let Some(v) = &mut app.viewer {
            v.total_display_lines = total_display;
            v.text_width = text_width;
        }

        let has_sel = sel.is_some();
        let footer = if let Some(msg) = message {
            format!(" {} ", msg)
        } else {
            format!(
                " {}/{} \u{2502} [w] wrap{} \u{2502} [h] hex \u{2502} {}drag\u{2192}select [c] copy \u{2502} ESC/F3/q close ",
                scroll + 1,
                total_display.max(1),
                if wrap { " ON" } else { "" },
                if has_sel { "\u{2713} " } else { "" },
            )
        };
        f.render_widget(Paragraph::new(footer).style(theme.cmdline_style()), chunks[1]);
    }
}

// ── Editor ────────────────────────────────────────────────────────────────────

fn render_editor(f: &mut Frame, ed: &mut EditorState, theme: &Theme, area: Rect) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Min(0), Constraint::Length(1)])
        .split(area);

    let dirty_mark = if ed.dirty { " [+]" } else { "" };
    let filename = ed.path.file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_else(|| ed.path.display().to_string());

    let block = Block::default()
        .borders(Borders::ALL)
        .title(Span::styled(
            format!(" {}{} ", filename, dirty_mark),
            Style::default().fg(theme.panel_title),
        ))
        .border_style(Style::default().fg(theme.panel_border))
        .style(theme.panel_style());

    let inner = block.inner(chunks[0]);
    f.render_widget(block, chunks[0]);

    let height = inner.height as usize;
    ed.adjust_scroll(height);

    let lines: Vec<Line> = ed.lines
        .iter()
        .enumerate()
        .skip(ed.scroll)
        .take(height)
        .map(|(i, line)| {
            let is_cursor_row = i == ed.cursor_row;
            let row_style = if is_cursor_row {
                theme.selected_style()
            } else {
                theme.panel_style()
            };
            Line::from(vec![
                Span::styled(
                    format!("{:>4}\u{2502} ", i + 1),
                    Style::default().fg(Color::DarkGray).bg(theme.panel_bg),
                ),
                Span::styled(line.clone(), row_style),
            ])
        })
        .collect();

    f.render_widget(Paragraph::new(lines).style(theme.panel_style()), inner);

    // Position the terminal cursor
    let cursor_screen_row = ed.cursor_row.saturating_sub(ed.scroll) as u16;
    let cursor_screen_col = ed.cursor_col as u16;
    // +6 for the line-number gutter "XXXX│ "
    f.set_cursor_position((
        inner.x + 6 + cursor_screen_col,
        inner.y + cursor_screen_row,
    ));

    // Footer
    let footer = if ed.confirm_close {
        " Unsaved changes! Discard and close? [y] Yes  [any key] Cancel ".to_string()
    } else if let Some(msg) = &ed.message {
        format!(" {} ", msg)
    } else {
        format!(
            " {}:{} \u{2502} Ctrl+S save \u{2502} Ctrl+C copy line \u{2502} Ctrl+X cut \u{2502} Ctrl+V paste \u{2502} ESC/F4 close ",
            ed.cursor_row + 1,
            ed.cursor_col + 1,
        )
    };

    f.render_widget(
        Paragraph::new(footer).style(theme.cmdline_style()),
        chunks[1],
    );
}

// ── Menu bar ──────────────────────────────────────────────────────────────────

fn render_menu(f: &mut Frame, app: &mut App, area: Rect) {
    let theme = &app.theme;

    // Calculate per-item rects for mouse hit-testing
    app.menu_item_rects.clear();
    let mut x = area.x;
    for &item in MENU_ITEMS.iter() {
        let w = (item.len() as u16) + 2; // " item "
        app.menu_item_rects.push(Rect::new(x, area.y, w, 1));
        x += w;
    }

    let spans: Vec<Span> = MENU_ITEMS
        .iter()
        .enumerate()
        .map(|(i, &item)| {
            let style = if app.show_menu && i == app.menu_index {
                theme.menu_selected_style()
            } else {
                theme.menu_style()
            };
            Span::styled(format!(" {} ", item), style)
        })
        .collect();

    f.render_widget(
        Paragraph::new(Line::from(spans)).style(theme.menu_style()),
        area,
    );
}

// ── Panels ────────────────────────────────────────────────────────────────────

fn render_panels(f: &mut Frame, app: &mut App, area: Rect) {
    let sp = app.split_percent;
    let chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(sp), Constraint::Percentage(100 - sp)])
        .split(area);

    // Store rects for mouse hit-testing
    app.left_panel_rect  = chunks[0];
    app.right_panel_rect = chunks[1];
    app.left_tab_rects   = tab_rects_for(&app.left_panel,  chunks[0]);
    app.right_tab_rects  = tab_rects_for(&app.right_panel, chunks[1]);

    let panel_height = (chunks[0].height as usize).saturating_sub(2);
    app.left_panel.adjust_scroll(panel_height);
    app.right_panel.adjust_scroll(panel_height);

    let left_active = app.active_panel == PanelSide::Left;
    let theme = app.theme.clone();

    let sr_side = app.search_results.as_ref().map(|s| s.side.clone());
    match sr_side {
        Some(PanelSide::Left) => {
            render_search_results_panel(f, app, chunks[0], left_active, &theme);
            let (sr, pr) = render_panel(f, &app.right_panel, chunks[1], !left_active, &theme);
            app.right_sort_rect = sr; app.right_path_rects = pr;
        }
        Some(PanelSide::Right) => {
            let (sl, pl) = render_panel(f, &app.left_panel, chunks[0], left_active, &theme);
            app.left_sort_rect = sl; app.left_path_rects = pl;
            render_search_results_panel(f, app, chunks[1], !left_active, &theme);
        }
        None => {
            let (sl, pl) = render_panel(f, &app.left_panel,  chunks[0], left_active,  &theme);
            app.left_sort_rect = sl; app.left_path_rects = pl;
            let (sr, pr) = render_panel(f, &app.right_panel, chunks[1], !left_active, &theme);
            app.right_sort_rect = sr; app.right_path_rects = pr;
        }
    }
}

fn render_panel(f: &mut Frame, panel: &Panel, area: Rect, active: bool, theme: &Theme) -> (Rect, Vec<(Rect, std::path::PathBuf)>) {
    let tab = panel.tab();
    let filter_tag = if panel.filter_exec { " [EXE]" } else { "" };
    let sort_tag = format!(" [{}{}]", if tab.sort_asc { "↑" } else { "↓" }, tab.sort_by.label());

    // Build title: tab bar when multiple tabs, plain path when single
    // Also track char offset of sort_tag for click detection
    let (title_line, sort_prefix_len): (Line, usize) = if panel.tab_count() > 1 {
        let mut spans = vec![Span::raw(" ")];
        let mut prefix = 1usize;
        for (i, t) in panel.tabs.iter().enumerate() {
            let label = format!(" {} ", t.label());
            prefix += label.chars().count() + 1; // +1 for the trailing " " span
            let style = if i == panel.active_tab {
                Style::default().fg(theme.panel_bg).bg(theme.panel_title)
            } else {
                Style::default().fg(theme.panel_border).bg(theme.panel_bg)
            };
            spans.push(Span::styled(label, style));
            spans.push(Span::raw(" "));
        }
        if !filter_tag.is_empty() {
            prefix += filter_tag.chars().count();
            spans.push(Span::styled(filter_tag.to_string(), Style::default().fg(theme.panel_title)));
        }
        spans.push(Span::styled(sort_tag.clone(), Style::default().fg(theme.panel_title)));
        (Line::from(spans), prefix)
    } else {
        let prefix = format!(" {}{}", clean_path(&panel.tab().path), filter_tag);
        let prefix_len = prefix.chars().count();
        let title = format!("{}{} ", prefix, sort_tag);
        (Line::from(Span::styled(title, Style::default().fg(theme.panel_title))), prefix_len)
    };

    // Sort rect: sits in the top border row at the computed offset
    let sort_x = (area.x + 1).saturating_add(sort_prefix_len as u16);
    let sort_rect = Rect::new(sort_x, area.y, sort_tag.chars().count() as u16, 1);

    // Path breadcrumb rects (single-tab only — multi-tab shows tab labels instead)
    // Title starts: left border (1) + leading space (1) = area.x + 2
    let path_rects: Vec<(Rect, std::path::PathBuf)> = if panel.tab_count() == 1 {
        let path = &panel.tab().path;
        let path_str = clean_path(path);
        // base_x: after left border + leading space in title
        let base_x = area.x + 2;
        let mut rects = Vec::new();
        let mut offset: u16 = 0;
        // Build prefix paths segment by segment
        let mut accumulated = std::path::PathBuf::new();
        // Split on the OS path separator — each segment click navigates to that prefix
        let sep = std::path::MAIN_SEPARATOR;
        let parts: Vec<&str> = path_str.split(sep).collect();
        for (i, part) in parts.iter().enumerate() {
            if i == 0 && part.is_empty() {
                // Unix root: the '/' separator itself
                accumulated.push("/");
                let rect = Rect::new(base_x + offset, area.y, 1, 1);
                rects.push((rect, accumulated.clone()));
                offset += 1;
            } else if i == 0 && part.ends_with(':') {
                // Windows drive root e.g. "C:" — include trailing separator so
                // PathBuf is absolute (C:\) and further push() calls work correctly
                accumulated = std::path::PathBuf::from(format!("{}{}", part, sep));
                let w = part.chars().count() as u16;
                let rect = Rect::new(base_x + offset, area.y, w, 1);
                rects.push((rect, accumulated.clone()));
                offset += w;
                if i + 1 < parts.len() { offset += 1; }
            } else if !part.is_empty() {
                accumulated.push(part);
                let w = part.chars().count() as u16;
                let rect = Rect::new(base_x + offset, area.y, w, 1);
                rects.push((rect, accumulated.clone()));
                offset += w;
                // skip the separator that follows (if not last)
                if i + 1 < parts.len() { offset += 1; }
            }
        }
        rects
    } else {
        Vec::new()
    };

    let border_style = if active {
        Style::default().fg(theme.panel_border)
    } else {
        Style::default().fg(theme.panel_fg)
    };

    let block = Block::default()
        .borders(Borders::ALL)
        .title(title_line)
        .border_style(border_style)
        .style(theme.panel_style());

    let has_filter = !tab.filter.is_empty();
    let filter_active = tab.filter_active;

    // Reserve one row for the filter bar when active
    let content_height = (area.height as usize).saturating_sub(2 + if has_filter || filter_active { 1 } else { 0 });
    // size=6, space=1, date=10, space=1 → 18 fixed chars
    let inner_width = (area.width as usize).saturating_sub(2);
    const FIXED: usize = 18; // " 128KB 2026-01-15"

    let filtered: Vec<&crate::panel::Entry> = tab.filtered_entries();
    let scroll = tab.scroll.min(filtered.len().saturating_sub(1));
    let selected = tab.selected;

    let items: Vec<ListItem> = filtered
        .iter()
        .enumerate()
        .skip(scroll)
        .take(content_height)
        .map(|(abs_idx, entry)| {
            let style = if abs_idx == selected {
                if active { theme.selected_style() }
                else { Style::default().fg(theme.selected_bg).bg(theme.panel_bg) }
            } else if tab.marked.contains(&entry.name) {
                Style::default().fg(Color::Yellow).bg(theme.panel_bg)
            } else {
                file_kind_style(&entry.kind, theme)
            };

            let name = if entry.is_dir && entry.name != ".." {
                format!("{}/", entry.name)
            } else {
                entry.name.clone()
            };

            let size_str = if entry.name == ".." {
                "       ".to_string()
            } else if entry.is_dir {
                " <DIR> ".to_string()
            } else {
                format!("{:>6} ", format_size(entry.size))
            };

            let date_str = if entry.name == ".." {
                "          ".to_string()
            } else {
                format_date(entry.modified)
            };

            let name_width = inner_width.saturating_sub(FIXED);
            let display = format!(
                "{:<width$} {}{}",
                truncate(&name, name_width), size_str, date_str,
                width = name_width
            );
            ListItem::new(display).style(style)
        })
        .collect();

    let inner = block.inner(area);
    f.render_widget(block, area);

    if has_filter || filter_active {
        // Split inner area: content on top, filter bar at bottom
        let split = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Min(0), Constraint::Length(1)])
            .split(inner);
        f.render_widget(List::new(items), split[0]);
        let cursor = if filter_active { "_" } else { "" };
        let filter_text = format!(" /{}{}", tab.filter, cursor);
        f.render_widget(
            Paragraph::new(filter_text).style(Style::default().fg(theme.panel_title).bg(theme.panel_bg)),
            split[1],
        );
    } else {
        f.render_widget(List::new(items), inner);
    }

    (sort_rect, path_rects)
}

// ── Command line / status ─────────────────────────────────────────────────────

fn render_cmdline(f: &mut Frame, app: &App, area: Rect) {
    let text = match &app.status_msg {
        Some(msg) => format!(" [!] {}", msg),
        None => format!("> {}_", app.command_line),
    };
    f.render_widget(Paragraph::new(text).style(app.theme.cmdline_style()), area);
}

// ── Function buttons ──────────────────────────────────────────────────────────

fn render_buttons(f: &mut Frame, app: &mut App, area: Rect) {
    let theme = &app.theme;
    let buttons = &app.config.buttons;
    if buttons.is_empty() { return; }

    let count = buttons.len() as u32;
    let constraints: Vec<Constraint> = buttons.iter().map(|_| Constraint::Ratio(1, count)).collect();
    let chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints(constraints)
        .split(area);

    // Store button rects for mouse hit-testing
    app.button_rects = chunks.to_vec();

    for (i, btn) in buttons.iter().enumerate() {
        if i >= chunks.len() { break; }
        let line = Line::from(vec![
            Span::styled(
                format!("{}", btn.key),
                Style::default().fg(theme.btn_key_fg).bg(theme.btn_key_bg),
            ),
            Span::styled(
                format!("{}", btn.label),
                Style::default().fg(theme.btn_label_fg).bg(theme.btn_label_bg),
            ),
        ]);
        f.render_widget(
            Paragraph::new(line).style(Style::default().bg(theme.btn_label_bg)),
            chunks[i],
        );
    }
}

// ── Dialogs ───────────────────────────────────────────────────────────────────

fn render_dialog(f: &mut Frame, app: &mut App, theme: &Theme, area: Rect) {
    match &app.dialog {
        Some(Dialog::Mkdir { input }) => {
            let popup = centered_rect(52, 8, area);
            render_input_dialog(f, popup, theme, " Make Directory ", "Directory name:", input);
        }
        Some(Dialog::NewFile { input, .. }) => {
            let popup = centered_rect(52, 8, area);
            render_input_dialog(f, popup, theme, " New File ", "File name:", input);
        }
        Some(Dialog::Copy { sources, dest_input }) => {
            let summary = op_summary(sources, "Copy");
            let popup = centered_rect(62, 9, area);
            render_dest_dialog(f, popup, theme, " Copy ", &summary, "  To:", dest_input);
        }
        Some(Dialog::Move { sources, dest_input }) => {
            let summary = op_summary(sources, "Move");
            let popup = centered_rect(62, 9, area);
            render_dest_dialog(f, popup, theme, " Move / Rename ", &summary, "  To:", dest_input);
        }
        Some(Dialog::Delete { targets }) => {
            let popup = centered_rect(52, 8, area);
            render_delete_dialog(f, popup, theme, targets);
        }
        Some(Dialog::Rename { path, input, cursor }) => {
            let current = path.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
            let dir = path.parent().map(|p| clean_path(p)).unwrap_or_default();
            let popup = centered_rect(66, 11, area);
            render_rename_dialog(f, popup, theme, &dir, &current, input, *cursor);
        }
        Some(Dialog::Goto { input, cursor, panel }) => {
            let popup = centered_rect(70, 10, area);
            render_goto_dialog(f, popup, theme, input, *cursor, panel);
        }
        Some(Dialog::Sha256Result { filename, hash }) => {
            let popup = centered_rect(72, 11, area);
            let (filename, hash) = (filename.clone(), hash.clone());
            render_sha256_dialog(f, app, popup, theme, &filename, &hash);
        }
        None => {}
    }
}

/// One-line summary: "Copy: filename.txt" or "Copy 5 marked items"
fn op_summary(sources: &[std::path::PathBuf], verb: &str) -> String {
    if sources.len() == 1 {
        let name = sources[0].file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
        format!("  {}: {}", verb, name)
    } else {
        format!("  {} {} marked items", verb, sources.len())
    }
}

fn render_input_dialog(
    f: &mut Frame, area: Rect, theme: &Theme,
    title: &str, label: &str, input: &str,
) {
    f.render_widget(Clear, area);
    let block = Block::default()
        .borders(Borders::ALL)
        .title(Span::styled(title, Style::default().fg(theme.panel_title)))
        .title_bottom(Span::styled(
            " Enter confirm  \u{2502}  Esc cancel ",
            Style::default().fg(theme.panel_fg).bg(theme.panel_bg),
        ))
        .border_style(Style::default().fg(theme.panel_border))
        .style(Style::default().bg(theme.panel_bg).fg(theme.panel_fg));
    let inner = block.inner(area);
    f.render_widget(block, area);

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1),
            Constraint::Length(1),
            Constraint::Length(1),
            Constraint::Length(3),
        ])
        .split(inner);

    f.render_widget(
        Paragraph::new(format!("  {}", label)).style(Style::default().fg(theme.panel_fg).bg(theme.panel_bg)),
        chunks[1],
    );
    f.render_widget(
        Paragraph::new(format!(" {}_ ", input))
            .block(Block::default().borders(Borders::ALL).border_style(Style::default().fg(theme.panel_border)))
            .style(Style::default().fg(theme.panel_fg).bg(theme.panel_bg)),
        chunks[3],
    );
}

fn render_dest_dialog(
    f: &mut Frame, area: Rect, theme: &Theme,
    title: &str, summary: &str, dest_label: &str, dest_input: &str,
) {
    f.render_widget(Clear, area);
    let block = Block::default()
        .borders(Borders::ALL)
        .title(Span::styled(title, Style::default().fg(theme.panel_title)))
        .title_bottom(Span::styled(
            " Enter confirm  \u{2502}  Esc cancel ",
            Style::default().fg(theme.panel_fg).bg(theme.panel_bg),
        ))
        .border_style(Style::default().fg(theme.panel_border))
        .style(Style::default().bg(theme.panel_bg).fg(theme.panel_fg));
    let inner = block.inner(area);
    f.render_widget(block, area);

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1),
            Constraint::Length(1),
            Constraint::Length(1),
            Constraint::Length(1),
            Constraint::Length(3),
        ])
        .split(inner);

    f.render_widget(
        Paragraph::new(summary).style(Style::default().fg(theme.panel_fg).bg(theme.panel_bg)),
        chunks[1],
    );
    f.render_widget(
        Paragraph::new(dest_label).style(Style::default().fg(theme.panel_fg).bg(theme.panel_bg)),
        chunks[3],
    );
    f.render_widget(
        Paragraph::new(format!(" {}_ ", dest_input))
            .block(Block::default().borders(Borders::ALL).border_style(Style::default().fg(theme.panel_border)))
            .style(Style::default().fg(theme.panel_fg).bg(theme.panel_bg)),
        chunks[4],
    );
}

fn render_delete_dialog(f: &mut Frame, area: Rect, theme: &Theme, targets: &[std::path::PathBuf]) {
    // Use a softer warning color that fits the active theme
    let warn_color = if theme.is_light_bg() {
        Color::Red
    } else {
        Color::Rgb(191, 97, 106) // muted red (Nord aurora / works on all dark themes)
    };

    f.render_widget(Clear, area);
    let block = Block::default()
        .borders(Borders::ALL)
        .title(Span::styled(" Delete ", Style::default().fg(warn_color)))
        .title_bottom(Span::styled(
            " Y / Enter confirm  \u{2502}  N / Esc cancel ",
            Style::default().fg(theme.panel_fg).bg(theme.panel_bg),
        ))
        .border_style(Style::default().fg(warn_color))
        .style(Style::default().bg(theme.panel_bg).fg(theme.panel_fg));
    let inner = block.inner(area);
    f.render_widget(block, area);

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1),
            Constraint::Length(1),
            Constraint::Length(1),
            Constraint::Length(1),
        ])
        .split(inner);

    let target_line = if targets.len() == 1 {
        let name = targets[0].file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
        format!("  Delete \"{}\"?", truncate(&name, inner.width as usize - 12))
    } else {
        format!("  Delete {} marked items?", targets.len())
    };

    f.render_widget(
        Paragraph::new(target_line).style(Style::default().fg(theme.panel_fg).bg(theme.panel_bg)),
        chunks[1],
    );
    f.render_widget(
        Paragraph::new("  This cannot be undone!")
            .style(Style::default().fg(warn_color).bg(theme.panel_bg)),
        chunks[3],
    );
}

fn render_sha256_dialog(f: &mut Frame, app: &mut App, area: Rect, theme: &Theme, filename: &str, hash: &str) {
    f.render_widget(Clear, area);
    let block = Block::default()
        .borders(Borders::ALL)
        .title(Span::styled(" SHA-256 ", Style::default().fg(theme.panel_title)))
        .title_bottom(Span::styled(
            " C copy  \u{2502}  Enter / Esc close ",
            Style::default().fg(theme.panel_fg).bg(theme.panel_bg),
        ))
        .border_style(Style::default().fg(theme.panel_border))
        .style(Style::default().bg(theme.panel_bg).fg(theme.panel_fg));
    let inner = block.inner(area);
    f.render_widget(block, area);

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1), // [0] blank
            Constraint::Length(1), // [1] File: ...
            Constraint::Length(1), // [2] blank
            Constraint::Length(1), // [3] Hash:
            Constraint::Length(1), // [4] hash value
            Constraint::Length(1), // [5] blank
            Constraint::Length(1), // [6] [ Copy to clipboard ] button
            Constraint::Min(0),    // [7] remainder
        ])
        .split(inner);

    f.render_widget(
        Paragraph::new(format!("  File: {}", truncate(filename, inner.width as usize - 8)))
            .style(Style::default().fg(theme.panel_fg).bg(theme.panel_bg)),
        chunks[1],
    );
    f.render_widget(
        Paragraph::new("  Hash:").style(Style::default().fg(theme.panel_fg).bg(theme.panel_bg)),
        chunks[3],
    );
    f.render_widget(
        Paragraph::new(format!("  {}", hash))
            .style(Style::default().fg(theme.panel_title).bg(theme.panel_bg)),
        chunks[4],
    );

    // Button: [ Copy to clipboard ]
    let btn_label = "[ Copy to clipboard ]";
    let pad = (inner.width as usize).saturating_sub(btn_label.len()) / 2;
    let btn_text = format!("{:pad$}{}", "", btn_label, pad = pad);
    let btn_rect = Rect::new(
        chunks[6].x + pad as u16,
        chunks[6].y,
        btn_label.len() as u16,
        1,
    );
    f.render_widget(
        Paragraph::new(btn_text)
            .style(Style::default().fg(theme.btn_key_fg).bg(theme.btn_key_bg)),
        chunks[6],
    );
    app.sha256_copy_btn_rect = btn_rect;
}

// ── File submenu ──────────────────────────────────────────────────────────────

fn render_file_submenu(f: &mut Frame, app: &mut App, theme: &Theme) {
    // logical items: 0=Search, 1=Log, [sep], 2=Clear settings, [sep], 3=Exit
    const ITEMS: &[&str] = &["Search", "Log", "Clear settings", "Exit"];
    let x = app.menu_item_rects.get(0).map(|r| r.x).unwrap_or(0);
    let width: u16 = 18;
    let height: u16 = ITEMS.len() as u16 + 2 + 2; // +2 for two separators
    let area = f.area();
    let x = x.min(area.width.saturating_sub(width));
    let rect = Rect::new(x, 1, width, height);
    app.file_submenu_rect = rect;
    f.render_widget(Clear, rect);

    let sep_style = Style::default().bg(theme.menu_bg).fg(theme.panel_border);
    let sep = ListItem::new(format!(" {:\u{2500}<width$} ", "", width = width as usize - 4)).style(sep_style);

    let mut items: Vec<ListItem> = Vec::new();
    for (i, &name) in ITEMS.iter().enumerate() {
        if i == 2 { items.push(sep.clone()); } // separator before "Clear settings"
        if i == 3 { items.push(sep.clone()); } // separator before "Exit"
        let style = if i == app.file_submenu_index { theme.menu_selected_style() } else { theme.menu_style() };
        items.push(ListItem::new(format!(" {} ", name)).style(style));
    }

    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(theme.panel_border))
        .style(Style::default().bg(theme.menu_bg).fg(theme.menu_fg));
    f.render_widget(List::new(items).block(block), rect);
}

fn render_goto_dialog(f: &mut Frame, area: Rect, theme: &Theme, input: &str, cursor: usize, panel: &PanelSide) {
    f.render_widget(Clear, area);
    let block = Block::default()
        .borders(Borders::ALL)
        .title(Span::styled(" Go to Path ", Style::default().fg(theme.panel_title)))
        .title_bottom(Span::styled(
            " Tab switch panel  \u{2502}  Enter confirm  \u{2502}  Esc cancel ",
            Style::default().fg(theme.panel_fg).bg(theme.panel_bg),
        ))
        .border_style(Style::default().fg(theme.panel_title))
        .style(Style::default().bg(theme.panel_bg).fg(theme.panel_fg));
    let inner = block.inner(area);
    f.render_widget(block, area);

    // inner = 10 - 2 = 8 rows
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1), // [0] blank
            Constraint::Length(1), // [1] panel selector
            Constraint::Length(1), // [2] blank
            Constraint::Length(1), // [3] "Path:"
            Constraint::Length(3), // [4] input field
            Constraint::Min(0),
        ])
        .split(inner);

    // Panel selector
    let l_style = if *panel == PanelSide::Left  { theme.selected_style() } else { theme.panel_style() };
    let r_style = if *panel == PanelSide::Right { theme.selected_style() } else { theme.panel_style() };
    f.render_widget(
        Paragraph::new(Line::from(vec![
            Span::raw("  Panel: "),
            Span::styled(" L ", l_style),
            Span::raw("  "),
            Span::styled(" R ", r_style),
            Span::styled("   (Tab to switch)", Style::default().fg(theme.panel_border).bg(theme.panel_bg)),
        ])).style(Style::default().bg(theme.panel_bg)),
        chunks[1],
    );

    f.render_widget(
        Paragraph::new("  Path:").style(Style::default().fg(theme.panel_fg).bg(theme.panel_bg)),
        chunks[3],
    );

    // Input with block cursor
    let chars: Vec<char> = input.chars().collect();
    let before: String = chars[..cursor].iter().collect();
    let cursor_ch: String = chars.get(cursor).map(|c| c.to_string()).unwrap_or_else(|| " ".to_string());
    let after: String = chars.get(cursor + 1..).unwrap_or(&[]).iter().collect();
    let cursor_line = Line::from(vec![
        Span::raw(format!(" {}", before)),
        Span::styled(cursor_ch, theme.selected_style()),
        Span::raw(format!("{} ", after)),
    ]);
    f.render_widget(
        Paragraph::new(cursor_line)
            .block(Block::default().borders(Borders::ALL).border_style(Style::default().fg(theme.panel_title)))
            .style(Style::default().fg(theme.panel_fg).bg(theme.panel_bg)),
        chunks[4],
    );
}

fn render_rename_dialog(f: &mut Frame, area: Rect, theme: &Theme, dir: &str, current: &str, input: &str, cursor: usize) {
    f.render_widget(Clear, area);
    let block = Block::default()
        .borders(Borders::ALL)
        .title(Span::styled(" Rename ", Style::default().fg(theme.panel_title)))
        .title_bottom(Span::styled(
            " Enter confirm  \u{2502}  Esc cancel ",
            Style::default().fg(theme.panel_fg).bg(theme.panel_bg),
        ))
        .border_style(Style::default().fg(theme.panel_title))
        .style(Style::default().bg(theme.panel_bg).fg(theme.panel_fg));
    let inner = block.inner(area);
    f.render_widget(block, area);

    // inner = 11 - 2 borders = 9 rows
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1), // [0] blank
            Constraint::Length(1), // [1] Dir: ...
            Constraint::Length(1), // [2] Current: ...
            Constraint::Length(1), // [3] blank
            Constraint::Length(1), // [4] New name:
            Constraint::Length(3), // [5] input field
            Constraint::Min(0),
        ])
        .split(inner);

    let max_dir = inner.width as usize - 7;
    f.render_widget(
        Paragraph::new(Line::from(vec![
            Span::raw("  Dir: "),
            Span::styled(truncate(dir, max_dir), Style::default().fg(theme.dir_fg)),
        ])).style(Style::default().bg(theme.panel_bg)),
        chunks[1],
    );

    let max_name = inner.width as usize - 12;
    f.render_widget(
        Paragraph::new(Line::from(vec![
            Span::raw("  Current: "),
            Span::styled(truncate(current, max_name), Style::default().fg(theme.panel_title)),
        ])).style(Style::default().bg(theme.panel_bg)),
        chunks[2],
    );
    f.render_widget(
        Paragraph::new("  New name:").style(Style::default().fg(theme.panel_fg).bg(theme.panel_bg)),
        chunks[4],
    );
    // Render input with block cursor
    let chars: Vec<char> = input.chars().collect();
    let before: String = chars[..cursor].iter().collect();
    let cursor_ch: String = chars.get(cursor).map(|c| c.to_string()).unwrap_or_else(|| " ".to_string());
    let after: String = chars.get(cursor + 1..).unwrap_or(&[]).iter().collect();
    let cursor_line = Line::from(vec![
        Span::raw(format!(" {}", before)),
        Span::styled(cursor_ch, theme.selected_style()),
        Span::raw(format!("{} ", after)),
    ]);
    f.render_widget(
        Paragraph::new(cursor_line)
            .block(Block::default().borders(Borders::ALL).border_style(Style::default().fg(theme.panel_title)))
            .style(Style::default().fg(theme.panel_fg).bg(theme.panel_bg)),
        chunks[5],
    );
}

// ── Theme submenu ─────────────────────────────────────────────────────────────

fn render_theme_submenu(f: &mut Frame, app: &mut App, theme: &Theme) {
    use crate::theme::Theme as T;

    let themes = T::all_names();
    // Anchor below the "Options" menu item (index 3)
    let x = app.menu_item_rects.get(1).map(|r| r.x).unwrap_or(20);
    let width: u16 = 14;
    let height: u16 = themes.len() as u16 + 2; // +2 for borders

    let area = Rect::new(x, 1, width, height);
    app.submenu_rect = area; // store for mouse hit-testing
    f.render_widget(Clear, area);

    let items: Vec<ListItem> = themes
        .iter()
        .enumerate()
        .map(|(i, &name)| {
            let is_cursor = i == app.submenu_index;
            let is_active = name == app.config.theme.as_str();
            let label = if is_active {
                format!(" \u{2022} {}", name) // bullet for current theme
            } else {
                format!("   {}", name)
            };
            let style = if is_cursor {
                theme.menu_selected_style()
            } else {
                theme.menu_style()
            };
            ListItem::new(label).style(style)
        })
        .collect();

    let block = Block::default()
        .borders(Borders::ALL)
        .title(Span::styled(" Theme ", Style::default().fg(theme.panel_title)))
        .border_style(Style::default().fg(theme.panel_border))
        .style(Style::default().bg(theme.menu_bg).fg(theme.menu_fg));

    f.render_widget(List::new(items).block(block), area);
}

// ── Panel filter submenu ──────────────────────────────────────────────────────

fn render_panel_submenu(f: &mut Frame, app: &mut App, theme: &Theme) {
    use crate::app::PanelSide;

    const FILTER_ITEMS: &[&str] = &["All files", "Executables only"];
    const ALL_ITEMS: &[&str]    = &["All files", "Executables only", "+ New file", "  History", "  Downloads", "+ New tab", "x Close tab"];

    let menu_idx = if app.panel_submenu_side == PanelSide::Left { 0 } else { 2 };
    let x = app.menu_item_rects.get(menu_idx).map(|r| r.x).unwrap_or(0);
    let width: u16 = 22;
    let height: u16 = ALL_ITEMS.len() as u16 + 1 + 2; // +1 for separator

    let area = Rect::new(x, 1, width, height);
    app.panel_submenu_rect = area;
    f.render_widget(Clear, area);

    let active_exec = if app.panel_submenu_side == PanelSide::Left {
        app.left_panel.filter_exec
    } else {
        app.right_panel.filter_exec
    };

    let sep = ListItem::new(format!(" {:\u{2500}<width$} ", "", width = width as usize - 4))
        .style(Style::default().bg(theme.menu_bg).fg(theme.panel_border));

    let mut items: Vec<ListItem> = Vec::new();
    for (i, &name) in ALL_ITEMS.iter().enumerate() {
        if i == 5 { items.push(sep.clone()); } // separator before tab items
        let is_cursor = i == app.panel_submenu_index;
        let is_active = i < FILTER_ITEMS.len() && (i == 1) == active_exec;
        let label = if is_active { format!(" \u{2022} {}", name) } else { format!("   {}", name) };
        let style = if is_cursor { theme.menu_selected_style() } else { theme.menu_style() };
        items.push(ListItem::new(label).style(style));
    }

    let block = Block::default()
        .borders(Borders::ALL)
        .title(Span::styled(" Panel ", Style::default().fg(theme.panel_title)))
        .border_style(Style::default().fg(theme.panel_border))
        .style(Style::default().bg(theme.menu_bg).fg(theme.menu_fg));

    f.render_widget(List::new(items).block(block), area);
}

// ── Context menu ──────────────────────────────────────────────────────────────

// ── Search dialog ─────────────────────────────────────────────────────────────

fn render_search_dialog(f: &mut Frame, app: &mut App, theme: &Theme, area: Rect) {
    let d = match &app.search_dialog { Some(d) => d, None => return };

    let popup_w: u16 = area.width.saturating_sub(6).max(60);
    let popup_h: u16 = 14;
    let rect = centered_rect(popup_w, popup_h, area);
    f.render_widget(Clear, rect);

    let block = Block::default()
        .borders(Borders::ALL)
        .title(Span::styled(" Search ", Style::default().fg(theme.panel_title)))
        .title_bottom(Span::styled(
            " Tab next field  \u{2502}  Enter search  \u{2502}  Esc cancel ",
            Style::default().fg(theme.panel_fg).bg(theme.panel_bg),
        ))
        .border_style(Style::default().fg(theme.panel_title))
        .style(theme.panel_style());
    let inner = block.inner(rect);
    f.render_widget(block, rect);

    let rows = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1), // blank
            Constraint::Length(3), // Path
            Constraint::Length(1), // blank
            Constraint::Length(3), // Name
            Constraint::Length(1), // blank
            Constraint::Length(3), // Find text / hex (side by side)
            Constraint::Min(0),
        ])
        .split(inner);

    let field_block = |label: &str, focused: bool| {
        Block::default()
            .borders(Borders::ALL)
            .title(Span::styled(format!(" {} ", label), Style::default().fg(theme.panel_title)))
            .border_style(Style::default().fg(if focused { theme.panel_title } else { theme.panel_border }))
            .style(theme.panel_style())
    };

    let render_input = |f: &mut Frame, area: Rect, text: &str, cursor: usize, focused: bool, label: &str| {
        let b = field_block(label, focused);
        let inner = b.inner(area);
        f.render_widget(b, area);
        let chars: Vec<char> = text.chars().collect();
        let before: String = chars[..cursor].iter().collect();
        let cursor_ch: String = chars.get(cursor).map(|c| c.to_string()).unwrap_or(" ".to_string());
        let after: String = chars.get(cursor + 1..).unwrap_or(&[]).iter().collect();
        // scroll so cursor is visible
        let w = inner.width.saturating_sub(2) as usize;
        let start = if cursor > w { cursor - w } else { 0 };
        let before_vis: String = before.chars().skip(start).collect();
        let line = if focused {
            Line::from(vec![
                Span::raw(format!(" {}", before_vis)),
                Span::styled(cursor_ch, theme.selected_style()),
                Span::raw(format!("{} ", after)),
            ])
        } else {
            let display: String = text.chars().skip(start).take(w + 1).collect();
            Line::from(vec![Span::raw(format!(" {} ", display))])
        };
        f.render_widget(Paragraph::new(line).style(theme.panel_style()), inner);
    };

    let (focused, path, path_cursor, name, name_cursor, find_text, text_cursor, find_hex, hex_cursor) = (
        d.focused.clone(), d.path.clone(), d.path_cursor,
        d.name.clone(), d.name_cursor,
        d.find_text.clone(), d.text_cursor,
        d.find_hex.clone(), d.hex_cursor,
    );

    let halves = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(50), Constraint::Percentage(50)])
        .split(rows[5]);

    render_input(f, rows[1],   &path,      path_cursor,  focused == SearchField::Path,     "Path");
    render_input(f, rows[3],   &name,      name_cursor,  focused == SearchField::Name,     "Filename (wildcard)");
    render_input(f, halves[0], &find_text, text_cursor,  focused == SearchField::FindText, "Find text");
    render_input(f, halves[1], &find_hex,  hex_cursor,   focused == SearchField::FindHex,  "Find hex (90 FF 00)");

    // Store field rects for mouse hit-testing
    if let Some(d) = &mut app.search_dialog {
        d.path_rect = rows[1];
        d.name_rect = rows[3];
        d.text_rect = halves[0];
        d.hex_rect  = halves[1];
    }
}

// ── Search results panel ──────────────────────────────────────────────────────

fn render_search_results_panel(f: &mut Frame, app: &mut App, area: Rect, active: bool, theme: &Theme) {
    let (results_len, selected, scroll, running, summary, anim_tick) = match &app.search_results {
        Some(sr) => (sr.results.len(), sr.selected, sr.scroll, sr.running, sr.summary.clone(), sr.anim_tick),
        None => return,
    };

    let hint = if running {
        " Enter/F3 view  \u{2502}  Space mark  \u{2502}  p print  \u{2502}  Esc stop "
    } else {
        " Enter/F3 view  \u{2502}  F5 copy  \u{2502}  Space mark  \u{2502}  p print  \u{2502}  Esc close "
    };

    let status = if running {
        format!(" Search: {}  [{} found…] ", summary, results_len)
    } else {
        format!(" Search: {}  [{} found] ", summary, results_len)
    };

    let border_style = Style::default().fg(if active { theme.panel_border } else { theme.panel_fg });
    let block = Block::default()
        .borders(Borders::ALL)
        .title(Span::styled(status, Style::default().fg(theme.panel_title)))
        .title_bottom(Span::styled(
            hint,
            Style::default().fg(theme.panel_fg).bg(theme.panel_bg),
        ))
        .border_style(border_style)
        .style(theme.panel_style());

    let inner = block.inner(area);
    f.render_widget(block, area);

    // Split inner: when running, reserve bottom row for progress bar
    let (list_area, bar_area) = if running && inner.height >= 2 {
        let la = Rect::new(inner.x, inner.y, inner.width, inner.height - 1);
        let ba = Rect::new(inner.x, inner.y + inner.height - 1, inner.width, 1);
        (la, Some(ba))
    } else {
        (inner, None)
    };

    let height = list_area.height as usize;

    // Adjust scroll
    let mut scroll = scroll;
    if selected < scroll { scroll = selected; }
    if results_len > 0 && selected >= scroll + height { scroll = selected + 1 - height; }
    if let Some(sr) = &mut app.search_results { sr.scroll = scroll; }

    if results_len == 0 {
        let msg = if running { "  Searching…" } else { "  No results." };
        f.render_widget(Paragraph::new(msg).style(theme.panel_style()), list_area);
    } else {
        let max_w = list_area.width.saturating_sub(2) as usize;
        let results = match &app.search_results { Some(sr) => &sr.results as *const Vec<SearchResult>, None => return };
        let marked: *const std::collections::HashSet<usize> = match &app.search_results { Some(sr) => &sr.marked, None => return };
        // SAFETY: we only read, no mutation, and borrow ends before next draw
        let results = unsafe { &*results };
        let marked  = unsafe { &*marked };

        let items: Vec<ListItem> = results.iter().enumerate()
            .skip(scroll).take(height)
            .map(|(i, r)| {
                let is_sel    = i == selected;
                let is_marked = marked.contains(&i);
                let path_str  = r.path.display().to_string();
                let label = match &r.kind {
                    SearchResultKind::NameMatch => {
                        format!(" {} ", truncate(&path_str, max_w))
                    }
                    SearchResultKind::TextMatch { line_num, line } => {
                        let prefix = format!("{}:{} ", path_str, line_num);
                        let rest   = truncate(line.trim(), max_w.saturating_sub(prefix.len()));
                        format!(" {}{} ", prefix, rest)
                    }
                    SearchResultKind::HexMatch { offset } => {
                        format!(" {} @ 0x{:X} ", truncate(&path_str, max_w.saturating_sub(14)), offset)
                    }
                };
                let style = if is_sel {
                    theme.selected_style()
                } else if is_marked {
                    Style::default().fg(Color::Yellow).bg(theme.panel_bg)
                } else {
                    theme.panel_style()
                };
                ListItem::new(label).style(style)
            })
            .collect();

        f.render_widget(List::new(items), list_area);
    }

    // Animated indeterminate progress bar
    if let Some(ba) = bar_area {
        let bar_w = ba.width.saturating_sub(2) as usize; // 1 space each side
        if bar_w > 0 {
            let block_w = (bar_w / 5).max(3).min(bar_w);
            let span    = bar_w.saturating_sub(block_w);
            let pos = if span == 0 {
                0
            } else {
                let t = (anim_tick as usize * 2) % (span * 2);
                if t > span { span * 2 - t } else { t }
            };
            let bar_str = format!(
                " {}{}{} ",
                " ".repeat(pos),
                "\u{2588}".repeat(block_w),
                " ".repeat(span - pos),
            );
            f.render_widget(
                Paragraph::new(bar_str)
                    .style(Style::default().fg(theme.panel_title).bg(theme.panel_bg)),
                ba,
            );
        }
    }
}

fn render_search_stop_dialog(f: &mut Frame, app: &mut App, area: Rect, theme: &Theme) {
    let results_len = app.search_results.as_ref().map(|s| s.results.len()).unwrap_or(0);

    let popup = centered_rect(42, 7, area);
    f.render_widget(Clear, popup);

    let block = Block::default()
        .borders(Borders::ALL)
        .title(Span::styled(" Stop search? ", Style::default().fg(theme.panel_title)))
        .border_style(Style::default().fg(theme.panel_border))
        .style(Style::default().bg(theme.menu_bg).fg(theme.menu_fg));
    let inner = block.inner(popup);
    f.render_widget(block, popup);

    // Line 1: result count
    let count_line = format!("  {} result(s) found so far.", results_len);
    f.render_widget(
        Paragraph::new(count_line).style(Style::default().fg(theme.menu_fg).bg(theme.menu_bg)),
        Rect::new(inner.x, inner.y, inner.width, 1),
    );

    // Line 2: hint
    f.render_widget(
        Paragraph::new("  Esc — continue searching")
            .style(Style::default().fg(theme.panel_fg).bg(theme.menu_bg)),
        Rect::new(inner.x, inner.y + 1, inner.width, 1),
    );

    // Line 4: buttons
    if inner.height >= 4 {
        let btn_y = inner.y + 3;
        let keep_label    = "[ Keep results ]";
        let discard_label = "[ Discard ]";
        let keep_x    = inner.x + 2;
        let discard_x = inner.x + 2 + keep_label.len() as u16 + 2;

        let keep_rect    = Rect::new(keep_x,    btn_y, keep_label.len() as u16,    1);
        let discard_rect = Rect::new(discard_x, btn_y, discard_label.len() as u16, 1);

        f.render_widget(
            Paragraph::new(keep_label)
                .style(Style::default().fg(theme.btn_key_fg).bg(theme.btn_key_bg)),
            keep_rect,
        );
        f.render_widget(
            Paragraph::new(discard_label)
                .style(Style::default().fg(theme.btn_key_fg).bg(theme.btn_key_bg)),
            discard_rect,
        );

        app.search_stop_keep_rect    = keep_rect;
        app.search_stop_discard_rect = discard_rect;
    }
}

fn render_goto_paste_menu(f: &mut Frame, rect: Rect, theme: &Theme) {
    let area = Rect::new(rect.x, rect.y, 9, 3);
    f.render_widget(Clear, area);
    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(theme.panel_border))
        .style(Style::default().bg(theme.menu_bg).fg(theme.menu_fg));
    let inner = block.inner(area);
    f.render_widget(block, area);
    f.render_widget(
        Paragraph::new(" Paste ").style(Style::default().bg(theme.menu_bg).fg(theme.menu_fg)),
        inner,
    );
}

fn render_context_menu(f: &mut Frame, app: &mut App) {
    let theme = app.theme.clone();

    let (x, y, width, height, items): (u16, u16, u16, u16, Vec<(String, bool)>) = {
        let m = match &app.context_menu { Some(m) => m, None => return };
        let w = m.items.iter().map(|(s, _)| s.len() as u16).max().unwrap_or(8) + 4;
        let h = m.items.len() as u16 + 2;
        let labels = m.items.iter().enumerate()
            .map(|(i, (label, _))| (label.clone(), i == m.selected))
            .collect();
        (m.x, m.y, w, h, labels)
    };

    let area = f.area();
    let x = x.min(area.width.saturating_sub(width));
    let y = y.min(area.height.saturating_sub(height));
    let rect = Rect::new(x, y, width, height);

    f.render_widget(Clear, rect);

    let list_items: Vec<ListItem> = items.iter().map(|(label, selected)| {
        let style = if *selected { theme.menu_selected_style() } else { theme.menu_style() };
        ListItem::new(format!(" {} ", label)).style(style)
    }).collect();

    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(theme.panel_border))
        .style(Style::default().bg(theme.menu_bg).fg(theme.menu_fg));

    f.render_widget(List::new(list_items).block(block), rect);

    if let Some(m) = &mut app.context_menu {
        m.rect = rect;
    }
}

// ── History popup ─────────────────────────────────────────────────────────────

fn render_history_popup(f: &mut Frame, app: &mut App, theme: &Theme) {
    let (entries, selected, scroll) = match &app.history_popup {
        Some(p) => (p.entries.clone(), p.selected, p.scroll),
        None => return,
    };

    if entries.is_empty() {
        app.history_popup = None;
        return;
    }

    let max_visible: usize = 15;
    let visible = entries.len().min(max_visible);
    let popup_h = visible as u16 + 3;
    let popup_w: u16 = 66;
    let area = f.area();
    let rect = centered_rect(popup_w, popup_h, area);

    // Adjust scroll so selected is visible
    let mut scroll = scroll;
    if selected < scroll { scroll = selected; }
    if selected >= scroll + visible { scroll = selected + 1 - visible; }
    if let Some(p) = &mut app.history_popup { p.scroll = scroll; }

    f.render_widget(Clear, rect);

    let block = Block::default()
        .borders(Borders::ALL)
        .title(Span::styled(" Directory History ", Style::default().fg(theme.panel_title)))
        .title_bottom(Span::styled(
            " Enter navigate  \u{2502}  Esc close ",
            Style::default().fg(theme.panel_fg).bg(theme.panel_bg),
        ))
        .border_style(Style::default().fg(theme.panel_border))
        .style(theme.panel_style());

    let inner = block.inner(rect);
    f.render_widget(block, rect);

    let tag_style = Style::default().fg(theme.dir_fg).bg(theme.panel_bg);
    let items: Vec<ListItem> = entries.iter()
        .enumerate()
        .skip(scroll)
        .take(visible)
        .map(|(i, (path, is_left))| {
            let tag = if *is_left { "[L]" } else { "[R]" };
            let path_str = clean_path(path);
            let max_path = inner.width.saturating_sub(6) as usize; // 4 tag + 1 space + 1 pad
            let label = format!(" {} {} ", tag, truncate(&path_str, max_path));
            let style = if i == selected { theme.selected_style() } else { theme.panel_style() };
            if i == selected {
                ListItem::new(label).style(style)
            } else {
                // Color the tag differently from the path
                let tag_span = Span::styled(format!(" {} ", tag), tag_style);
                let path_span = Span::styled(format!("{} ", truncate(&path_str, max_path)), theme.panel_style());
                ListItem::new(Line::from(vec![tag_span, path_span])).style(style)
            }
        })
        .collect();

    f.render_widget(List::new(items), inner);

    if let Some(p) = &mut app.history_popup {
        p.rect = rect;
    }
}

// ── Bookmark popup ────────────────────────────────────────────────────────────

fn render_bookmark_popup(f: &mut Frame, app: &mut App, theme: &Theme) {
    let (entries, selected, scroll, target_panel) = match &app.bookmark_popup {
        Some(p) => (p.entries.clone(), p.selected, p.scroll, p.target_panel.clone()),
        None => return,
    };

    let max_visible: usize = 15;
    let visible = entries.len().min(max_visible);
    let popup_h = visible as u16 + 4; // +1 for tab bar row
    let popup_w: u16 = 66;
    let area = f.area();
    let rect = centered_rect(popup_w, popup_h, area);

    let mut scroll = scroll;
    if selected < scroll { scroll = selected; }
    if selected >= scroll + visible { scroll = selected + 1 - visible; }
    if let Some(p) = &mut app.bookmark_popup { p.scroll = scroll; }

    f.render_widget(Clear, rect);

    let block = Block::default()
        .borders(Borders::ALL)
        .title(Span::styled(" Bookmarks ", Style::default().fg(theme.panel_title)))
        .title_bottom(Span::styled(
            " Enter/DblClick  \u{2502}  Tab switch panel  \u{2502}  Del remove  \u{2502}  Esc close ",
            Style::default().fg(theme.panel_fg).bg(theme.panel_bg),
        ))
        .border_style(Style::default().fg(theme.panel_border))
        .style(theme.panel_style());

    let inner = block.inner(rect);
    f.render_widget(block, rect);

    // Split inner into tab bar (1 row) + list
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(1), Constraint::Min(0)])
        .split(inner);

    // Tab bar: Left / Right toggle
    let left_style  = if target_panel == PanelSide::Left  { theme.selected_style() } else { theme.panel_style() };
    let right_style = if target_panel == PanelSide::Right { theme.selected_style() } else { theme.panel_style() };
    let tab_line = Line::from(vec![
        Span::styled(" Left ",  left_style),
        Span::styled(" \u{2502} ", Style::default().fg(theme.panel_border)),
        Span::styled(" Right ", right_style),
    ]);
    f.render_widget(Paragraph::new(tab_line), chunks[0]);

    // Bookmark list
    let items: Vec<ListItem> = entries.iter()
        .enumerate()
        .skip(scroll)
        .take(visible)
        .map(|(i, path)| {
            let max_w = inner.width.saturating_sub(2) as usize;
            let label = format!(" {} ", truncate(path, max_w));
            let style = if i == selected { theme.selected_style() } else { theme.panel_style() };
            ListItem::new(label).style(style)
        })
        .collect();

    f.render_widget(List::new(items), chunks[1]);

    if let Some(p) = &mut app.bookmark_popup {
        p.rect = rect;
    }
}

// ── Log full-screen view ──────────────────────────────────────────────────────

fn render_log_view(f: &mut Frame, app: &mut App, area: Rect) {
    let theme = app.theme.clone();

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Min(0), Constraint::Length(1)])
        .split(area);

    let (selected, scroll) = match &app.log_popup {
        Some(p) => (p.selected, p.scroll),
        None => return,
    };

    let count = app.op_log.len();
    let title = if count == 0 {
        " File Operation Log — empty ".to_string()
    } else {
        format!(" File Operation Log  [{}/{}] ", selected + 1, count)
    };

    let block = Block::default()
        .borders(Borders::ALL)
        .title(Span::styled(title, Style::default().fg(theme.panel_title)))
        .border_style(Style::default().fg(theme.panel_border))
        .style(theme.panel_style());

    let inner = block.inner(chunks[0]);
    f.render_widget(block, chunks[0]);

    // Status bar
    let hint = " \u{2191}\u{2193} navigate  \u{2502}  c copy entry  \u{2502}  Esc close ";
    f.render_widget(
        Paragraph::new(hint).style(theme.cmdline_style()),
        chunks[1],
    );

    if count == 0 {
        f.render_widget(
            Paragraph::new("  No operations logged yet.")
                .style(theme.panel_style()),
            inner,
        );
        return;
    }

    let visible = inner.height as usize;

    // Adjust scroll so selected is visible
    let mut scroll = scroll;
    if selected < scroll { scroll = selected; }
    if selected >= scroll + visible { scroll = selected + 1 - visible; }
    if let Some(p) = &mut app.log_popup { p.scroll = scroll; }

    let op_style = |op: &str| match op {
        "DEL"  => Style::default().fg(Color::Red).bg(theme.panel_bg),
        "COPY" => Style::default().fg(theme.dir_fg).bg(theme.panel_bg),
        "MOV"  => Style::default().fg(theme.panel_title).bg(theme.panel_bg),
        "MKD"  => Style::default().fg(Color::Green).bg(theme.panel_bg),
        "NEW"  => Style::default().fg(theme.dir_fg).bg(theme.panel_bg),
        "REN"  => Style::default().fg(theme.panel_title).bg(theme.panel_bg),
        _      => theme.panel_style(),
    };

    // Full width: time(10) + op(5) = 15 prefix chars, rest is path
    let max_path = inner.width.saturating_sub(15) as usize;

    let items: Vec<ListItem> = app.op_log.iter()
        .enumerate()
        .skip(scroll)
        .take(visible)
        .map(|(i, entry)| {
            let row_style = if i == selected { theme.selected_style() } else { theme.panel_style() };
            let time_span = Span::styled(format!(" [{}] ", entry.time), row_style);
            let op_span   = Span::styled(format!("{:<4} ", entry.op), if i == selected { row_style } else { op_style(entry.op) });
            let path_span = if let Some(dest) = &entry.dest {
                // For operations with dest: show full src → dest, no truncation on src
                let arrow = " \u{2192} ";
                let dest_max = max_path.saturating_sub(entry.src.len() + arrow.len()).max(10);
                let src_max  = max_path.saturating_sub(dest_max + arrow.len()).max(10);
                Span::styled(
                    format!("{}{}{}", truncate(&entry.src, src_max), arrow, truncate(dest, dest_max)),
                    row_style,
                )
            } else {
                Span::styled(entry.src.clone(), row_style)
            };
            ListItem::new(Line::from(vec![time_span, op_span, path_span])).style(row_style)
        })
        .collect();

    f.render_widget(List::new(items), inner);
}

/// Calculate the bounding rects of each tab label in the panel title border row.
fn tab_rects_for(panel: &Panel, area: Rect) -> Vec<Rect> {
    if panel.tab_count() <= 1 { return vec![]; }
    let y = area.y;
    let mut x = area.x + 2; // after '┌' + leading space in title
    panel.tabs.iter().map(|tab| {
        let w = (tab.label().len() as u16) + 2; // " label "
        let r = Rect::new(x, y, w, 1);
        x += w + 1; // +1 for separator space
        r
    }).collect()
}

// ── Helpers ───────────────────────────────────────────────────────────────────

fn file_kind_style(kind: &FileKind, theme: &Theme) -> Style {
    let bg = theme.panel_bg;
    if theme.is_light_bg() {
        match kind {
            FileKind::Dir        => theme.dir_style(),
            FileKind::Executable => Style::default().fg(Color::Green).bg(bg),
            FileKind::Archive    => Style::default().fg(Color::Red).bg(bg),
            FileKind::Document   => Style::default().fg(Color::Rgb(150, 90, 0)).bg(bg),
            FileKind::Image      => Style::default().fg(Color::Rgb(160, 0, 160)).bg(bg),
            FileKind::Media      => Style::default().fg(Color::Rgb(150, 80, 140)).bg(bg),
            FileKind::Text       => Style::default().fg(theme.file_fg).bg(bg),
            FileKind::Source     => Style::default().fg(Color::Rgb(0, 130, 160)).bg(bg),
            FileKind::Other      => theme.file_style(),
        }
    } else {
        let image_fg = if theme.name == "nord" {
            Color::Rgb(180, 142, 173) // Nord aurora muted purple #B48EAD
        } else if theme.name == "monokai" {
            Color::Rgb(249, 38, 114)  // Monokai pink
        } else {
            Color::Rgb(220, 100, 220) // bright pink-magenta for dark theme
        };
        match kind {
            FileKind::Dir        => theme.dir_style(),
            FileKind::Executable => Style::default().fg(Color::LightGreen).bg(bg),
            FileKind::Archive    => Style::default().fg(Color::Rgb(210, 100, 80)).bg(bg),
            FileKind::Document   => Style::default().fg(Color::Yellow).bg(bg),
            FileKind::Image      => Style::default().fg(image_fg).bg(bg),
            FileKind::Media      => Style::default().fg(Color::Rgb(180, 120, 200)).bg(bg),
            FileKind::Text       => Style::default().fg(theme.file_fg).bg(bg),
            FileKind::Source     => Style::default().fg(Color::LightCyan).bg(bg),
            FileKind::Other      => theme.file_style(),
        }
    }
}

fn centered_rect(width: u16, height: u16, area: Rect) -> Rect {
    let vert = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(area.height.saturating_sub(height) / 2),
            Constraint::Length(height),
            Constraint::Min(0),
        ])
        .split(area);

    Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Length(area.width.saturating_sub(width) / 2),
            Constraint::Length(width),
            Constraint::Min(0),
        ])
        .split(vert[1])[1]
}

fn format_date(t: std::time::SystemTime) -> String {
    use std::time::UNIX_EPOCH;
    let secs = t.duration_since(UNIX_EPOCH).unwrap_or_default().as_secs();
    // Days since epoch
    let days = secs / 86400;
    // Approximate year/month/day using a simple algorithm
    let mut y = 1970u32;
    let mut remaining = days;
    loop {
        let leap = (y % 4 == 0 && y % 100 != 0) || y % 400 == 0;
        let days_in_year = if leap { 366 } else { 365 };
        if remaining < days_in_year { break; }
        remaining -= days_in_year;
        y += 1;
    }
    let leap = (y % 4 == 0 && y % 100 != 0) || y % 400 == 0;
    let month_days: [u32; 12] = [31, if leap { 29 } else { 28 }, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31];
    let mut m = 0u32;
    for &md in &month_days {
        if remaining < md as u64 { break; }
        remaining -= md as u64;
        m += 1;
    }
    let d = remaining + 1;
    format!("{:04}-{:02}-{:02}", y, m + 1, d)
}

fn format_size(size: u64) -> String {
    if size < 1_024 {
        format!("{:>5}B", size)
    } else if size < 1_048_576 {
        format!("{:>4}KB", size / 1_024)
    } else if size < 1_073_741_824 {
        format!("{:>4}MB", size / 1_048_576)
    } else {
        format!("{:>4}GB", size / 1_073_741_824)
    }
}

fn truncate(s: &str, max: usize) -> String {
    if max == 0 { return String::new(); }
    if s.len() <= max { return s.to_string(); }
    format!("{}~", &s[..max.saturating_sub(1)])
}
