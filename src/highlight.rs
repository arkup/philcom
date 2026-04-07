use ratatui::style::Color;

// ── Language detection ────────────────────────────────────────────────────────

#[derive(Clone, Copy, PartialEq)]
pub enum Lang {
    Rust, C, Cpp, Python, JavaScript, TypeScript, Java, Go, Shell, Unknown,
}

impl Lang {
    pub fn from_ext(ext: &str) -> Self {
        match ext {
            "rs"                          => Lang::Rust,
            "c" | "h"                     => Lang::C,
            "cpp"|"cc"|"cxx"|"hpp"|"hxx" => Lang::Cpp,
            "py"                          => Lang::Python,
            "js" | "jsx" | "mjs"          => Lang::JavaScript,
            "ts" | "tsx"                  => Lang::TypeScript,
            "java"                        => Lang::Java,
            "go"                          => Lang::Go,
            "sh"|"bash"|"zsh"|"fish"     => Lang::Shell,
            _                             => Lang::Unknown,
        }
    }

    fn line_comment(&self) -> &'static str {
        match self {
            Lang::Python | Lang::Shell => "#",
            Lang::Unknown              => "",
            _                          => "//",
        }
    }

    fn block_comment(&self) -> Option<(&'static str, &'static str)> {
        match self {
            Lang::Python | Lang::Shell | Lang::Unknown => None,
            _ => Some(("/*", "*/")),
        }
    }

    fn keywords(&self) -> &'static [&'static str] {
        match self {
            Lang::Rust => &[
                "as","async","await","break","const","continue","crate","dyn","else","enum",
                "extern","false","fn","for","if","impl","in","let","loop","match","mod","move",
                "mut","pub","ref","return","self","Self","static","struct","super","trait","true",
                "type","union","unsafe","use","where","while","yield",
                // common types
                "bool","u8","u16","u32","u64","u128","usize","i8","i16","i32","i64","i128",
                "isize","f32","f64","str","char","String","Vec","Option","Result","Box","Some",
                "None","Ok","Err",
            ],
            Lang::C => &[
                "auto","break","case","char","const","continue","default","do","double","else",
                "enum","extern","float","for","goto","if","inline","int","long","register",
                "return","short","signed","sizeof","static","struct","switch","typedef",
                "union","unsigned","void","volatile","while",
            ],
            Lang::Cpp => &[
                "auto","bool","break","case","catch","char","class","const","constexpr",
                "continue","default","delete","do","double","else","enum","explicit","extern",
                "false","float","for","friend","goto","if","inline","int","long","mutable",
                "namespace","new","nullptr","operator","override","private","protected","public",
                "register","return","short","signed","sizeof","static","struct","switch","template",
                "this","throw","true","try","typedef","typename","union","unsigned","using",
                "virtual","void","volatile","while",
            ],
            Lang::Python => &[
                "and","as","assert","async","await","break","class","continue","def","del",
                "elif","else","except","False","finally","for","from","global","if","import",
                "in","is","lambda","None","nonlocal","not","or","pass","raise","return","self",
                "super","True","try","while","with","yield",
            ],
            Lang::JavaScript | Lang::TypeScript => &[
                "async","await","break","case","catch","class","const","continue","debugger",
                "default","delete","do","else","export","extends","false","finally","for",
                "from","function","if","import","in","instanceof","let","new","null","of",
                "return","static","super","switch","this","throw","true","try","typeof",
                "undefined","var","void","while","with","yield",
            ],
            Lang::Java => &[
                "abstract","assert","boolean","break","byte","case","catch","char","class",
                "const","continue","default","do","double","else","enum","extends","final",
                "finally","float","for","goto","if","implements","import","instanceof","int",
                "interface","long","native","new","null","package","private","protected",
                "public","return","short","static","strictfp","super","switch","synchronized",
                "this","throw","throws","transient","true","false","try","void","volatile","while",
            ],
            Lang::Go => &[
                "break","case","chan","const","continue","default","defer","else","fallthrough",
                "for","func","go","goto","if","import","interface","map","package","range",
                "return","select","struct","switch","type","var",
                "nil","true","false","error","string","int","int8","int16","int32","int64",
                "uint","uint8","uint16","uint32","uint64","uintptr","float32","float64",
                "complex64","complex128","byte","rune","bool","make","new","len","cap","append",
                "copy","delete","close","panic","recover","print","println",
            ],
            Lang::Shell => &[
                "if","then","else","elif","fi","for","while","do","done","case","esac",
                "function","return","local","export","echo","exit","source","in","break",
                "continue","shift","read","set","unset","readonly","trap","eval",
            ],
            Lang::Unknown => &[],
        }
    }
}

// ── Colors ────────────────────────────────────────────────────────────────────

const C_KEYWORD:  Color = Color::Cyan;
const C_STRING:   Color = Color::Yellow;
const C_COMMENT:  Color = Color::DarkGray;
const C_NUMBER:   Color = Color::Rgb(150, 210, 150);
const C_PREPROC:  Color = Color::Rgb(220, 150, 70);
const C_NORMAL:   Color = Color::Reset;

// ── Highlighter ───────────────────────────────────────────────────────────────

pub struct HighlightSpan {
    pub color: Color,
    pub text:  String,
}

pub struct Highlighter {
    pub lang:             Lang,
    pub in_block_comment: bool,
}

impl Highlighter {
    pub fn new(ext: &str) -> Self {
        Self { lang: Lang::from_ext(ext), in_block_comment: false }
    }

    pub fn is_active(&self) -> bool {
        self.lang != Lang::Unknown
    }

    /// Highlight one line (or chunk). Updates `in_block_comment` for next line.
    pub fn highlight(&mut self, text: &str) -> Vec<HighlightSpan> {
        let chars: Vec<char> = text.chars().collect();
        let n = chars.len();
        let mut spans: Vec<HighlightSpan> = Vec::new();
        let mut buf = String::new();
        let mut buf_color = C_NORMAL;
        let mut i = 0;

        macro_rules! push_buf {
            () => {
                if !buf.is_empty() {
                    spans.push(HighlightSpan { color: buf_color, text: std::mem::take(&mut buf) });
                }
            };
        }
        macro_rules! push_colored {
            ($color:expr, $text:expr) => {
                push_buf!();
                spans.push(HighlightSpan { color: $color, text: $text.to_string() });
                buf_color = C_NORMAL;
            };
        }

        // ── Preprocessor line (#include, #define …) ───────────────────────
        let is_preproc = !self.in_block_comment
            && matches!(self.lang, Lang::C | Lang::Cpp)
            && text.trim_start().starts_with('#');

        // ── Block comment continuation ────────────────────────────────────
        if self.in_block_comment {
            let bc_end = self.lang.block_comment().map(|(_, e)| e).unwrap_or("*/");
            if let Some(pos) = find_str(&chars, 0, bc_end) {
                let end = pos + bc_end.len();
                push_colored!(C_COMMENT, &text[..char_byte_end(&chars, end)]);
                self.in_block_comment = false;
                i = end;
            } else {
                return vec![HighlightSpan { color: C_COMMENT, text: text.to_string() }];
            }
        }

        if is_preproc {
            // For #include lines, color <file> or "file" part as string
            let trimmed = text.trim_start();
            if trimmed.starts_with("#include") {
                let directive_end = text.find("include").map(|p| p + 7).unwrap_or(text.len());
                let rest = &text[directive_end..];
                // find the opening < or "
                if let Some(open_pos) = rest.find(|c| c == '<' || c == '"') {
                    let close_char = if rest.chars().nth(open_pos) == Some('<') { '>' } else { '"' };
                    let after_open = open_pos + 1;
                    if let Some(close_pos) = rest[after_open..].find(close_char) {
                        let full_open = directive_end + open_pos;
                        let full_close = directive_end + after_open + close_pos + 1;
                        return vec![
                            HighlightSpan { color: C_PREPROC, text: text[..full_open].to_string() },
                            HighlightSpan { color: C_STRING,  text: text[full_open..full_close].to_string() },
                            HighlightSpan { color: C_PREPROC, text: text[full_close..].to_string() },
                        ];
                    }
                }
            }
            return vec![HighlightSpan { color: C_PREPROC, text: text.to_string() }];
        }

        let lc   = self.lang.line_comment();
        let bc   = self.lang.block_comment();
        let kwds = self.lang.keywords();

        let mut in_string = false;
        let mut string_char = '"';

        while i < n {
            let c = chars[i];

            // ── Inside a string ───────────────────────────────────────────
            if in_string {
                buf.push(c);
                if c == '\\' && i + 1 < n {
                    // escape sequence
                    i += 1;
                    buf.push(chars[i]);
                } else if c == string_char {
                    push_colored!(C_STRING, &buf);
                    in_string = false;
                }
                i += 1;
                continue;
            }

            // ── Line comment ──────────────────────────────────────────────
            if !lc.is_empty() && starts_with_str(&chars, i, lc) {
                push_buf!();
                let rest: String = chars[i..].iter().collect();
                spans.push(HighlightSpan { color: C_COMMENT, text: rest });
                return spans;
            }

            // ── Block comment open ────────────────────────────────────────
            if let Some((bc_start, bc_end)) = bc {
                if starts_with_str(&chars, i, bc_start) {
                    push_buf!();
                    let search_from = i + bc_start.len();
                    if let Some(end_pos) = find_str(&chars, search_from, bc_end) {
                        let end = end_pos + bc_end.len();
                        let s: String = chars[i..end].iter().collect();
                        push_colored!(C_COMMENT, &s);
                        i = end;
                    } else {
                        // Block comment runs to end of line
                        let rest: String = chars[i..].iter().collect();
                        push_buf!();
                        spans.push(HighlightSpan { color: C_COMMENT, text: rest });
                        self.in_block_comment = true;
                        return spans;
                    }
                    continue;
                }
            }

            // ── String / char literal open ────────────────────────────────
            if c == '"' || c == '\'' {
                // Python triple quotes: skip (treat as regular string start for simplicity)
                push_buf!();
                buf_color = C_STRING;
                buf.push(c);
                in_string = true;
                string_char = c;
                i += 1;
                continue;
            }

            // ── Number ────────────────────────────────────────────────────
            if c.is_ascii_digit() || (c == '.' && i + 1 < n && chars[i+1].is_ascii_digit()) {
                push_buf!();
                let start = i;
                // hex
                if c == '0' && i + 1 < n && (chars[i+1] == 'x' || chars[i+1] == 'X') {
                    i += 2;
                    while i < n && chars[i].is_ascii_hexdigit() { i += 1; }
                } else {
                    while i < n && (chars[i].is_ascii_digit() || chars[i] == '.' || chars[i] == '_') { i += 1; }
                    // exponent
                    if i < n && (chars[i] == 'e' || chars[i] == 'E') {
                        i += 1;
                        if i < n && (chars[i] == '+' || chars[i] == '-') { i += 1; }
                        while i < n && chars[i].is_ascii_digit() { i += 1; }
                    }
                    // suffix (u32, f64, usize …)
                    while i < n && chars[i].is_alphanumeric() { i += 1; }
                }
                let num: String = chars[start..i].iter().collect();
                push_colored!(C_NUMBER, &num);
                continue;
            }

            // ── Identifier / keyword ──────────────────────────────────────
            if c.is_alphabetic() || c == '_' {
                push_buf!();
                let start = i;
                while i < n && (chars[i].is_alphanumeric() || chars[i] == '_') { i += 1; }
                let word: String = chars[start..i].iter().collect();
                let color = if kwds.contains(&word.as_str()) { C_KEYWORD } else { C_NORMAL };
                push_colored!(color, &word);
                continue;
            }

            // ── Default character ─────────────────────────────────────────
            if buf_color != C_NORMAL && !buf.is_empty() {
                push_buf!();
                buf_color = C_NORMAL;
            }
            buf.push(c);
            i += 1;
        }

        push_buf!();
        spans
    }
}

// ── Helpers ───────────────────────────────────────────────────────────────────

fn starts_with_str(chars: &[char], pos: usize, pat: &str) -> bool {
    let pat_chars: Vec<char> = pat.chars().collect();
    if pos + pat_chars.len() > chars.len() { return false; }
    chars[pos..pos + pat_chars.len()] == pat_chars[..]
}

fn find_str(chars: &[char], from: usize, pat: &str) -> Option<usize> {
    let pat_chars: Vec<char> = pat.chars().collect();
    let plen = pat_chars.len();
    if plen == 0 { return None; }
    for i in from..chars.len().saturating_sub(plen - 1) {
        if chars[i..i + plen] == pat_chars[..] { return Some(i); }
    }
    None
}

fn char_byte_end(chars: &[char], char_count: usize) -> usize {
    chars[..char_count].iter().map(|c| c.len_utf8()).sum()
}
