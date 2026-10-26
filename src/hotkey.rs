use gtk4::gio;
use gtk4::glib;
use gtk4::prelude::*;

const SCHEMA: &str = "org.gnome.settings-daemon.plugins.media-keys";
const ITEM_SCHEMA: &str = "org.gnome.settings-daemon.plugins.media-keys.custom-keybinding";
pub const PATH: &str = "/org/gnome/settings-daemon/plugins/media-keys/custom-keybindings/wxshot/";

/// Turn human friendly shortcuts ("F1", "Ctrl+Alt+A") into the accelerator
/// format GNOME expects ("<F1>", "<Control><Alt>a").
pub fn normalize(input: &str) -> Result<String, String> {
    let mut parts: Vec<&str> = input
        .split('+')
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .collect();
    let key = parts.pop().ok_or_else(|| "快捷键为空".to_string())?;

    let mut mods = String::new();
    for part in parts {
        match part.to_ascii_lowercase().as_str() {
            "ctrl" | "control" => mods.push_str("<Control>"),
            "alt" | "option" | "opt" => mods.push_str("<Alt>"),
            "shift" => mods.push_str("<Shift>"),
            "super" | "win" | "meta" | "cmd" | "command" => mods.push_str("<Super>"),
            other => return Err(format!("无法识别的修饰键: {other}")),
        }
    }

    let key_norm = if key.chars().count() == 1 {
        key.to_ascii_lowercase()
    } else {
        match key.to_ascii_lowercase().as_str() {
            "space" => "space".to_string(),
            "escape" | "esc" => "Escape".to_string(),
            "enter" | "return" => "Return".to_string(),
            "print" | "printscreen" => "Print".to_string(),
            _ => {
                let upper = key.to_ascii_uppercase();
                if upper.starts_with('F') && upper[1..].parse::<u8>().is_ok() {
                    // gsd-media-keys 只接受裸的 F1，写成 <F1> 会 grab 失败
                    upper
                } else {
                    key.to_string()
                }
            }
        }
    };

    Ok(format!("{mods}{key_norm}"))
}

/// 当前系统是否提供 GNOME 的媒体键 schema（KDE 等桌面没有）。
pub fn available() -> bool {
    let source = gio::SettingsSchemaSource::default();
    let has = |id: &str| {
        source
            .as_ref()
            .and_then(|s| s.lookup(id, true))
            .is_some()
    };
    has(SCHEMA) && has(ITEM_SCHEMA)
}

pub fn apply(hotkey: &str, command: &str) -> Result<(), String> {
    if !available() {
        return Err(
            "当前桌面没有 GNOME 快捷键设置（KDE 下由 GlobalShortcuts portal 注册）".to_string(),
        );
    }
    let binding = normalize(hotkey)?;
    let settings = gio::Settings::new(SCHEMA);

    let mut paths: Vec<glib::GString> = settings
        .strv("custom-keybindings")
        .iter()
        .map(|s| glib::GString::from(s.as_str()))
        .collect();
    if !paths.iter().any(|p| p.as_str() == PATH) {
        paths.push(glib::GString::from(PATH));
        settings
            .set_strv("custom-keybindings", paths)
            .map_err(|e| format!("写入快捷键列表失败: {e}"))?;
    }

    let item = gio::Settings::with_path(ITEM_SCHEMA, PATH);
    item.set_string("name", "截图（wxshot）")
        .map_err(|e| format!("写入快捷键名称失败: {e}"))?;
    item.set_string("command", command)
        .map_err(|e| format!("写入快捷键命令失败: {e}"))?;
    item.set_string("binding", &binding)
        .map_err(|e| format!("写入快捷键失败: {e}"))?;

    // GSettings 的写入是延迟的，进程立刻退出会丢；显式刷盘。
    gio::Settings::sync();

    Ok(())
}

pub fn remove_binding() {
    if !available() {
        return;
    }
    let settings = gio::Settings::new(SCHEMA);
    let mut paths: Vec<glib::GString> = settings
        .strv("custom-keybindings")
        .iter()
        .map(|s| glib::GString::from(s.as_str()))
        .collect();
    let before = paths.len();
    paths.retain(|p| p.as_str() != PATH);
    if paths.len() != before {
        let _ = settings.set_strv("custom-keybindings", paths);
    }
}
