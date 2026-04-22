use ratatui::style::Color;

use crate::config::{AppConfig, ThemeName};

#[derive(Clone, Copy)]
pub struct Palette {
    pub base: Color,
    pub mantle: Color,
    pub crust: Color,
    pub text: Color,
    pub subtext: Color,
    pub blue: Color,
    pub sapphire: Color,
    pub green: Color,
    pub yellow: Color,
    pub red: Color,
    pub mauve: Color,
    pub overlay: Color,
}

impl Palette {
    pub fn from_theme(theme: &ThemeName) -> Self {
        match theme {
            ThemeName::CatppuccinMocha => Self::catppuccin_mocha(),
        }
    }

    pub fn severity_color(&self, value: f64, warning: f64, critical: f64) -> Color {
        if value >= critical {
            self.red
        } else if value >= warning {
            self.yellow
        } else {
            self.green
        }
    }

    pub fn stale_color(&self, stale: bool) -> Color {
        if stale {
            self.yellow
        } else {
            self.subtext
        }
    }

    fn catppuccin_mocha() -> Self {
        Self {
            base: Color::Rgb(30, 30, 46),
            mantle: Color::Rgb(24, 24, 37),
            crust: Color::Rgb(17, 17, 27),
            text: Color::Rgb(205, 214, 244),
            subtext: Color::Rgb(166, 173, 200),
            blue: Color::Rgb(137, 180, 250),
            sapphire: Color::Rgb(116, 199, 236),
            green: Color::Rgb(166, 227, 161),
            yellow: Color::Rgb(249, 226, 175),
            red: Color::Rgb(243, 139, 168),
            mauve: Color::Rgb(203, 166, 247),
            overlay: Color::Rgb(108, 112, 134),
        }
    }
}

pub fn palette(config: &AppConfig) -> Palette {
    Palette::from_theme(&config.theme)
}
