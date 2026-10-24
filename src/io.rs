use gtk4::cairo::ImageSurface;
use gtk4::glib;
use std::io::Write;
use std::path::Path;
use std::process::{Command, Stdio};

pub fn png_bytes(surface: &ImageSurface) -> Result<Vec<u8>, String> {
    let mut buf: Vec<u8> = Vec::new();
    surface
        .write_to_png(&mut buf)
        .map_err(|e| format!("编码 PNG 失败: {e}"))?;
    Ok(buf)
}

pub fn copy_to_clipboard(surface: &ImageSurface) -> Result<(), String> {
    let bytes = png_bytes(surface)?;
    let mut child = Command::new("wl-copy")
        .arg("--type")
        .arg("image/png")
        .stdin(Stdio::piped())
        .spawn()
        .map_err(|e| format!("启动 wl-copy 失败: {e}"))?;
    {
        let mut stdin = child
            .stdin
            .take()
            .ok_or_else(|| "无法写入剪贴板".to_string())?;
        stdin
            .write_all(&bytes)
            .map_err(|e| format!("写入剪贴板失败: {e}"))?;
    }
    let status = child
        .wait()
        .map_err(|e| format!("等待 wl-copy 失败: {e}"))?;
    if !status.success() {
        return Err("wl-copy 返回失败".to_string());
    }
    Ok(())
}

pub fn save_png_to(surface: &ImageSurface, path: &Path) -> Result<(), String> {
    let mut file = std::fs::File::create(path).map_err(|e| format!("创建文件失败: {e}"))?;
    surface
        .write_to_png(&mut file)
        .map_err(|e| format!("写入 PNG 失败: {e}"))?;
    Ok(())
}

pub fn default_screenshot_name() -> String {
    let stamp = glib::DateTime::now_local()
        .ok()
        .and_then(|t| t.format("%Y-%m-%d_%H-%M-%S").ok())
        .map(|s| s.to_string())
        .unwrap_or_else(|| "screenshot".to_string());
    format!("Screenshot_{stamp}.png")
}
