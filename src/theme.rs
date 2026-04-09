use ratatui::style::{Color, Style};

#[derive(Clone)]
pub struct Theme {
    pub name: String,
    pub panel_bg: Color,
    pub panel_fg: Color,
    pub panel_border: Color,
    pub panel_title: Color,
    pub selected_bg: Color,
    pub selected_fg: Color,
    pub dir_fg: Color,
    pub file_fg: Color,
    pub menu_bg: Color,
    pub menu_fg: Color,
    pub menu_selected_bg: Color,
    pub menu_selected_fg: Color,
    pub btn_key_bg: Color,
    pub btn_key_fg: Color,
    pub btn_label_bg: Color,
    pub btn_label_fg: Color,
    pub cmdline_bg: Color,
    pub cmdline_fg: Color,
}

impl Theme {
    pub fn dark() -> Self {
        Self {
            name: "dark".to_string(),
            panel_bg: Color::Rgb(46, 52, 64),
            panel_fg: Color::Rgb(216, 222, 233),
            panel_border: Color::Rgb(136, 192, 208),
            panel_title: Color::Rgb(235, 203, 139),
            selected_bg: Color::Rgb(67, 76, 94),
            selected_fg: Color::Rgb(236, 239, 244),
            dir_fg: Color::Rgb(136, 192, 208),
            file_fg: Color::Rgb(216, 222, 233),
            menu_bg: Color::Rgb(36, 41, 51),
            menu_fg: Color::Rgb(216, 222, 233),
            menu_selected_bg: Color::Rgb(136, 192, 208),
            menu_selected_fg: Color::Rgb(36, 41, 51),
            btn_key_bg: Color::Rgb(94, 129, 172),
            btn_key_fg: Color::Rgb(236, 239, 244),
            btn_label_bg: Color::Rgb(46, 52, 64),
            btn_label_fg: Color::Rgb(216, 222, 233),
            cmdline_bg: Color::Rgb(36, 41, 51),
            cmdline_fg: Color::Rgb(216, 222, 233),
        }
    }

    pub fn light() -> Self {
        Self {
            name: "light".to_string(),
            panel_bg: Color::White,
            panel_fg: Color::Black,
            panel_border: Color::Blue,
            panel_title: Color::Blue,
            selected_bg: Color::Blue,
            selected_fg: Color::White,
            dir_fg: Color::Blue,
            file_fg: Color::Black,
            menu_bg: Color::Gray,
            menu_fg: Color::Black,
            menu_selected_bg: Color::Blue,
            menu_selected_fg: Color::White,
            btn_key_bg: Color::Blue,
            btn_key_fg: Color::White,
            btn_label_bg: Color::Rgb(210, 210, 210),
            btn_label_fg: Color::Black,
            cmdline_bg: Color::White,
            cmdline_fg: Color::Black,
        }
    }

    pub fn monokai() -> Self {
        Self {
            name: "monokai".to_string(),
            panel_bg: Color::Rgb(39, 40, 34),
            panel_fg: Color::Rgb(248, 248, 242),
            panel_border: Color::Rgb(102, 217, 239),
            panel_title: Color::Rgb(230, 219, 116),
            selected_bg: Color::Rgb(73, 72, 62),
            selected_fg: Color::Rgb(248, 248, 242),
            dir_fg: Color::Rgb(102, 217, 239),
            file_fg: Color::Rgb(248, 248, 242),
            menu_bg: Color::Rgb(30, 31, 26),
            menu_fg: Color::Rgb(248, 248, 242),
            menu_selected_bg: Color::Rgb(102, 217, 239),
            menu_selected_fg: Color::Rgb(30, 31, 26),
            btn_key_bg: Color::Rgb(166, 226, 46),
            btn_key_fg: Color::Rgb(30, 31, 26),
            btn_label_bg: Color::Rgb(39, 40, 34),
            btn_label_fg: Color::Rgb(248, 248, 242),
            cmdline_bg: Color::Rgb(30, 31, 26),
            cmdline_fg: Color::Rgb(248, 248, 242),
        }
    }

    pub fn nord() -> Self {
        Self {
            name: "nord".to_string(),
            panel_bg: Color::Blue,
            panel_fg: Color::White,
            panel_border: Color::Cyan,
            panel_title: Color::Yellow,
            selected_bg: Color::Cyan,
            selected_fg: Color::Black,
            dir_fg: Color::White,
            file_fg: Color::White,
            menu_bg: Color::Cyan,
            menu_fg: Color::Black,
            menu_selected_bg: Color::Black,
            menu_selected_fg: Color::White,
            btn_key_bg: Color::Black,
            btn_key_fg: Color::White,
            btn_label_bg: Color::Cyan,
            btn_label_fg: Color::Black,
            cmdline_bg: Color::Black,
            cmdline_fg: Color::White,
        }
    }

    pub fn by_name(name: &str) -> Self {
        match name {
            "light" => Self::light(),
            "monokai" => Self::monokai(),
            "nord" => Self::nord(),
            _ => Self::dark(),
        }
    }

    pub fn all_names() -> &'static [&'static str] {
        &["dark", "light", "monokai", "nord"]
    }

    pub fn panel_style(&self) -> Style {
        Style::default().fg(self.panel_fg).bg(self.panel_bg)
    }

    pub fn selected_style(&self) -> Style {
        Style::default().fg(self.selected_fg).bg(self.selected_bg)
    }

    pub fn dir_style(&self) -> Style {
        Style::default().fg(self.dir_fg).bg(self.panel_bg)
    }

    pub fn file_style(&self) -> Style {
        Style::default().fg(self.file_fg).bg(self.panel_bg)
    }

    pub fn menu_style(&self) -> Style {
        Style::default().fg(self.menu_fg).bg(self.menu_bg)
    }

    pub fn menu_selected_style(&self) -> Style {
        Style::default().fg(self.menu_selected_fg).bg(self.menu_selected_bg)
    }

    pub fn cmdline_style(&self) -> Style {
        Style::default().fg(self.cmdline_fg).bg(self.cmdline_bg)
    }

    pub fn is_light_bg(&self) -> bool {
        matches!(self.panel_bg, Color::White)
    }

    /// Separator color: panel_border unless it matches menu_bg (would be invisible)
    pub fn menu_sep_fg(&self) -> Color {
        if self.panel_border == self.menu_bg { self.menu_fg } else { self.panel_border }
    }
}
