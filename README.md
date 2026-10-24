# wxshot

微信风格的 Linux 截图工具：**Rust + GTK4 + Cairo**，面向 GNOME Wayland。

按 `F1`（可改）→ 整屏冻结并变暗 → 拖动框选 → 悬浮工具条标注 → `Ctrl+C` 复制即关闭。

## 功能

- 原生全屏遮罩、绿色选区、尺寸提示、十字准星、放大镜
- 标注：矩形、椭圆、箭头、画笔、文字（支持 fcitx5 中文输入、可调颜色/字号）、马赛克、序号
- 撤销（`Ctrl+Z`）、另存为（`Ctrl+S` / 软盘按钮）、复制到剪贴板（`Ctrl+C` / `Enter`）
- 选区拖动 / 8 个手柄缩放
- 快捷键可配置（默认 `F1`）
- 截图走 xdg-desktop-portal 非交互接口，不弹 GNOME 自带的选择框
- 剪贴板通过 `wl-copy` 写入，GNOME Wayland 下可靠

## 构建

```bash
sudo apt install cargo rustc build-essential pkg-config libgtk-4-dev
cd wxshot
cargo build --release
```

产物：`target/release/wxshot`（release 配置了 `lto = "thin"` 和 `strip = true`）。

需要 GTK 4.10+（另存为对话框使用 `GtkFileDialog`）。

## 发布（维护者）

```bash
# 先把 Cargo.toml 里的 version 改成新版本，提交后：
git tag -a v0.1.1 -m "wxshot v0.1.1"
git push origin v0.1.1
```

推送 `v*` tag 后，GitHub Actions（[.github/workflows/release.yml](.github/workflows/release.yml)）会自动：

1. 在 **Ubuntu 24.04**（glibc 2.39 + GTK 4.14）上构建，兼容面比开发机更宽
2. 校验 `Cargo.toml` 版本与 tag 一致，不一致直接失败
3. 把 `wxshot-linux-x86_64` 和 `SHA256SUMS` 传到对应 Release（没有就新建）

也可以在 Actions 页面手动触发 `release` 工作流并填写已有 tag，重新生成/覆盖附件；
留空则只构建，产物在 workflow 的 Artifacts 里。

## 安装 / 卸载

```bash
./target/release/wxshot --install     # 安装到 ~/.local/bin，写 desktop/图标，绑定配置里的热键
./target/release/wxshot --uninstall   # 解绑并清理
```

安装后：

- 可执行文件：`~/.local/bin/wxshot`
- 菜单项：`~/.local/share/applications/wxshot.desktop`
- 配置：`~/.config/wxshot/config.ini`

> **不要把它加进“启动应用程序”（开机自启）。**
> wxshot 是单次截图工具，按 `F1` 时由 GNOME 的全局快捷键直接拉起，不需要常驻进程；
> 自启只会在每次登录后立刻弹出一张全屏遮罩（还要手动 `Esc`）。
> `--install` / `--uninstall` 会顺手清掉 `~/.config/autostart/` 里指向 wxshot 的自启项；
> 即使自启项被重新加回来，程序检测到由 autostart 启动时也会直接跳过截图并退出。

```ini
[general]
hotkey=F1
save_dir=~/图片

[text]
color=#FF3B30
size=22
```

`save_dir` 会随每次“另存为”自动更新；文字颜色和字号改一次就会记住。

## 修改快捷键

```bash
wxshot --set-hotkey F1
wxshot --set-hotkey Ctrl+Alt+A
wxshot --set-hotkey Super+Shift+S
```

支持 `Ctrl/Alt/Shift/Super` 组合，程序会转成 GNOME 的 accelerator 格式并写入自定义快捷键。

> 注意：全局热键会覆盖应用内的同名按键。把 `F1` 绑成截图后，浏览器/IDE 里的 F1 帮助会被截图取代。

## 使用

| 按键 / 操作 | 作用 |
|---|---|
| 拖动 | 框选 |
| 松开 | 出现工具条 |
| 选区内拖动（指针工具） | 移动选区 |
| 拖角/边手柄 | 缩放选区 |
| 工具 + 拖动 | 画矩形/椭圆/箭头/画笔/马赛克 |
| 文字工具 + 点击 | 弹出输入框，回车提交；上方样式条可调颜色和字号 |
| 序号工具 + 点击 | 放置递增序号 |
| `Ctrl+C` / `Enter` / 绿色「完成」按钮 | 复制并关闭 |
| `Ctrl+S` / 软盘按钮 | 另存为并关闭；取消对话框则直接退出 |
| `Ctrl+Z` | 撤销上一个标注 |
| `Esc` | 退出（文字输入中先取消输入） |
| 右键 | 有选区时取消选区；无选区时退出 |
| `Alt+A`（旧） | 未绑定，可用 `--set-hotkey` 改回 |

> 截图遮罩已经显示时再次按热键，不会重复截图或叠加窗口，只会把当前遮罩拉回前台。

## 架构

```
快捷键 (GNOME custom keybinding)
   │
   ▼
capture.rs   xdg-desktop-portal Screenshot (interactive=false)
   │          → 整屏 PNG → cairo::ImageSurface，删除 portal 临时文件
   ▼
ui.rs        GTK4 全屏无边框窗口 + DrawingArea
   │          状态机：Idle → DraggingNew → Selected → Move/Resize/Draw
   │          文字输入用 GtkEntry（Overlay + Fixed）承载 IME
   ▼
render.rs    Cairo 纯绘制：
   │          draw_scene()    遮罩、选区、工具条、标注预览、放大镜
   │          export()        选区裁切 + 标注合成
   ▼
io.rs        wl-copy / PNG 保存
```

- 所有几何数据统一用**图像像素**，绘制时按 `widget_width / image_width` 缩放，HiDPI 自动适配
- 预览和导出共用同一套标注绘制函数，所见即所得
- 马赛克预先对整屏做 8×8 像素化，预览/导出时按矩形裁切，避免每帧重算

## 开发辅助

```bash
wxshot --capture-test          # 只截屏，保存 /tmp/wxshot-capture.png
wxshot --selftest 图片.png     # 无界面：选区 + 全类型标注 + 导出 /tmp/wxshot-selftest.png
wxshot --demo                  # 渲染带工具条的完整界面到 /tmp/wxshot-demo.png
```

## 已知限制

- 仅支持单显示器（portal 非交互截图返回单屏）
- 没有贴图置顶（GNOME Wayland 不允许普通窗口置顶）
- 不支持录屏
- 首次截图可能有系统快门声/轻微延迟

## 排错

- **F1 没反应**：GNOME 50 的 `gsd-media-keys` 只接受裸的 `F1`，写成 `<F1>` 会 grab 失败
  （`journalctl --user | grep 'Failed to grab accelerator'` 可确认）。
  用 `wxshot --set-hotkey F1` 重设即可。
- **提示 "portal 未返回截图"**：热键启动的窗口没有焦点，portal 首次截图会尝试弹授权框，
  但 "Only the focused app is allowed to show a system access dialog" 会拒绝。
  `wxshot --install` 会向 `org.freedesktop.impl.portal.PermissionStore` 写入
  `screenshot` 表对 `wxshot` / `io.github.wxshot.WxShot` 的 `yes` 权限，之后不再弹框。
- **按 F1 后看起来没变化**：全屏窗口显示的是冻结的桌面截图 + 45% 黑色遮罩，
  鼠标移到顶部提示条/拖动框选后才会出现明显变化。

## 无闪烁截图

GNOME 50 的 screenshot portal 在调用 Shell 截图接口时把 `flash` 参数写死为 `TRUE`，
没有设置可以关。项目里带了一个本地 GNOME Shell 扩展
`extensions/wxshot-noflash@local/`，把 `Screenshot*` 的 flash 参数改成 `false`，
并让 `FlashArea` 空操作；`wxshot --install` 会自动安装并启用它。

> 扩展需要**注销重登一次**才会被 GNOME Shell 加载（Wayland 不能热加载扩展）。
> 生效后所有走 Shell/portal 的截图都不再闪；`wxshot --uninstall` 会移除它。

## 许可证

[MIT](LICENSE) © 2026 bcdws

随意使用、修改、分发，保留版权声明即可；不提供任何担保。
