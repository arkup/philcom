use anyhow::Result;
use std::collections::HashSet;
use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

// ── Sort ──────────────────────────────────────────────────────────────────────

#[derive(Clone, PartialEq)]
pub enum SortBy { Name, Size, Date, Ext }

impl SortBy {
    pub fn next(&self) -> Self {
        match self {
            SortBy::Name => SortBy::Size,
            SortBy::Size => SortBy::Date,
            SortBy::Date => SortBy::Ext,
            SortBy::Ext  => SortBy::Name,
        }
    }
    pub fn label(&self) -> &'static str {
        match self { SortBy::Name => "Name", SortBy::Size => "Size", SortBy::Date => "Date", SortBy::Ext => "Ext" }
    }
}

// ── Per-tab state ─────────────────────────────────────────────────────────────

pub struct TabState {
    pub path:     PathBuf,
    pub entries:  Vec<Entry>,
    pub selected: usize,
    pub scroll:   usize,
    pub marked:   HashSet<String>,
    pub visited:  Vec<PathBuf>,
    pub filter:        String,
    pub filter_active: bool,
    pub sort_by:  SortBy,
    pub sort_asc: bool,
    history:      Vec<(PathBuf, String)>,
}

impl TabState {
    pub fn new(path: PathBuf) -> Result<Self> {
        let mut t = Self {
            path,
            entries:  Vec::new(),
            selected: 0,
            scroll:   0,
            marked:   HashSet::new(),
            visited:  Vec::new(),
            filter:        String::new(),
            filter_active: false,
            sort_by:  SortBy::Name,
            sort_asc: true,
            history:  Vec::new(),
        };
        t.load_entries()?;
        t.record_visit();
        Ok(t)
    }

    pub fn load_entries(&mut self) -> Result<()> {
        self.entries.clear();
        self.marked.clear();
        self.entries.push(Entry {
            name: "..".to_string(), is_dir: true, size: 0,
            modified: SystemTime::UNIX_EPOCH, kind: FileKind::Dir,
        });

        let mut dirs  = Vec::new();
        let mut files = Vec::new();

        for entry in fs::read_dir(&self.path)? {
            let entry = entry?;
            let meta  = entry.metadata()?;
            let name  = entry.file_name().to_string_lossy().to_string();
            let is_dir   = meta.is_dir();
            let size     = if is_dir { 0 } else { meta.len() };
            let modified = meta.modified().unwrap_or(SystemTime::UNIX_EPOCH);
            let kind     = if is_dir { FileKind::Dir } else { detect_file_kind(&name, &meta) };
            let e = Entry { name, is_dir, size, modified, kind };
            if is_dir { dirs.push(e); } else { files.push(e); }
        }

        self.sort_vecs(&mut dirs, &mut files);
        self.entries.extend(dirs);
        self.entries.extend(files);
        Ok(())
    }

    fn sort_vecs(&self, dirs: &mut Vec<Entry>, files: &mut Vec<Entry>) {
        let asc = self.sort_asc;
        let cmp = |a: &Entry, b: &Entry| -> std::cmp::Ordering {
            let ord = match &self.sort_by {
                SortBy::Name => a.name.to_lowercase().cmp(&b.name.to_lowercase()),
                SortBy::Size => a.size.cmp(&b.size),
                SortBy::Date => a.modified.cmp(&b.modified),
                SortBy::Ext  => {
                    let ea = a.name.rsplit('.').next().unwrap_or("").to_lowercase();
                    let eb = b.name.rsplit('.').next().unwrap_or("").to_lowercase();
                    ea.cmp(&eb).then(a.name.to_lowercase().cmp(&b.name.to_lowercase()))
                }
            };
            if asc { ord } else { ord.reverse() }
        };
        dirs.sort_by(cmp);
        files.sort_by(cmp);
    }

    pub fn cycle_sort(&mut self) {
        let new_sort = self.sort_by.next();
        if new_sort == self.sort_by {
            self.sort_asc = !self.sort_asc;
        } else {
            self.sort_by = new_sort;
            self.sort_asc = true;
        }
        self.selected = 0;
        self.scroll = 0;
        let _ = self.load_entries();
    }

    pub fn toggle_sort_dir(&mut self) {
        self.sort_asc = !self.sort_asc;
        self.selected = 0;
        self.scroll = 0;
        let _ = self.load_entries();
    }

    pub fn record_visit(&mut self) {
        let path = self.path.clone();
        self.visited.retain(|p| p != &path);
        self.visited.insert(0, path);
        if self.visited.len() > 50 { self.visited.truncate(50); }
    }

    pub fn move_up(&mut self) {
        if self.selected > 0 {
            self.selected -= 1;
            if self.selected < self.scroll { self.scroll = self.selected; }
        }
    }

    pub fn move_down(&mut self) {
        if self.selected + 1 < self.entries.len() { self.selected += 1; }
    }

    pub fn toggle_mark(&mut self) {
        if let Some(entry) = self.entries.get(self.selected) {
            if entry.name == ".." { return; }
            if self.marked.contains(&entry.name) {
                self.marked.remove(&entry.name);
            } else {
                self.marked.insert(entry.name.clone());
            }
            self.move_down();
        }
    }

    pub fn enter_selected(&mut self) {
        if let Some(entry) = self.entries.get(self.selected) {
            if entry.is_dir {
                let new_path = if entry.name == ".." {
                    self.path.parent().map(|p| p.to_path_buf())
                } else {
                    Some(self.path.join(&entry.name))
                };
                if let Some(path) = new_path {
                    if entry.name != ".." {
                        self.history.push((self.path.clone(), entry.name.clone()));
                    }
                    self.path = path;
                    self.selected = 0;
                    self.scroll = 0;
                    let _ = self.load_entries();
                    self.record_visit();
                }
            }
        }
    }

    pub fn go_back(&mut self) -> bool {
        if let Some((prev_path, prev_name)) = self.history.pop() {
            self.path = prev_path;
            self.scroll = 0;
            let _ = self.load_entries();
            self.record_visit();
            self.selected = self.entries.iter().position(|e| e.name == prev_name).unwrap_or(0);
            true
        } else {
            false
        }
    }

    pub fn effective_targets(&self) -> Vec<PathBuf> {
        if !self.marked.is_empty() {
            self.entries.iter()
                .filter(|e| self.marked.contains(&e.name))
                .map(|e| self.path.join(&e.name))
                .collect()
        } else if let Some(entry) = self.entries.get(self.selected) {
            if entry.name != ".." { vec![self.path.join(&entry.name)] } else { vec![] }
        } else {
            vec![]
        }
    }

    pub fn visible_entries(&self, height: usize) -> &[Entry] {
        let end = (self.scroll + height).min(self.entries.len());
        &self.entries[self.scroll..end]
    }

    /// Entries matching the current filter (or all entries if filter is empty).
    pub fn filtered_entries(&self) -> Vec<&Entry> {
        if self.filter.is_empty() {
            self.entries.iter().collect()
        } else {
            let needle = self.filter.to_lowercase();
            self.entries.iter().filter(|e| {
                e.name == ".." || e.name.to_lowercase().contains(&needle)
            }).collect()
        }
    }

    pub fn adjust_scroll(&mut self, height: usize) {
        if height == 0 { return; }
        if self.selected >= self.scroll + height {
            self.scroll = self.selected + 1 - height;
        }
    }

    /// Short display label for the tab bar (last path component).
    pub fn label(&self) -> String {
        self.path
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_else(|| "/".to_string())
    }
}

// ── Panel ─────────────────────────────────────────────────────────────────────

pub struct Panel {
    pub tabs:       Vec<TabState>,
    pub active_tab: usize,
    pub filter_exec: bool,
}

impl Panel {
    pub fn new(path: &str) -> Result<Self> {
        let path = PathBuf::from(path).canonicalize()?;
        Ok(Self {
            tabs: vec![TabState::new(path)?],
            active_tab: 0,
            filter_exec: false,
        })
    }

    pub fn new_from_paths(paths: &[String], active: usize) -> Result<Self> {
        let mut tabs = Vec::new();
        for p in paths {
            let path = PathBuf::from(p);
            if path.exists() {
                if let Ok(t) = TabState::new(path) {
                    tabs.push(t);
                }
            }
        }
        if tabs.is_empty() {
            return Self::new(".");
        }
        Ok(Self {
            active_tab: active.min(tabs.len() - 1),
            tabs,
            filter_exec: false,
        })
    }

    // ── Active-tab accessors ────────────────────────────────────────────────

    pub fn tab(&self) -> &TabState { &self.tabs[self.active_tab] }
    pub fn tab_mut(&mut self) -> &mut TabState { &mut self.tabs[self.active_tab] }

    // ── Delegate common operations to active tab ────────────────────────────

    pub fn load_entries(&mut self) -> Result<()> {
        let filter = self.filter_exec;
        let path   = self.tab().path.clone();
        let tab    = self.tab_mut();
        tab.load_entries()?;
        if filter {
            tab.entries.retain(|e| e.is_dir || is_executable_magic(&path.join(&e.name)));
        }
        Ok(())
    }
    pub fn record_visit(&mut self)               { self.tab_mut().record_visit() }
    pub fn move_up(&mut self)                    { self.tab_mut().move_up() }
    pub fn move_down(&mut self)                  { self.tab_mut().move_down() }
    pub fn toggle_mark(&mut self)                { self.tab_mut().toggle_mark() }
    pub fn enter_selected(&mut self)             { self.tab_mut().enter_selected() }
    pub fn go_back(&mut self) -> bool            { self.tab_mut().go_back() }
    pub fn effective_targets(&self) -> Vec<PathBuf> { self.tab().effective_targets() }
    pub fn clear_marks(&mut self)                { self.tab_mut().marked.clear() }
    pub fn visible_entries(&self, h: usize) -> &[Entry] { self.tab().visible_entries(h) }
    pub fn adjust_scroll(&mut self, h: usize)   { self.tab_mut().adjust_scroll(h) }
    pub fn cycle_sort(&mut self)                 { self.tab_mut().cycle_sort() }
    pub fn toggle_sort_dir(&mut self)            { self.tab_mut().toggle_sort_dir() }

    /// Navigate active tab to `path`, push to history so Esc goes back.
    pub fn navigate_to(&mut self, path: PathBuf) {
        let current = self.tab().path.clone();
        let tab = self.tab_mut();
        tab.history.push((current, String::new()));
        tab.path = path;
        tab.selected = 0;
        tab.scroll = 0;
        let _ = self.load_entries();
        self.record_visit();
    }

    pub fn set_exec_filter(&mut self, on: bool) {
        self.filter_exec = on;
        self.tab_mut().selected = 0;
        self.tab_mut().scroll = 0;
        let _ = self.load_entries();
    }

    // ── Tab management ──────────────────────────────────────────────────────

    pub fn new_tab(&mut self) {
        let path = self.tab().path.clone();
        if let Ok(t) = TabState::new(path) {
            self.tabs.insert(self.active_tab + 1, t);
            self.active_tab += 1;
        }
    }

    pub fn close_tab(&mut self) {
        if self.tabs.len() <= 1 { return; }
        self.tabs.remove(self.active_tab);
        if self.active_tab >= self.tabs.len() {
            self.active_tab = self.tabs.len() - 1;
        }
    }

    pub fn next_tab(&mut self) {
        self.active_tab = (self.active_tab + 1) % self.tabs.len();
    }

    pub fn prev_tab(&mut self) {
        if self.active_tab == 0 {
            self.active_tab = self.tabs.len() - 1;
        } else {
            self.active_tab -= 1;
        }
    }

    pub fn tab_count(&self) -> usize { self.tabs.len() }
}

// ── File types ────────────────────────────────────────────────────────────────

#[derive(Clone, PartialEq)]
pub enum FileKind {
    Dir, Executable, Archive, Document, Image, Media, Text, Source, Other,
}

pub struct Entry {
    pub name:     String,
    pub is_dir:   bool,
    pub size:     u64,
    pub modified: SystemTime,
    pub kind:     FileKind,
}

fn detect_file_kind(name: &str, meta: &fs::Metadata) -> FileKind {
    let ext = name.rsplit('.').next().unwrap_or("").to_ascii_lowercase();
    match ext.as_str() {
        "exe"|"dll"|"com"|"msi"|"bat"|"cmd"|"ps1"|"so"|"dylib"|"lib"|"a"|"o"
            => FileKind::Executable,
        "zip"|"tar"|"gz"|"bz2"|"xz"|"7z"|"rar"|"tgz"|"tbz2"|"zst"|"lz4"|"cab"|"iso"
            => FileKind::Archive,
        "pdf"|"doc"|"docx"|"xls"|"xlsx"|"ppt"|"pptx"|"odt"|"ods"|"odp"|"epub"|"pages"|"numbers"
            => FileKind::Document,
        "jpg"|"jpeg"|"png"|"gif"|"bmp"|"svg"|"webp"|"ico"|"tiff"|"tif"|"heic"|"avif"|"raw"
            => FileKind::Image,
        "mp3"|"mp4"|"mov"|"avi"|"mkv"|"wav"|"flac"|"ogg"|"m4a"|"m4v"|"webm"|"wmv"|"aac"|"opus"
            => FileKind::Media,
        "txt"|"md"|"rst"|"log"|"csv"|"tsv"|"toml"|"yaml"|"yml"|"json"|"xml"|"ini"|"cfg"|"conf"|"env"|"properties"
            => FileKind::Text,
        "rs"|"py"|"js"|"ts"|"jsx"|"tsx"|"c"|"cpp"|"cc"|"h"|"hpp"|"go"|"java"|"rb"|"php"|"cs"
        |"swift"|"kt"|"kts"|"lua"|"sh"|"bash"|"zsh"|"fish"|"vim"|"el"|"clj"|"ex"|"exs"|"hs"
        |"ml"|"r"|"dart"|"zig"|"nim"
            => FileKind::Source,
        _ => {
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                if meta.permissions().mode() & 0o111 != 0 { return FileKind::Executable; }
            }
            let _ = meta;
            FileKind::Other
        }
    }
}

fn is_executable_magic(path: &Path) -> bool {
    let mut f = match fs::File::open(path) { Ok(f) => f, Err(_) => return false };
    let mut buf = [0u8; 4];
    let n = match f.read(&mut buf) { Ok(n) => n, Err(_) => return false };
    if n < 2 { return false; }
    if buf[0] == b'M' && buf[1] == b'Z' { return true; }
    if buf[0] == b'#' && buf[1] == b'!' { return true; }
    if n < 4 { return false; }
    if buf[0] == 0x7f && &buf[1..4] == b"ELF" { return true; }
    matches!(buf,
        [0xFE,0xED,0xFA,0xCE]|[0xFE,0xED,0xFA,0xCF]|
        [0xCE,0xFA,0xED,0xFE]|[0xCF,0xFA,0xED,0xFE]|
        [0xCA,0xFE,0xBA,0xBE]
    )
}
