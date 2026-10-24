use std::cell::RefCell;
use std::rc::Rc;

use gtk4 as gtk;
use gtk::cairo;
use gtk::gdk;
use gtk::gio;
use gtk::glib;
use gtk::prelude::*;

use crate::config::{self, Config};
use crate::io;
use crate::model::{Annotation, Handle, Rect, TextStyle, Tool};
use crate::render::{self, StyleBarLayout, StyleHit, ToolbarLayout};

#[derive(Clone, Debug)]
enum Drag {
    New {
        anchor: (f64, f64),
    },
    Move {
        grab: (f64, f64),
        orig: Rect,
    },
    Resize {
        handle: Handle,
        orig: Rect,
    },
    Draw {
        start: (f64, f64),
    },
}

struct State {
    image: Rc<cairo::ImageSurface>,
    pixelated: Rc<cairo::ImageSurface>,
    image_w: f64,
    image_h: f64,
    scale: f64,
    sel: Option<Rect>,
    tool: Tool,
    annotations: Vec<Annotation>,
    draft: Option<Annotation>,
    drag: Option<Drag>,
    cursor: Option<(f64, f64)>,
    hover_btn: Option<usize>,
    toolbar: Option<ToolbarLayout>,
    number_next: u32,
    text_anchor: Option<(f64, f64)>,
    text_editing: bool,
    text_style: TextStyle,
    stylebar: Option<StyleBarLayout>,
    hover_style: Option<StyleHit>,
    dialog_open: bool,
    cfg: Config,
}

struct Ctx {
    state: Rc<RefCell<State>>,
    area: gtk::DrawingArea,
    entry: gtk::Entry,
    window: gtk::ApplicationWindow,
    css: gtk::CssProvider,
}

pub fn run(
    app: &gtk::Application,
    image: cairo::ImageSurface,
    cfg: Config,
) -> gtk::ApplicationWindow {
    let image_w = image.width() as f64;
    let image_h = image.height() as f64;
    let pixelated = render::pixelate(&image);

    let state = Rc::new(RefCell::new(State {
        image: Rc::new(image),
        pixelated: Rc::new(pixelated),
        image_w,
        image_h,
        scale: 1.0,
        sel: None,
        tool: Tool::Pointer,
        annotations: Vec::new(),
        draft: None,
        drag: None,
        cursor: None,
        hover_btn: None,
        toolbar: None,
        number_next: 1,
        text_anchor: None,
        text_editing: false,
        text_style: cfg.text_style,
        stylebar: None,
        hover_style: None,
        dialog_open: false,
        cfg,
    }));

    let area = gtk::DrawingArea::new();
    area.set_hexpand(true);
    area.set_vexpand(true);
    area.set_focusable(true);
    area.set_cursor_from_name(Some("crosshair"));

    let entry = gtk::Entry::new();
    entry.add_css_class("wxshot-entry");
    entry.set_visible(false);
    entry.set_size_request(220, 34);
    entry.set_width_chars(16);
    entry.set_halign(gtk::Align::Start);
    entry.set_valign(gtk::Align::Start);

    let overlay = gtk::Overlay::new();
    overlay.set_child(Some(&area));
    overlay.add_overlay(&entry);

    let window = gtk::ApplicationWindow::builder()
        .application(app)
        .title("wxshot")
        .decorated(false)
        .build();
    area.set_size_request(image_w as i32, image_h as i32);
    window.set_default_size(image_w as i32, image_h as i32);
    window.set_child(Some(&overlay));

    let provider = gtk::CssProvider::new();
    provider.load_from_data(&entry_css(state.borrow().text_style));
    if let Some(display) = gdk::Display::default() {
        gtk::style_context_add_provider_for_display(
            &display,
            &provider,
            gtk::STYLE_PROVIDER_PRIORITY_APPLICATION,
        );
    }

    let ctx = Rc::new(Ctx {
        state,
        area: area.clone(),
        entry: entry.clone(),
        window: window.clone(),
        css: provider,
    });

    {
        let c = ctx.clone();
        area.set_draw_func(move |_, cr, w, h| c.draw(cr, w, h));
    }
    {
        let c = ctx.clone();
        let motion = gtk::EventControllerMotion::new();
        motion.connect_motion(move |_, x, y| c.on_motion(x, y));
        area.add_controller(motion);
    }
    {
        let click = gtk::GestureClick::new();
        click.set_button(gdk::BUTTON_PRIMARY);
        let c = ctx.clone();
        click.connect_pressed(move |_, _, x, y| c.on_press(x, y));
        let c = ctx.clone();
        click.connect_released(move |_, _, x, y| c.on_release(x, y));
        area.add_controller(click);
    }
    {
        let click = gtk::GestureClick::new();
        click.set_button(gdk::BUTTON_SECONDARY);
        let c = ctx.clone();
        click.connect_pressed(move |_, _, _, _| c.on_right_click());
        area.add_controller(click);
    }
    {
        let keys = gtk::EventControllerKey::new();
        keys.set_propagation_phase(gtk::PropagationPhase::Capture);
        let c = ctx.clone();
        keys.connect_key_pressed(move |_, keyval, _, mods| c.on_key(keyval, mods));
        window.add_controller(keys);
    }
    {
        let c = ctx.clone();
        entry.connect_activate(move |_| c.commit_text());
    }
    {
        let c = ctx.clone();
        let focus = gtk::EventControllerFocus::new();
        focus.connect_leave(move |_| c.commit_text_if_editing());
        entry.add_controller(focus);
    }
    {
        let c = ctx.clone();
        let keys = gtk::EventControllerKey::new();
        keys.set_propagation_phase(gtk::PropagationPhase::Capture);
        keys.connect_key_pressed(move |_, keyval, _, _| {
            if keyval == gdk::Key::Escape {
                c.cancel_text();
                glib::Propagation::Stop
            } else {
                glib::Propagation::Proceed
            }
        });
        entry.add_controller(keys);
    }
    {
        let area = area.clone();
        window.connect_map(move |_| {
            area.grab_focus();
        });
    }
    window.present();
    window.fullscreen();
    area.grab_focus();
    window
}

impl Ctx {
    fn draw(&self, cr: &cairo::Context, w: i32, h: i32) {
        cr.set_source_rgb(0.08, 0.02, 0.02);
        let _ = cr.paint();
        let mut st = self.state.borrow_mut();
        let scale = if st.image_w > 0.0 {
            w as f64 / st.image_w
        } else {
            1.0
        };
        st.scale = scale;
        let (toolbar, stylebar) = {
            let scene = render::Scene {
                image: st.image.as_ref(),
                pixelated: st.pixelated.as_ref(),
                sel: st.sel,
                annotations: &st.annotations,
                draft: st.draft.as_ref(),
                tool: st.tool,
                cursor: st.cursor,
                selecting: matches!(st.drag, Some(Drag::New { .. })),
                hover_btn: st.hover_btn,
            };
            let toolbar = render::draw_scene(cr, &scene, w as f64, h as f64, scale);
            let stylebar = if st.tool == Tool::Text {
                toolbar
                    .as_ref()
                    .map(|tb| render::build_style_bar(tb, w as f64, h as f64))
            } else {
                None
            };
            if let Some(bar) = &stylebar {
                render::draw_style_bar(cr, bar, st.text_style, st.hover_style);
            }
            (toolbar, stylebar)
        };
        st.toolbar = toolbar;
        st.stylebar = stylebar;
    }

    fn scale(&self) -> f64 {
        let st = self.state.borrow();
        if st.scale > 0.0 { st.scale } else { 1.0 }
    }

    fn to_img(&self, x: f64, y: f64) -> (f64, f64) {
        let s = self.scale();
        (x / s, y / s)
    }

    fn on_press(&self, x: f64, y: f64) {
        let (ix, iy) = self.to_img(x, y);
        let mut st = self.state.borrow_mut();
        if st.dialog_open {
            return;
        }

        if let Some(sb) = st.stylebar.clone() {
            if x >= sb.x && x <= sb.x + sb.w && y >= sb.y && y <= sb.y + sb.h {
                let hit = sb.hit(x, y);
                drop(st);
                if let Some(hit) = hit {
                    self.on_style(hit);
                }
                return;
            }
        }

        if let Some(tb) = st.toolbar.clone() {
            if x >= tb.x && x <= tb.x + tb.w && y >= tb.y && y <= tb.y + tb.h {
                let id = tb.hit(x, y);
                drop(st);
                if let Some(id) = id {
                    self.on_tool(id);
                }
                return;
            }
        }

        let scale = if st.scale > 0.0 { st.scale } else { 1.0 };
        if let Some(sel) = st.sel {
            if st.tool == Tool::Pointer {
                let tol = 10.0 / scale;
                for handle in Handle::ALL {
                    let p = handle.pos(&sel);
                    if (p.0 - ix).abs() <= tol && (p.1 - iy).abs() <= tol {
                        st.drag = Some(Drag::Resize {
                            handle,
                            orig: sel,
                        });
                        return;
                    }
                }
            }
            if sel.contains((ix, iy)) {
                match st.tool {
                    Tool::Pointer => {
                        st.drag = Some(Drag::Move {
                            grab: (ix, iy),
                            orig: sel,
                        });
                    }
                    Tool::Text => {
                        drop(st);
                        self.show_text_entry(x, y);
                    }
                    Tool::Number => {
                        let n = st.number_next;
                        st.number_next += 1;
                        st.annotations.push(Annotation::Number { pos: (ix, iy), n });
                        drop(st);
                        self.area.queue_draw();
                    }
                    Tool::Pen => {
                        st.draft = Some(Annotation::Pen {
                            points: vec![(ix, iy)],
                        });
                        st.drag = Some(Drag::Draw { start: (ix, iy) });
                    }
                    _ => {
                        st.drag = Some(Drag::Draw { start: (ix, iy) });
                    }
                }
                return;
            }
        }

        st.sel = None;
        st.toolbar = None;
        st.draft = None;
        st.tool = Tool::Pointer;
        st.drag = Some(Drag::New { anchor: (ix, iy) });
    }

    fn on_motion(&self, x: f64, y: f64) {
        let (ix, iy) = self.to_img(x, y);
        let mut st = self.state.borrow_mut();
        st.cursor = Some((ix, iy));

        let drag = st.drag.clone();
        let tool = st.tool;
        if let Some(drag) = drag {
            match drag {
                Drag::New { anchor } => {
                    st.sel = Some(Rect::from_points(anchor, (ix, iy)));
                }
                Drag::Move { grab, orig } => {
                    let dx = ix - grab.0;
                    let dy = iy - grab.1;
                    let mx = (st.image_w - orig.w).max(0.0);
                    let my = (st.image_h - orig.h).max(0.0);
                    st.sel = Some(Rect {
                        x: (orig.x + dx).clamp(0.0, mx),
                        y: (orig.y + dy).clamp(0.0, my),
                        w: orig.w,
                        h: orig.h,
                    });
                }
                Drag::Resize { handle, orig } => {
                    st.sel = Some(handle.apply(&orig, (ix, iy)).clamp_to(st.image_w, st.image_h));
                }
                Drag::Draw { start } => match tool {
                    Tool::Rect => {
                        st.draft = Some(Annotation::Rect {
                            a: start,
                            b: (ix, iy),
                        });
                    }
                    Tool::Ellipse => {
                        st.draft = Some(Annotation::Ellipse {
                            a: start,
                            b: (ix, iy),
                        });
                    }
                    Tool::Arrow => {
                        st.draft = Some(Annotation::Arrow {
                            a: start,
                            b: (ix, iy),
                        });
                    }
                    Tool::Mosaic => {
                        st.draft = Some(Annotation::Mosaic {
                            a: start,
                            b: (ix, iy),
                        });
                    }
                    Tool::Pen => {
                        if let Some(Annotation::Pen { points }) = &mut st.draft {
                            points.push((ix, iy));
                        }
                    }
                    _ => {}
                },
            }
        } else {
            st.hover_btn = st.toolbar.as_ref().and_then(|tb| tb.hit(x, y));
            st.hover_style = st.stylebar.as_ref().and_then(|sb| sb.hit(x, y));
        }

        drop(st);
        self.update_cursor(x, y);
        self.area.queue_draw();
    }

    fn update_cursor(&self, x: f64, y: f64) {
        let (ix, iy) = self.to_img(x, y);
        let st = self.state.borrow();
        let mut name = "crosshair";
        if st.drag.is_none() {
            if let Some(sel) = st.sel {
                if st.tool == Tool::Pointer {
                    let scale = if st.scale > 0.0 { st.scale } else { 1.0 };
                    let tol = 10.0 / scale;
                    let mut over_handle = false;
                    for h in Handle::ALL {
                        let p = h.pos(&sel);
                        if (p.0 - ix).abs() <= tol && (p.1 - iy).abs() <= tol {
                            name = match h {
                                Handle::NW | Handle::SE => "nwse-resize",
                                Handle::NE | Handle::SW => "nesw-resize",
                                Handle::N | Handle::S => "ns-resize",
                                Handle::E | Handle::W => "ew-resize",
                            };
                            over_handle = true;
                            break;
                        }
                    }
                    if !over_handle && sel.contains((ix, iy)) {
                        name = "move";
                    }
                }
            }
        }
        if st.hover_btn.is_some() || st.hover_style.is_some() {
            name = "pointer";
        }
        drop(st);
        self.area.set_cursor_from_name(Some(name));
    }

    fn on_release(&self, _x: f64, _y: f64) {
        let mut st = self.state.borrow_mut();
        if let Some(drag) = st.drag.take() {
            match drag {
                Drag::New { .. } => {
                    if let Some(r) = st.sel {
                        st.sel = if r.w < 4.0 || r.h < 4.0 {
                            None
                        } else {
                            Some(r.clamp_to(st.image_w, st.image_h))
                        };
                    }
                }
                Drag::Draw { .. } => {
                    if let Some(a) = st.draft.take() {
                        if a.is_meaningful() {
                            st.annotations.push(a);
                        }
                    }
                }
                _ => {}
            }
        }
        drop(st);
        self.area.queue_draw();
    }

    fn on_right_click(&self) {
        let mut st = self.state.borrow_mut();
        if st.dialog_open {
            return;
        }
        if st.sel.is_some() {
            st.sel = None;
            st.tool = Tool::Pointer;
            st.draft = None;
            drop(st);
            self.area.queue_draw();
        } else {
            drop(st);
            self.window.close();
        }
    }

    fn on_tool(&self, id: usize) {
        let mut st = self.state.borrow_mut();
        if let Some(tool) = render::tool_for_button(id) {
            st.tool = tool;
        } else {
            match id {
                8 => {
                    st.annotations.pop();
                }
                9 => {
                    drop(st);
                    self.save_as();
                    return;
                }
                10 => {
                    drop(st);
                    self.copy_and_close();
                    return;
                }
                11 => {
                    drop(st);
                    self.window.close();
                    return;
                }
                _ => {}
            }
        }
        drop(st);
        self.area.queue_draw();
    }

    fn on_key(&self, key: gdk::Key, mods: gdk::ModifierType) -> glib::Propagation {
        if self.state.borrow().text_editing || self.state.borrow().dialog_open {
            return glib::Propagation::Proceed;
        }
        let ctrl = mods.contains(gdk::ModifierType::CONTROL_MASK);
        match key {
            gdk::Key::Escape => {
                self.window.close();
                glib::Propagation::Stop
            }
            gdk::Key::Return | gdk::Key::KP_Enter => {
                self.copy_and_close();
                glib::Propagation::Stop
            }
            gdk::Key::c if ctrl => {
                self.copy_and_close();
                glib::Propagation::Stop
            }
            gdk::Key::s if ctrl => {
                self.save_as();
                glib::Propagation::Stop
            }
            gdk::Key::z if ctrl => {
                self.state.borrow_mut().annotations.pop();
                self.area.queue_draw();
                glib::Propagation::Stop
            }
            _ => glib::Propagation::Proceed,
        }
    }

    fn export_selection(&self) -> Option<Rc<cairo::ImageSurface>> {
        let (sel, anns, image, pixelated) = {
            let st = self.state.borrow();
            (
                st.sel,
                st.annotations.clone(),
                st.image.clone(),
                st.pixelated.clone(),
            )
        };
        let sel = sel?;
        match render::export(&image, &pixelated, sel, &anns) {
            Ok(out) => Some(Rc::new(out)),
            Err(e) => {
                eprintln!("导出失败: {e}");
                None
            }
        }
    }

    fn copy_and_close(&self) {
        let Some(out) = self.export_selection() else {
            return;
        };
        match io::copy_to_clipboard(&out) {
            Ok(()) => self.window.close(),
            Err(e) => eprintln!("复制失败: {e}"),
        }
    }

    fn save_as(&self) {
        if self.state.borrow().dialog_open {
            return;
        }
        let Some(out) = self.export_selection() else {
            return;
        };
        let (initial_dir, initial_name) = {
            let st = self.state.borrow();
            (st.cfg.save_dir.clone(), io::default_screenshot_name())
        };
        let dialog = gtk::FileDialog::builder()
            .title("保存截图")
            .modal(true)
            .initial_name(initial_name)
            .initial_folder(&gio::File::for_path(&initial_dir))
            .build();

        self.state.borrow_mut().dialog_open = true;
        let state = self.state.clone();
        let window = self.window.clone();
        let area = self.area.clone();
        glib::spawn_future_local(async move {
            match dialog.save_future(Some(&window)).await {
                Ok(file) => {
                    let Some(path) = file.path() else {
                        eprintln!("无法获取保存路径");
                        state.borrow_mut().dialog_open = false;
                        area.grab_focus();
                        return;
                    };
                    let path = if path
                        .extension()
                        .and_then(|ext| ext.to_str())
                        .is_some_and(|ext| ext.eq_ignore_ascii_case("png"))
                    {
                        path
                    } else {
                        path.with_extension("png")
                    };
                    match io::save_png_to(&out, &path) {
                        Ok(()) => {
                            {
                                let mut st = state.borrow_mut();
                                st.dialog_open = false;
                                if let Some(parent) = path.parent() {
                                    // 沙盒/portal 可能返回 /run/user/... 的临时路径，不作为下次默认目录
                                    if !parent.starts_with("/run/user") {
                                        st.cfg.save_dir = parent.to_path_buf();
                                    }
                                }
                                if let Err(e) = st.cfg.save() {
                                    eprintln!("保存配置失败: {e}");
                                }
                            }
                            println!("已保存: {}", path.display());
                            window.close();
                        }
                        Err(e) => {
                            eprintln!("保存失败: {e}");
                            state.borrow_mut().dialog_open = false;
                            area.grab_focus();
                        }
                    }
                }
                Err(e) => {
                    state.borrow_mut().dialog_open = false;
                    if e.matches(gtk::DialogError::Cancelled)
                        || e.matches(gtk::DialogError::Dismissed)
                    {
                        window.close();
                    } else {
                        eprintln!("打开保存对话框失败: {e}");
                        area.grab_focus();
                    }
                }
            }
        });
    }

    fn on_style(&self, hit: StyleHit) {
        {
            let mut st = self.state.borrow_mut();
            match hit {
                StyleHit::Color(i) => {
                    if let Some(color) = render::TEXT_PALETTE.get(i) {
                        st.text_style.color = *color;
                    }
                }
                StyleHit::SizeDown => {
                    st.text_style.size = (st.text_style.size - 2.0).max(render::TEXT_MIN_SIZE);
                }
                StyleHit::SizeUp => {
                    st.text_style.size = (st.text_style.size + 2.0).min(render::TEXT_MAX_SIZE);
                }
            }
            st.cfg.text_style = st.text_style;
            if let Err(e) = st.cfg.save() {
                eprintln!("保存配置失败: {e}");
            }
        }
        self.update_entry_css();
        self.area.queue_draw();
    }

    fn update_entry_css(&self) {
        let style = self.state.borrow().text_style;
        self.css.load_from_data(&entry_css(style));
    }

    fn show_text_entry(&self, x: f64, y: f64) {
        let (ix, iy) = self.to_img(x, y);
        {
            let mut st = self.state.borrow_mut();
            st.text_anchor = Some((ix, iy));
            st.text_editing = true;
        }
        self.update_entry_css();
        let max_x = (self.area.width() as f64 - 230.0).max(2.0);
        let max_y = (self.area.height() as f64 - 40.0).max(2.0);
        let px = x.clamp(2.0, max_x);
        let py = y.clamp(2.0, max_y);
        self.entry.set_text("");
        self.entry.set_margin_start(px as i32);
        self.entry.set_margin_top(py as i32);
        self.entry.set_visible(true);
        self.entry.grab_focus();
        self.area.queue_draw();
    }

    fn commit_text_if_editing(&self) {
        if self.state.borrow().text_editing {
            self.commit_text();
        }
    }

    fn commit_text(&self) {
        let text = self.entry.text().to_string();
        {
            let mut st = self.state.borrow_mut();
            if !text.trim().is_empty() {
                if let Some(pos) = st.text_anchor.take() {
                    let style = st.text_style;
                    st.annotations.push(Annotation::Text { pos, text, style });
                }
            } else {
                st.text_anchor = None;
            }
            st.text_editing = false;
        }
        self.entry.set_visible(false);
        self.area.grab_focus();
        self.area.queue_draw();
    }

    fn cancel_text(&self) {
        {
            let mut st = self.state.borrow_mut();
            st.text_anchor = None;
            st.text_editing = false;
        }
        self.entry.set_visible(false);
        self.area.grab_focus();
        self.area.queue_draw();
    }
}

fn entry_css(style: TextStyle) -> String {
    let color = style.color;
    let luma = 0.2126 * color.0 + 0.7152 * color.1 + 0.0722 * color.2;
    let (background, border) = if luma < 0.25 {
        ("rgba(250,250,250,0.95)", "rgba(0,0,0,0.35)")
    } else {
        ("rgba(20,20,20,0.92)", "rgba(255,255,255,0.35)")
    };
    format!(
        "entry.wxshot-entry {{ background: {background}; color: {}; \
         border: 1px solid {border}; border-radius: 6px; padding: 2px 6px; }}",
        config::color_to_hex(color)
    )
}
