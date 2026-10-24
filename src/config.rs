use gtk4::glib;
use std::path::PathBuf;

use crate::model::TextStyle;

#[derive(Clone)]
pub struct Config {
    pub hotkey: String,
    pub save_dir: PathBuf,
    pub text_style: TextStyle,
}

impl Default for Config {
    fn default() -> Self {
        Config {
            hotkey: "F1".to_string(),
            save_dir: default_save_dir(),
            text_style: TextStyle::default(),
        }
    }
}

pub fn default_save_dir() -> PathBuf {
    glib::user_special_dir(glib::UserDirectory::Pictures)
        .unwrap_or_else(|| glib::home_dir().join("Pictures"))
}

pub fn config_path() -> PathBuf {
    glib::user_config_dir().join("wxshot").join("config.ini")
}

pub fn parse_hex_color(input: &str) -> Option<(f64, f64, f64)> {
    let s = input.trim().trim_start_matches('#');
    if s.len() != 6 || !s.chars().all(|c| c.is_ascii_hexdigit()) {
        return None;
    }
    let r = u8::from_str_radix(&s[0..2], 16).ok()? as f64 / 255.0;
    let g = u8::from_str_radix(&s[2..4], 16).ok()? as f64 / 255.0;
    let b = u8::from_str_radix(&s[4..6], 16).ok()? as f64 / 255.0;
    Some((r, g, b))
}

pub fn color_to_hex(color: (f64, f64, f64)) -> String {
    let channel = |v: f64| (v.clamp(0.0, 1.0) * 255.0).round() as u8;
    format!("#{:02X}{:02X}{:02X}", channel(color.0), channel(color.1), channel(color.2))
}

impl Config {
    pub fn load() -> Config {
        let mut cfg = Config::default();
        let path = config_path();
        let kf = glib::KeyFile::new();
        if kf.load_from_file(&path, glib::KeyFileFlags::NONE).is_ok() {
            if let Ok(v) = kf.string("general", "hotkey") {
                if !v.trim().is_empty() {
                    cfg.hotkey = v.to_string();
                }
            }
            if let Ok(v) = kf.string("general", "save_dir") {
                if !v.trim().is_empty() {
                    cfg.save_dir = expand_tilde(v.as_str());
                }
            }
            if let Ok(v) = kf.string("text", "color") {
                if let Some(color) = parse_hex_color(v.as_str()) {
                    cfg.text_style.color = color;
                }
            }
            if let Ok(v) = kf.string("text", "size") {
                if let Ok(size) = v.trim().parse::<f64>() {
                    if size.is_finite() {
                        cfg.text_style.size = size.clamp(12.0, 48.0).round();
                    }
                }
            }
        }
        cfg
    }

    pub fn save(&self) -> Result<(), String> {
        let path = config_path();
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(|e| format!("创建配置目录失败: {e}"))?;
        }
        let kf = glib::KeyFile::new();
        kf.set_string("general", "hotkey", &self.hotkey);
        kf.set_string("general", "save_dir", &self.save_dir.to_string_lossy());
        kf.set_string("text", "color", &color_to_hex(self.text_style.color));
        kf.set_string(
            "text",
            "size",
            &format!("{}", self.text_style.size.round() as i32),
        );
        kf.save_to_file(&path)
            .map_err(|e| format!("写入配置失败: {e}"))
    }
}

pub fn expand_tilde(input: &str) -> PathBuf {
    if let Some(rest) = input.strip_prefix("~/") {
        glib::home_dir().join(rest)
    } else if input == "~" {
        glib::home_dir()
    } else {
        PathBuf::from(input)
    }
}
