use gtk4::cairo::ImageSurface;
use gtk4::glib;
use std::fs::File;

/// Grab the full screen through the XDG desktop portal without showing
/// GNOME's own selection UI, so we can draw our own overlay.
pub fn grab_screen() -> Result<ImageSurface, String> {
    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(|e| format!("初始化运行时失败: {e}"))?;

    let uri = rt.block_on(async {
        let response = ashpd::desktop::screenshot::Screenshot::request()
            .interactive(false)
            .modal(false)
            .send()
            .await
            .map_err(|e| format!("调用截图 portal 失败: {e}"))?
            .response()
            .map_err(|e| format!("portal 未返回截图: {e}"))?;
        Ok::<String, String>(response.uri().as_str().to_string())
    })?;

    let (path, _host) =
        glib::filename_from_uri(&uri).map_err(|e| format!("解析截图路径失败: {e}"))?;
    let mut file = File::open(&path).map_err(|e| format!("打开截图失败: {e}"))?;
    let surface =
        ImageSurface::create_from_png(&mut file).map_err(|e| format!("解码截图失败: {e}"))?;
    drop(file);
    let _ = std::fs::remove_file(&path);
    Ok(surface)
}
