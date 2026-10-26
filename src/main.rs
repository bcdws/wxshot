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
use std::io::Write;
use std::os::unix::io::AsRawFd;
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
        "--daemon" => {
            if let Err(e) = run_daemon() {
                eprintln!("daemon 启动失败: {e}");
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
            // 需要开机常驻请用 `wxshot --daemon`（它不会截图，只注册快捷键）。
            if launched_from_autostart() {
                eprintln!(
                    "wxshot: 检测到由开机自启直接启动，已跳过截图。\n\
                     要开机常驻、按快捷键截图，请使用 `wxshot --daemon`；\n\
                     要清理自启项，运行 `wxshot --install`。"
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
    // KDE / GNOME 都可能把 XDG autostart 转换成 systemd 单元
    // （systemd-xdg-autostart-generator 生成 app-<id>@autostart.service），
    // 此时进程的 cgroup 里能看到该单元名。
    if let Ok(cgroup) = std::fs::read_to_string("/proc/self/cgroup") {
        if cgroup.contains("@autostart.service") {
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
         wxshot --daemon         常驻后台（开机自启用，不截图）\n  \
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
    // 再次触发只把已有窗口拉回前台，绝不重新截图/新建窗口。
    let active = Rc::new(Cell::new(false));
    app.connect_activate({
        let active = active.clone();
        move |app| start_capture(app, &active, false)
    });
    let _ = app.run();
}

/// 常驻后台：开机自启后不截图，只注册全局快捷键并等待。
///
/// KDE Wayland 下用标准的 GlobalShortcuts portal 注册快捷键（由桌面环境
/// 分配/保存，不直接碰 KGlobalAccel，避免破坏合成器）；实际触发时回到
/// GTK 主线程执行截图。`wxshot` 无参数启动会通过 GApplication 单实例
/// 转发到这里，所以菜单、快捷键、重复启动行为一致。
fn run_daemon() -> Result<(), String> {
    let mut lock = match acquire_daemon_lock() {
        Some(file) => file,
        // 已有实例在跑，静默退出（比如重复自启）
        None => return Ok(()),
    };
    let _ = writeln!(lock, "{}", std::process::id());

    let app = gtk::Application::builder().application_id(APP_ID).build();
    let active = Rc::new(Cell::new(false));
    let first = Rc::new(Cell::new(true));
    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel::<()>();

    {
        let app = app.clone();
        let active = active.clone();
        let _ = glib::spawn_future_local(async move {
            while rx.recv().await.is_some() {
                start_capture(&app, &active, true);
            }
        });
    }

    if is_kde() {
        register_global_shortcut(tx.clone(), config::Config::load().hotkey);
    }

    app.connect_activate({
        let first = first.clone();
        move |app| {
            // 进程刚启动的那次 activate 不截图，否则开机自启又会弹遮罩
            if first.replace(false) {
                return;
            }
            start_capture(app, &active, true);
        }
    });

    // 没有窗口时也保持运行
    let _hold = app.hold();
    // 用固定 argv 跑 GApplication，避免它把 --daemon 当成未知选项
    let _ = app.run_with_args(&["wxshot"]);
    drop(lock);
    Ok(())
}

fn start_capture(app: &gtk::Application, active: &Rc<Cell<bool>>, keep_alive: bool) {
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
            if !keep_alive {
                app.quit();
            }
            return;
        }
    };
    let cfg = config::Config::load();
    let window = ui::run(app, image, cfg);
    let active = active.clone();
    window.connect_destroy(move |_| active.set(false));
}

fn daemon_lock_path() -> PathBuf {
    glib::user_runtime_dir().join("wxshot-daemon.lock")
}

/// 用 flock 保证只有一个常驻实例；进程退出锁自动释放。
fn acquire_daemon_lock() -> Option<std::fs::File> {
    let path = daemon_lock_path();
    let file = std::fs::OpenOptions::new()
        .create(true)
        .read(true)
        .write(true)
        .open(path)
        .ok()?;
    let rc = unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) };
    (rc == 0).then_some(file)
}

fn stop_daemon() {
    let path = daemon_lock_path();
    let Ok(pid) = std::fs::read_to_string(&path)
        .unwrap_or_default()
        .trim()
        .parse::<i32>()
    else {
        return;
    };
    // 避免 PID 复用误杀
    let cmdline = std::fs::read_to_string(format!("/proc/{pid}/cmdline")).unwrap_or_default();
    if !cmdline.contains("wxshot") || !cmdline.contains("--daemon") {
        return;
    }
    unsafe {
        libc::kill(pid, libc::SIGTERM);
    }
    for _ in 0..20 {
        if !std::path::Path::new(&format!("/proc/{pid}")).exists() {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(50));
    }
}

/// 是否已有常驻实例在跑（尝试取锁后立刻释放）。
fn daemon_running() -> bool {
    match acquire_daemon_lock() {
        Some(lock) => {
            drop(lock);
            false
        }
        None => true,
    }
}

/// 拉起常驻实例。
///
/// 必须通过 systemd 启动，并且单元名以 `app-<appid>@...` 形式存在：
/// xdg-desktop-portal 的 host app 是从 cgroup 里的单元名推导 app id 的，
/// 直接 fork 出来的进程没有 app id，GlobalShortcuts 会报 "An app id is required"。
fn spawn_daemon(exe: &std::path::Path) {
    use std::process::Stdio;
    if daemon_running() {
        return;
    }
    let dir = glib::user_cache_dir().join("wxshot");
    let _ = std::fs::create_dir_all(&dir);
    let log = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(dir.join("daemon.log"))
        .ok();

    let unit = "app-wxshot@local.service";
    let mut cmd = std::process::Command::new("systemd-run");
    cmd.args([
        "--user",
        "--collect",
        "--quiet",
        &format!("--unit={unit}"),
        "--description=wxshot 后台",
    ]);
    cmd.arg(exe).arg("--daemon");
    cmd.stdin(Stdio::null()).stdout(Stdio::null());
    cmd.stderr(match log.as_ref() {
        Some(file) => Stdio::from(file.try_clone().unwrap_or_else(|_| std::fs::File::open("/dev/null").unwrap())),
        None => Stdio::null(),
    });
    if cmd.status().map(|s| s.success()).unwrap_or(false) {
        return;
    }

    // 没有 systemd 时退回直接启动（此时 portal 快捷键可能不可用）
    let mut direct = std::process::Command::new(exe);
    direct.arg("--daemon").stdin(Stdio::null()).stdout(Stdio::null());
    direct.stderr(match log {
        Some(file) => Stdio::from(file),
        None => Stdio::null(),
    });
    let _ = direct.spawn();
}

fn is_kde() -> bool {
    std::env::var("XDG_CURRENT_DESKTOP")
        .map(|v| v.to_ascii_uppercase().contains("KDE"))
        .unwrap_or(false)
        || std::env::var_os("KDE_FULL_SESSION").is_some()
}

/// 把 "Ctrl+Alt+A" 转成 portal 的 trigger 写法 "CTRL+ALT+A"。
fn portal_trigger(hotkey: &str) -> String {
    let mut parts: Vec<&str> = hotkey
        .split('+')
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .collect();
    let Some(key) = parts.pop() else {
        return "F1".to_string();
    };
    let mut out: Vec<String> = Vec::new();
    for part in parts {
        let m = match part.to_ascii_lowercase().as_str() {
            "ctrl" | "control" => "CTRL",
            "alt" | "option" | "opt" => "ALT",
            "shift" => "SHIFT",
            "super" | "win" | "meta" | "cmd" => "LOGO",
            _ => continue,
        };
        out.push(m.to_string());
    }
    let key = match key.to_ascii_lowercase().as_str() {
        "space" => "space".to_string(),
        "esc" | "escape" => "Escape".to_string(),
        "enter" | "return" => "Return".to_string(),
        other if other.starts_with('f') && other[1..].parse::<u8>().is_ok() => {
            other.to_ascii_uppercase()
        }
        other => other.to_string(),
    };
    out.push(key);
    out.join("+")
}

fn register_global_shortcut(tx: tokio::sync::mpsc::UnboundedSender<()>, hotkey: String) {
    let trigger = portal_trigger(&hotkey);
    let spawned = std::thread::Builder::new()
        .name("wxshot-portal".into())
        .spawn(move || {
            let rt = match tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
            {
                Ok(rt) => rt,
                Err(e) => {
                    eprintln!("wxshot: 初始化快捷键运行时失败: {e}");
                    return;
                }
            };
            rt.block_on(portal_loop(&trigger, tx));
        });
    if let Err(e) = spawned {
        eprintln!("wxshot: 启动快捷键线程失败: {e}");
    }
}

async fn portal_loop(trigger: &str, tx: tokio::sync::mpsc::UnboundedSender<()>) {
    // portal 可能在登录早期还没准备好，或者在合成器重启后断开：
    // 失败就退避重试，保证登录后过一会儿快捷键仍然可用。
    let mut delay_secs = 1u64;
    loop {
        match portal_once(trigger, tx.clone()).await {
            Ok(()) => {
                delay_secs = 1;
                eprintln!("wxshot: 快捷键会话结束，准备重新注册");
            }
            Err(e) => {
                eprintln!(
                    "wxshot: 注册全局快捷键失败（{e}），{} 秒后重试",
                    delay_secs
                );
            }
        }
        tokio::time::sleep(std::time::Duration::from_secs(delay_secs)).await;
        delay_secs = (delay_secs * 2).min(60);
    }
}

async fn portal_once(
    trigger: &str,
    tx: tokio::sync::mpsc::UnboundedSender<()>,
) -> Result<(), ashpd::Error> {
    use ashpd::desktop::global_shortcuts::{GlobalShortcuts, NewShortcut};
    use futures_util::StreamExt;

    let proxy = GlobalShortcuts::new().await?;
    let session = proxy.create_session(Default::default()).await?;
    let shortcut =
        NewShortcut::new("capture", "wxshot 截图").preferred_trigger(Some(trigger));
    let request = proxy
        .bind_shortcuts(&session, &[shortcut], None, Default::default())
        .await?;
    let bound = request.response()?;
    for s in bound.shortcuts() {
        let shown = s.trigger_description();
        eprintln!(
            "wxshot: 全局快捷键已注册: {} → {}",
            s.description(),
            if shown.is_empty() { "（未分配，请在系统设置里设置）" } else { shown }
        );
    }

    let mut activated = proxy.receive_activated().await?;
    while let Some(event) = activated.next().await {
        if event.shortcut_id() == "capture" {
            let _ = tx.send(());
        }
    }
    Ok(())
}

fn daemon_autostart_path() -> PathBuf {
    // 文件名必须和菜单项 wxshot.desktop 一致：
    // systemd-xdg-autostart-generator 会生成 app-wxshot@autostart.service，
    // xdg-desktop-portal 从单元名解析出 app id "wxshot"，再查 wxshot.desktop
    // 确认身份；名字对不上就拿不到 app id，GlobalShortcuts 会被拒绝。
    glib::user_config_dir().join("autostart/wxshot.desktop")
}

fn write_daemon_autostart(exe: &std::path::Path) -> Result<(), String> {
    let path = daemon_autostart_path();
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| format!("创建自启目录失败: {e}"))?;
    }
    let entry = [
        "[Desktop Entry]".to_string(),
        "Type=Application".to_string(),
        "Name=wxshot 后台".to_string(),
        "Comment=常驻后台等待截图快捷键（不会自动截图）".to_string(),
        format!("Exec=\"{}\" --daemon", exe.display()),
        format!("Icon={}", icon_path().display()),
        "Terminal=false".to_string(),
        "NoDisplay=true".to_string(),
        "X-GNOME-Autostart-enabled=true".to_string(),
        String::new(),
    ]
    .join("\n");
    std::fs::write(&path, entry).map_err(|e| format!("写入自启项失败: {e}"))
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
    let has_schema = gtk::gio::SettingsSchemaSource::default()
        .and_then(|s| s.lookup("org.gnome.shell", true))
        .is_some();
    if !has_schema {
        return Ok(());
    }
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
    let has_schema = gtk::gio::SettingsSchemaSource::default()
        .and_then(|s| s.lookup("org.gnome.shell", true))
        .is_some();
    if !has_schema {
        return;
    }
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
    if hotkey::available() {
        hotkey::apply(&cfg.hotkey, &cmd.to_string_lossy())?;
    }
    println!(
        "快捷键已设为 {}（{}）",
        keys,
        hotkey::normalize(keys)?
    );
    if is_kde() {
        // 常驻进程启动时通过 portal 绑定快捷键，改键后需要重启它
        stop_daemon();
        spawn_daemon(&cmd);
        println!("后台服务已按新快捷键重启");
    }
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
    if hotkey::available() {
        hotkey::apply(&cfg.hotkey, &dest.to_string_lossy())?;
        grant_screenshot_permission();
        install_noflash_extension()?;
    }
    for removed in remove_autostart_entries() {
        println!("已移除会直接截图的旧自启项: {removed}");
    }

    if is_kde() {
        for legacy in ["wxshot-daemon.desktop", "io.github.wxshot.WxShot.desktop"] {
            let _ = std::fs::remove_file(glib::user_config_dir().join("autostart").join(legacy));
        }
        write_daemon_autostart(&dest)?;
        // 拉起常驻实例；若已在运行，新实例会因 flock 自动退出
        spawn_daemon(&dest);
        println!("已设置开机自启：登录后常驻后台，不会自动截图");
    }

    println!("已安装: {}", dest.display());
    if hotkey::available() {
        println!("无闪烁扩展已安装（注销重登后生效）");
        println!("快捷键: {} → {}", cfg.hotkey, hotkey::normalize(&cfg.hotkey)?);
    } else {
        println!("快捷键: {}（由桌面环境的全局快捷键服务管理）", cfg.hotkey);
    }
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
            line.starts_with("Exec=") && line.contains("wxshot") && !line.contains("--daemon")
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
    stop_daemon();
    hotkey::remove_binding();
    remove_noflash_extension();
    remove_autostart_entries();
    let _ = std::fs::remove_file(daemon_autostart_path());
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
