mod capture;
mod config;
mod hotkey;
mod io;
mod model;
mod render;
mod ui;

use gtk4 as gtk;
use gtk::cairo;
use gtk::glib;
use gtk::glib::variant::ToVariant;
use gtk::prelude::*;
use std::cell::Cell;
use std::path::PathBuf;
use std::rc::Rc;

const APP_ID: &str = "io.github.wxshot.WxShot";

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let cmd = args.get(1).map(|s| s.as_str()).unwrap_or("");
    match cmd {
        "--help" | "-h" => help(),
        "--version" | "-V" => println!("wxshot {}", env!("CARGO_PKG_VERSION")),
        "--selftest" => {
            if let Err(e) = selftest(args.get(2).map(|s| s.as_str())) {
                eprintln!("selftest 失败: {e}");
                std::process::exit(1);
            }
        }
        "--demo" => {
            if let Err(e) = demo_render() {
                eprintln!("demo 失败: {e}");
                std::process::exit(1);
            }
        }
        "--capture-test" => {
            if let Err(e) = capture_test() {
                eprintln!("截图失败: {e}");
                std::process::exit(1);
            }
        }
        "--set-hotkey" => {
            let Some(keys) = args.get(2) else {
                eprintln!("用法: wxshot --set-hotkey F1");
                std::process::exit(2);
            };
            if let Err(e) = set_hotkey(keys) {
                eprintln!("{e}");
                std::process::exit(1);
            }
        }
        "--install" => {
            if let Err(e) = install() {
                eprintln!("安装失败: {e}");
                std::process::exit(1);
            }
        }
        "--uninstall" => {
            if let Err(e) = uninstall() {
                eprintln!("卸载失败: {e}");
                std::process::exit(1);
            }
        }
        _ => {
            // 被“启动应用程序”(XDG autostart) 拉起时绝不截图：
            // 否则每次登录/重启都会立刻弹出一张全屏遮罩，还得手动 Esc。
            if launched_from_autostart() {
                eprintln!(
                    "wxshot: 检测到由开机自启启动，已跳过截图。\n\
                     wxshot 是一次性截图工具，按快捷键时由 GNOME 直接执行，\
                     无需加入“启动应用程序”。\n\
                     想让登录后立即可用 F1，保留全局快捷键即可；\
                     如需彻底清理自启项，运行: wxshot --install"
                );
                return;
            }
            run_gui()
        }
    }
}

/// 是否由 XDG autostart（GNOME“启动应用程序”）启动。
///
/// GIO 通过 .desktop 启动程序时会设置 `GIO_LAUNCHED_DESKTOP_FILE`，
/// 自启项位于 `$XDG_CONFIG_HOME/autostart/`，据此精确区分“登录自启”
/// 与“用户点击菜单 / 按全局快捷键”（后者是裸 spawn，不带该变量）。
fn launched_from_autostart() -> bool {
    if let Some(path) = std::env::var_os("GIO_LAUNCHED_DESKTOP_FILE") {
        let in_autostart = PathBuf::from(path)
            .parent()
            .and_then(|dir| dir.file_name())
            .map(|name| name == "autostart")
            .unwrap_or(false);
        if in_autostart {
            return true;
        }
    }
    // 老版本 gnome-session 用这个变量标记自启进程
    std::env::var_os("DESKTOP_AUTOSTART_ID").is_some()
}

fn help() {
    println!(
        "wxshot {} — 微信风格截图工具\n\n\
         用法:\n  \
         wxshot                  截图\n  \
         wxshot --set-hotkey F1  修改全局快捷键\n  \
         wxshot --install        安装到 ~/.local/bin 并绑定快捷键\n  \
         wxshot --uninstall      卸载\n  \
         wxshot --capture-test   只截图并保存到 /tmp/wxshot-capture.png\n  \
         wxshot --selftest [图片] 无界面自测（标注 + 导出）\n",
        env!("CARGO_PKG_VERSION")
    );
}

fn run_gui() {
    let app = gtk::Application::builder().application_id(APP_ID).build();
    // 一次只允许存在一个截图会话。截图进行中或遮罩窗口已显示时，
    // 再次按热键只把已有窗口拉回前台，绝不重新截图/新建窗口。
    let active = Rc::new(Cell::new(false));

    app.connect_activate({
        let active = active.clone();
        move |app| {
            if active.replace(true) {
                if let Some(window) = app
                    .active_window()
                    .or_else(|| app.windows().first().cloned())
                {
                    window.present();
                }
                return;
            }

            let image = match capture::grab_screen() {
                Ok(img) => img,
                Err(e) => {
                    eprintln!("截图失败: {e}");
                    active.set(false);
                    app.quit();
                    return;
                }
            };
            let cfg = config::Config::load();
            let window = ui::run(app, image, cfg);
            let active = active.clone();
            window.connect_destroy(move |_| active.set(false));
        }
    });
    let _ = app.run();
}

fn install_path() -> PathBuf {
    glib::home_dir().join(".local/bin/wxshot")
}

fn icon_path() -> PathBuf {
    glib::user_data_dir().join("icons/hicolor/scalable/apps/wxshot.svg")
}

fn desktop_path() -> PathBuf {
    glib::user_data_dir().join("applications/wxshot.desktop")
}

const NOFLASH_UUID: &str = "wxshot-noflash@local";
const NOFLASH_METADATA: &str =
    include_str!("../extensions/wxshot-noflash@local/metadata.json");
const NOFLASH_JS: &str =
    include_str!("../extensions/wxshot-noflash@local/extension.js");

fn install_noflash_extension() -> Result<(), String> {
    let dir = glib::user_data_dir()
        .join("gnome-shell/extensions")
        .join(NOFLASH_UUID);
    std::fs::create_dir_all(&dir).map_err(|e| format!("创建扩展目录失败: {e}"))?;
    std::fs::write(dir.join("metadata.json"), NOFLASH_METADATA)
        .map_err(|e| format!("写入扩展 metadata 失败: {e}"))?;
    std::fs::write(dir.join("extension.js"), NOFLASH_JS)
        .map_err(|e| format!("写入扩展代码失败: {e}"))?;

    let settings = gtk::gio::Settings::new("org.gnome.shell");
    let mut list: Vec<glib::GString> = settings
        .strv("enabled-extensions")
        .iter()
        .map(|s| glib::GString::from(s.as_str()))
        .collect();
    if !list.iter().any(|s| s.as_str() == NOFLASH_UUID) {
        list.push(glib::GString::from(NOFLASH_UUID));
        settings
            .set_strv("enabled-extensions", list)
            .map_err(|e| format!("启用无闪烁扩展失败: {e}"))?;
        gtk::gio::Settings::sync();
    }
    Ok(())
}

fn remove_noflash_extension() {
    let dir = glib::user_data_dir()
        .join("gnome-shell/extensions")
        .join(NOFLASH_UUID);
    let _ = std::fs::remove_dir_all(dir);
    let settings = gtk::gio::Settings::new("org.gnome.shell");
    let mut list: Vec<glib::GString> = settings
        .strv("enabled-extensions")
        .iter()
        .map(|s| glib::GString::from(s.as_str()))
        .collect();
    let before = list.len();
    list.retain(|s| s.as_str() != NOFLASH_UUID);
    if list.len() != before {
        let _ = settings.set_strv("enabled-extensions", list);
        gtk::gio::Settings::sync();
    }
}

const ICON_SVG: &str = r##"<svg xmlns="http://www.w3.org/2000/svg" width="64" height="64" viewBox="0 0 64 64">
  <rect width="64" height="64" rx="14" fill="#07C160"/>
  <path d="M18 24V18h8M38 18h8v6M46 40v6h-8M26 46h-8v-6" stroke="#fff" stroke-width="4" stroke-linecap="round" fill="none"/>
  <circle cx="32" cy="32" r="7" fill="#fff"/>
</svg>
"##;

fn set_hotkey(keys: &str) -> Result<(), String> {
    let mut cfg = config::Config::load();
    cfg.hotkey = keys.to_string();
    cfg.save()?;
    let cmd = install_path();
    let cmd = if cmd.exists() {
        cmd
    } else {
        std::env::current_exe().map_err(|e| format!("定位可执行文件失败: {e}"))?
    };
    hotkey::apply(&cfg.hotkey, &cmd.to_string_lossy())?;
    println!(
        "快捷键已设为 {}（{}）",
        keys,
        hotkey::normalize(keys)?
    );
    Ok(())
}

fn grant_screenshot_permission() {
    let Ok(conn) = gtk::gio::bus_get_sync(gtk::gio::BusType::Session, None::<&gtk::gio::Cancellable>)
    else {
        return;
    };
    for app in ["wxshot", "io.github.wxshot.WxShot"] {
        let args = (
            "screenshot",
            true,
            "screenshot",
            app,
            vec!["yes".to_string()],
        );
        let _ = conn.call_sync(
            Some("org.freedesktop.impl.portal.PermissionStore"),
            "/org/freedesktop/impl/portal/PermissionStore",
            "org.freedesktop.impl.portal.PermissionStore",
            "SetPermission",
            Some(&args.to_variant()),
            None,
            gtk::gio::DBusCallFlags::NONE,
            -1,
            None::<&gtk::gio::Cancellable>,
        );
    }
}

fn install() -> Result<(), String> {
    let exe = std::env::current_exe().map_err(|e| format!("定位可执行文件失败: {e}"))?;
    let dest = install_path();
    if let Some(parent) = dest.parent() {
        std::fs::create_dir_all(parent).map_err(|e| format!("创建目录失败: {e}"))?;
    }
    if exe != dest {
        std::fs::copy(&exe, &dest).map_err(|e| format!("复制可执行文件失败: {e}"))?;
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&dest, std::fs::Permissions::from_mode(0o755))
            .map_err(|e| format!("设置权限失败: {e}"))?;
    }

    let icon = icon_path();
    if let Some(parent) = icon.parent() {
        std::fs::create_dir_all(parent).map_err(|e| format!("创建图标目录失败: {e}"))?;
    }
    std::fs::write(&icon, ICON_SVG).map_err(|e| format!("写入图标失败: {e}"))?;

    let desktop = desktop_path();
    if let Some(parent) = desktop.parent() {
        std::fs::create_dir_all(parent).map_err(|e| format!("创建菜单目录失败: {e}"))?;
    }
    let entry = format!(
        "[Desktop Entry]\nType=Application\nName=wxshot\nName[zh_CN]=wxshot 截图\n\
         Comment=微信风格截图工具\nExec={}\nIcon={}\nTerminal=false\n\
         Categories=Graphics;\nStartupNotify=false\n",
        dest.display(),
        icon.display()
    );
    std::fs::write(&desktop, entry).map_err(|e| format!("写入菜单项失败: {e}"))?;

    let cfg = config::Config::load();
    cfg.save()?;
    hotkey::apply(&cfg.hotkey, &dest.to_string_lossy())?;
    grant_screenshot_permission();
    install_noflash_extension()?;
    for removed in remove_autostart_entries() {
        println!("已移除开机自启项: {removed}（wxshot 由快捷键拉起，无需自启）");
    }

    println!("已安装: {}", dest.display());
    println!("无闪烁扩展已安装（注销重登后生效）");
    println!("快捷键: {} → {}", cfg.hotkey, hotkey::normalize(&cfg.hotkey)?);
    println!("保存目录: {}", cfg.save_dir.display());
    Ok(())
}

fn autostart_dir() -> PathBuf {
    glib::user_config_dir().join("autostart")
}

/// 删除指向 wxshot 的开机自启项。
///
/// 通过“启动应用程序”添加 wxshot 时，GNOME 通常会在
/// `$XDG_CONFIG_HOME/autostart/` 放一个软链或副本；登录时它会执行裸的
/// `wxshot`，从而立刻触发一次截图。wxshot 是单次截图工具，热键由
/// gsd-media-keys 直接执行，完全不需要常驻，因此这里直接清掉。
fn remove_autostart_entries() -> Vec<String> {
    let mut removed = Vec::new();
    let Ok(entries) = std::fs::read_dir(autostart_dir()) else {
        return removed;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) != Some("desktop") {
            continue;
        }
        let Ok(text) = std::fs::read_to_string(&path) else {
            continue;
        };
        let targets_wxshot = text.lines().any(|line| {
            let line = line.trim_start();
            line.starts_with("Exec=") && line.contains("wxshot")
        });
        // 只删本程序的自启项，不碰用户其他启动程序
        if targets_wxshot && std::fs::remove_file(&path).is_ok() {
            if let Some(name) = path.file_name().and_then(|n| n.to_str()) {
                removed.push(name.to_string());
            }
        }
    }
    removed
}

fn uninstall() -> Result<(), String> {
    hotkey::remove_binding();
    remove_noflash_extension();
    remove_autostart_entries();
    for path in [desktop_path(), icon_path(), install_path()] {
        if path.exists() {
            let _ = std::fs::remove_file(&path);
        }
    }
    println!("已卸载 wxshot");
    Ok(())
}

fn capture_test() -> Result<(), String> {
    let image = capture::grab_screen()?;
    let path = PathBuf::from("/tmp/wxshot-capture.png");
    let mut file = std::fs::File::create(&path).map_err(|e| format!("创建文件失败: {e}"))?;
    image
        .write_to_png(&mut file)
        .map_err(|e| format!("写入 PNG 失败: {e}"))?;
    println!(
        "截图已保存: {} ({}x{})",
        path.display(),
        image.width(),
        image.height()
    );
    Ok(())
}

fn demo_render() -> Result<(), String> {
    let input = "/tmp/wxshot-capture.png";
    let mut file = std::fs::File::open(input).map_err(|e| format!("打开图片失败: {e}"))?;
    let image = cairo::ImageSurface::create_from_png(&mut file)
        .map_err(|e| format!("解码图片失败: {e}"))?;
    let pixelated = render::pixelate(&image);
    let w = image.width() as f64;
    let h = image.height() as f64;
    let sel = model::Rect {
        x: w * 0.18,
        y: h * 0.18,
        w: w * 0.55,
        h: h * 0.42,
    };
    let anns = vec![
        model::Annotation::Rect {
            a: (sel.x + 40.0, sel.y + 40.0),
            b: (sel.x + 260.0, sel.y + 140.0),
        },
        model::Annotation::Arrow {
            a: (sel.x + 300.0, sel.y + 300.0),
            b: (sel.x + 520.0, sel.y + 210.0),
        },
        model::Annotation::Text {
            pos: (sel.x + 60.0, sel.y + 190.0),
            text: "微信风格截图".to_string(),
            style: model::TextStyle::default(),
        },
        model::Annotation::Number {
            pos: (sel.x + 300.0, sel.y + 360.0),
            n: 1,
        },
    ];
    let out = cairo::ImageSurface::create(cairo::Format::ARgb32, w as i32, h as i32)
        .map_err(|e| format!("创建画布失败: {e}"))?;
    let cr = cairo::Context::new(&out).map_err(|e| format!("创建上下文失败: {e}"))?;
    let scene = render::Scene {
        image: &image,
        pixelated: &pixelated,
        sel: Some(sel),
        annotations: &anns,
        draft: None,
        tool: model::Tool::Text,
        cursor: Some((w * 0.5, h * 0.5)),
        selecting: false,
        hover_btn: Some(3),
    };
    if let Some(tb) = render::draw_scene(&cr, &scene, w, h, 1.0) {
        let bar = render::build_style_bar(&tb, w, h);
        render::draw_style_bar(
            &cr,
            &bar,
            model::TextStyle::default(),
            Some(render::StyleHit::Color(0)),
        );
    }
    out.flush();
    let path = std::path::Path::new("/tmp/wxshot-demo.png");
    let mut f = std::fs::File::create(path).map_err(|e| format!("创建文件失败: {e}"))?;
    out.write_to_png(&mut f)
        .map_err(|e| format!("写入 PNG 失败: {e}"))?;
    println!("demo: {}", path.display());
    Ok(())
}

fn selftest(input: Option<&str>) -> Result<(), String> {
    let image = match input {
        Some(path) => {
            let mut file =
                std::fs::File::open(path).map_err(|e| format!("打开图片失败: {e}"))?;
            cairo::ImageSurface::create_from_png(&mut file)
                .map_err(|e| format!("解码图片失败: {e}"))?
        }
        None => capture::grab_screen()?,
    };
    let w = image.width() as f64;
    let h = image.height() as f64;
    let sel = model::Rect {
        x: w * 0.12,
        y: h * 0.12,
        w: w * 0.62,
        h: h * 0.55,
    };
    let anns = vec![
        model::Annotation::Rect {
            a: (sel.x + 40.0, sel.y + 50.0),
            b: (sel.x + 300.0, sel.y + 170.0),
        },
        model::Annotation::Ellipse {
            a: (sel.x + 340.0, sel.y + 60.0),
            b: (sel.x + 560.0, sel.y + 200.0),
        },
        model::Annotation::Arrow {
            a: (sel.x + 380.0, sel.y + 320.0),
            b: (sel.x + 620.0, sel.y + 230.0),
        },
        model::Annotation::Pen {
            points: vec![
                (sel.x + 60.0, sel.y + 300.0),
                (sel.x + 110.0, sel.y + 250.0),
                (sel.x + 160.0, sel.y + 330.0),
                (sel.x + 220.0, sel.y + 270.0),
            ],
        },
        model::Annotation::Text {
            pos: (sel.x + 60.0, sel.y + 210.0),
            text: "微信风格截图 wxshot".to_string(),
            style: model::TextStyle::default(),
        },
        model::Annotation::Mosaic {
            a: (sel.x + 600.0, sel.y + 340.0),
            b: (sel.x + 820.0, sel.y + 470.0),
        },
        model::Annotation::Number {
            pos: (sel.x + 300.0, sel.y + 380.0),
            n: 1,
        },
        model::Annotation::Number {
            pos: (sel.x + 360.0, sel.y + 380.0),
            n: 2,
        },
    ];
    let pixelated = render::pixelate(&image);
    let out = render::export(&image, &pixelated, sel, &anns)?;
    let path = std::path::Path::new("/tmp/wxshot-selftest.png");
    let mut file = std::fs::File::create(path).map_err(|e| format!("创建文件失败: {e}"))?;
    out.write_to_png(&mut file)
        .map_err(|e| format!("写入 PNG 失败: {e}"))?;
    println!(
        "自测导出: {} (选区 {}x{})",
        path.display(),
        out.width(),
        out.height()
    );
    Ok(())
}
