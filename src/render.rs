use gtk4::cairo::{self, Context, Filter, Format, ImageSurface, LineCap, LineJoin};
use gtk4::pango;
use std::f64::consts::PI;

use crate::model::{Annotation, Handle, Rect, TextStyle, Tool};

pub const ACCENT: (f64, f64, f64) = (0.027, 0.757, 0.376);
pub const DRAW: (f64, f64, f64) = (1.0, 0.23, 0.19);
pub const DRAW_WIDTH: f64 = 3.0;
pub const FONT: &str = "Noto Sans CJK SC";

pub const TEXT_MIN_SIZE: f64 = 12.0;
pub const TEXT_MAX_SIZE: f64 = 48.0;
pub const TEXT_PALETTE: [(f64, f64, f64); 8] = [
    (1.0, 0.231, 0.188),  // #FF3B30 红
    (1.0, 0.584, 0.0),    // #FF9500 橙
    (1.0, 0.800, 0.0),    // #FFCC00 黄
    (0.204, 0.780, 0.349), // #34C759 绿
    (0.0, 0.478, 1.0),    // #007AFF 蓝
    (0.686, 0.322, 0.871), // #AF52DE 紫
    (1.0, 1.0, 1.0),      // #FFFFFF 白
    (0.0, 0.0, 0.0),      // #000000 黑
];

pub const BTN: f64 = 38.0;
pub const PAD: f64 = 6.0;
pub const N_BUTTONS: usize = 12;

const SWATCH: f64 = 30.0;
const SIZE_BTN: f64 = 32.0;
const SIZE_LABEL_W: f64 = 34.0;

#[derive(Clone, Debug)]
pub struct Button {
    pub id: usize,
    pub x: f64,
    pub y: f64,
    pub w: f64,
    pub h: f64,
}

#[derive(Clone, Debug, Default)]
pub struct ToolbarLayout {
    pub x: f64,
    pub y: f64,
    pub w: f64,
    pub h: f64,
    pub buttons: Vec<Button>,
}

impl ToolbarLayout {
    pub fn hit(&self, x: f64, y: f64) -> Option<usize> {
        self.buttons
            .iter()
            .find(|b| x >= b.x && x <= b.x + b.w && y >= b.y && y <= b.y + b.h)
            .map(|b| b.id)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StyleHit {
    Color(usize),
    SizeDown,
    SizeUp,
}

#[derive(Clone, Debug)]
pub struct StyleBarLayout {
    pub x: f64,
    pub y: f64,
    pub w: f64,
    pub h: f64,
    pub swatches: Vec<(Rect, usize)>,
    pub minus: Rect,
    pub plus: Rect,
    pub size_label: Rect,
}

impl StyleBarLayout {
    pub fn hit(&self, x: f64, y: f64) -> Option<StyleHit> {
        if let Some((_, idx)) = self.swatches.iter().find(|(r, _)| contains(r, x, y)) {
            return Some(StyleHit::Color(*idx));
        }
        if contains(&self.minus, x, y) {
            return Some(StyleHit::SizeDown);
        }
        if contains(&self.plus, x, y) {
            return Some(StyleHit::SizeUp);
        }
        None
    }
}

fn contains(r: &Rect, x: f64, y: f64) -> bool {
    x >= r.x && x <= r.right() && y >= r.y && y <= r.bottom()
}

pub struct Scene<'a> {
    pub image: &'a ImageSurface,
    pub pixelated: &'a ImageSurface,
    pub sel: Option<Rect>,
    pub annotations: &'a [Annotation],
    pub draft: Option<&'a Annotation>,
    pub tool: Tool,
    pub cursor: Option<(f64, f64)>,
    pub selecting: bool,
    pub hover_btn: Option<usize>,
}

pub fn tool_for_button(id: usize) -> Option<Tool> {
    Tool::from_id(id)
}

pub fn draw_scene(
    cr: &Context,
    s: &Scene,
    w: f64,
    h: f64,
    scale: f64,
) -> Option<ToolbarLayout> {
    // frozen screen
    let _ = cr.save();
    cr.scale(scale, scale);
    let _ = cr.set_source_surface(s.image, 0.0, 0.0);
    let _ = cr.paint();
    let _ = cr.restore();

    // dim outside the selection
    cr.set_source_rgba(0.0, 0.0, 0.0, 0.45);
    match s.sel {
        Some(r) => {
            let x = (r.x * scale).clamp(0.0, w);
            let y = (r.y * scale).clamp(0.0, h);
            let right = (r.right() * scale).clamp(0.0, w);
            let bottom = (r.bottom() * scale).clamp(0.0, h);
            if y > 0.0 {
                cr.rectangle(0.0, 0.0, w, y);
            }
            if bottom < h {
                cr.rectangle(0.0, bottom, w, h - bottom);
            }
            if x > 0.0 {
                cr.rectangle(0.0, y, x, bottom - y);
            }
            if right < w {
                cr.rectangle(right, y, w - right, bottom - y);
            }
            let _ = cr.fill();
        }
        None => {
            cr.rectangle(0.0, 0.0, w, h);
            let _ = cr.fill();
        }
    }

    // annotations, clipped to the selection
    if let Some(r) = s.sel {
        let _ = cr.save();
        cr.rectangle(r.x * scale, r.y * scale, r.w * scale, r.h * scale);
        let _ = cr.clip();
        cr.scale(scale, scale);
        for a in s.annotations {
            draw_annotation(cr, a, s.pixelated);
        }
        if let Some(d) = s.draft {
            draw_annotation(cr, d, s.pixelated);
        }
        let _ = cr.restore();
    }

    if s.sel.is_none() && !s.selecting {
        draw_hint(cr, w);
    }

    if let Some(r) = s.sel {
        draw_selection(cr, &r, scale, w, h);
        draw_size_label(cr, &r, scale, w, h);
    }

    if s.selecting {
        if let Some(p) = s.cursor {
            draw_magnifier(cr, s.image, p, scale, w, h);
        }
    }

    if let Some(r) = s.sel {
        let tb = build_toolbar(&r, scale, w, h);
        draw_toolbar(cr, &tb, s.hover_btn, s.tool);
        Some(tb)
    } else {
        None
    }
}

pub fn build_toolbar(sel: &Rect, scale: f64, w: f64, h: f64) -> ToolbarLayout {
    let bar_w = N_BUTTONS as f64 * BTN + PAD * 2.0;
    let bar_h = BTN + PAD * 2.0;
    let sx = sel.x * scale;
    let sy = sel.y * scale;
    let sw = sel.w * scale;
    let sh = sel.h * scale;

    let mut x = sx + sw - bar_w;
    let mut y = sy + sh + 10.0;
    if y + bar_h > h - 8.0 {
        y = sy - bar_h - 10.0;
    }
    if y < 8.0 {
        y = (sy + sh - bar_h - 10.0).max(8.0);
    }
    x = x.max(8.0).min((w - bar_w - 8.0).max(8.0));

    let mut buttons = Vec::with_capacity(N_BUTTONS);
    for i in 0..N_BUTTONS {
        buttons.push(Button {
            id: i,
            x: x + PAD + i as f64 * BTN,
            y: y + PAD,
            w: BTN,
            h: BTN,
        });
    }
    ToolbarLayout {
        x,
        y,
        w: bar_w,
        h: bar_h,
        buttons,
    }
}

pub fn build_style_bar(tb: &ToolbarLayout, w: f64, h: f64) -> StyleBarLayout {
    let gap = 10.0;
    let bar_w = PAD * 2.0
        + TEXT_PALETTE.len() as f64 * SWATCH
        + gap
        + SIZE_BTN * 2.0
        + SIZE_LABEL_W;
    let bar_h = BTN + PAD * 2.0;
    let mut x = tb.x + tb.w - bar_w;
    x = x.max(8.0).min((w - bar_w - 8.0).max(8.0));
    let mut y = tb.y - 6.0 - bar_h;
    if y < 8.0 {
        y = tb.y + tb.h + 6.0;
    }
    if y + bar_h > h - 8.0 {
        y = (tb.y - bar_h - 6.0).max(8.0);
    }

    let mut swatches = Vec::with_capacity(TEXT_PALETTE.len());
    let mut sx = x + PAD;
    for i in 0..TEXT_PALETTE.len() {
        swatches.push((
            Rect {
                x: sx,
                y: y + PAD,
                w: SWATCH,
                h: BTN,
            },
            i,
        ));
        sx += SWATCH;
    }
    let minus = Rect {
        x: sx + gap,
        y: y + PAD,
        w: SIZE_BTN,
        h: BTN,
    };
    let size_label = Rect {
        x: minus.right(),
        y: y + PAD,
        w: SIZE_LABEL_W,
        h: BTN,
    };
    let plus = Rect {
        x: size_label.right(),
        y: y + PAD,
        w: SIZE_BTN,
        h: BTN,
    };
    StyleBarLayout {
        x,
        y,
        w: bar_w,
        h: bar_h,
        swatches,
        minus,
        plus,
        size_label,
    }
}

pub fn draw_style_bar(
    cr: &Context,
    bar: &StyleBarLayout,
    style: TextStyle,
    hover: Option<StyleHit>,
) {
    round_rect(cr, bar.x, bar.y, bar.w, bar.h, 12.0);
    cr.set_source_rgba(0.09, 0.09, 0.10, 0.94);
    let _ = cr.fill_preserve();
    cr.set_source_rgba(1.0, 1.0, 1.0, 0.10);
    cr.set_line_width(1.0);
    let _ = cr.stroke();

    for (rect, idx) in &bar.swatches {
        let color = TEXT_PALETTE[*idx];
        let selected = same_color(color, style.color);
        if hover == Some(StyleHit::Color(*idx)) {
            round_rect(cr, rect.x + 2.0, rect.y + 2.0, rect.w - 4.0, rect.h - 4.0, 9.0);
            cr.set_source_rgba(1.0, 1.0, 1.0, 0.10);
            let _ = cr.fill();
        }
        let cx = rect.x + rect.w / 2.0;
        let cy = rect.y + rect.h / 2.0;
        cr.arc(cx, cy, 8.0, 0.0, 2.0 * PI);
        cr.set_source_rgba(color.0, color.1, color.2, 1.0);
        let _ = cr.fill_preserve();
        cr.set_source_rgba(1.0, 1.0, 1.0, 0.25);
        cr.set_line_width(1.0);
        let _ = cr.stroke();
        if selected {
            cr.arc(cx, cy, 11.5, 0.0, 2.0 * PI);
            cr.set_source_rgba(1.0, 1.0, 1.0, 0.95);
            cr.set_line_width(2.0);
            let _ = cr.stroke();
        }
    }

    draw_size_button(cr, &bar.minus, "A-", hover == Some(StyleHit::SizeDown));
    draw_size_button(cr, &bar.plus, "A+", hover == Some(StyleHit::SizeUp));
    draw_text_centered(
        cr,
        &bar.size_label,
        &format!("{}", style.size.round() as i32),
        14.0,
        (1.0, 1.0, 1.0),
    );
}

fn draw_size_button(cr: &Context, rect: &Rect, label: &str, hot: bool) {
    if hot {
        round_rect(cr, rect.x + 2.0, rect.y + 2.0, rect.w - 4.0, rect.h - 4.0, 8.0);
        cr.set_source_rgba(1.0, 1.0, 1.0, 0.10);
        let _ = cr.fill();
    }
    draw_text_centered(cr, rect, label, 13.0, (1.0, 1.0, 1.0));
}

fn draw_text_centered(
    cr: &Context,
    rect: &Rect,
    text: &str,
    size: f64,
    color: (f64, f64, f64),
) {
    let (tw, th) = text_size(cr, text, size);
    draw_text_ui(
        cr,
        rect.x + (rect.w - tw) / 2.0,
        rect.y + (rect.h - th) / 2.0,
        text,
        size,
        color,
    );
}

fn same_color(a: (f64, f64, f64), b: (f64, f64, f64)) -> bool {
    (a.0 - b.0).abs() < 0.01 && (a.1 - b.1).abs() < 0.01 && (a.2 - b.2).abs() < 0.01
}

fn draw_toolbar(cr: &Context, tb: &ToolbarLayout, hover: Option<usize>, tool: Tool) {
    round_rect(cr, tb.x, tb.y, tb.w, tb.h, 12.0);
    cr.set_source_rgba(0.09, 0.09, 0.10, 0.94);
    let _ = cr.fill_preserve();
    cr.set_source_rgba(1.0, 1.0, 1.0, 0.10);
    cr.set_line_width(1.0);
    let _ = cr.stroke();

    // divider between tools and actions
    let div_x = tb.x + PAD + 8.0 * BTN;
    cr.move_to(div_x, tb.y + 10.0);
    cr.line_to(div_x, tb.y + tb.h - 10.0);
    cr.set_source_rgba(1.0, 1.0, 1.0, 0.12);
    cr.set_line_width(1.0);
    let _ = cr.stroke();

    for b in &tb.buttons {
        let active = tool_for_button(b.id) == Some(tool);
        let hot = hover == Some(b.id);
        if b.id == 10 {
            // 绿色「完成」按钮：复制并关闭
            round_rect(cr, b.x + 2.0, b.y + 2.0, b.w - 4.0, b.h - 4.0, 9.0);
            let a = if hot { 1.0 } else { 0.92 };
            cr.set_source_rgba(ACCENT.0, ACCENT.1, ACCENT.2, a);
            let _ = cr.fill();
            draw_icon(cr, b.id, b.x + b.w / 2.0, b.y + b.h / 2.0, (1.0, 1.0, 1.0));
            continue;
        }
        if active || hot {
            round_rect(cr, b.x + 2.0, b.y + 2.0, b.w - 4.0, b.h - 4.0, 9.0);
            if active {
                cr.set_source_rgba(ACCENT.0, ACCENT.1, ACCENT.2, 0.22);
            } else {
                cr.set_source_rgba(1.0, 1.0, 1.0, 0.10);
            }
            let _ = cr.fill();
        }
        let color = if active {
            ACCENT
        } else {
            (1.0, 1.0, 1.0)
        };
        draw_icon(cr, b.id, b.x + b.w / 2.0, b.y + b.h / 2.0, color);
    }
}

fn draw_icon(cr: &Context, id: usize, cx: f64, cy: f64, color: (f64, f64, f64)) {
    let _ = cr.save();
    cr.translate(cx, cy);
    cr.set_line_width(1.8);
    cr.set_line_cap(LineCap::Round);
    cr.set_line_join(LineJoin::Round);
    cr.set_source_rgba(color.0, color.1, color.2, 1.0);
    match id {
        0 => {
            cr.move_to(-5.0, -8.0);
            cr.line_to(6.5, 1.0);
            cr.line_to(1.0, 1.0);
            cr.line_to(3.5, 8.0);
            cr.line_to(0.5, 9.5);
            cr.line_to(-2.0, 2.0);
            cr.line_to(-6.0, 5.0);
            cr.close_path();
            let _ = cr.fill();
        }
        1 => {
            cr.rectangle(-8.0, -6.5, 16.0, 13.0);
            let _ = cr.stroke();
        }
        2 => {
            let _ = cr.save();
            let _ = cr.scale(1.0, 0.78);
            cr.arc(0.0, 0.0, 8.0, 0.0, 2.0 * PI);
            let _ = cr.restore();
            let _ = cr.stroke();
        }
        3 => {
            cr.move_to(-7.0, 7.0);
            cr.line_to(6.0, -6.0);
            let _ = cr.stroke();
            cr.move_to(6.0, -6.0);
            cr.line_to(0.5, -5.2);
            cr.move_to(6.0, -6.0);
            cr.line_to(5.2, -0.5);
            let _ = cr.stroke();
        }
        4 => {
            cr.move_to(-8.0, 5.0);
            cr.curve_to(-4.0, -7.0, -1.0, 7.0, 3.0, -2.0);
            cr.curve_to(5.0, -6.0, 6.5, 2.0, 8.0, -4.0);
            let _ = cr.stroke();
        }
        5 => {
            cr.move_to(-6.5, -7.0);
            cr.line_to(6.5, -7.0);
            cr.move_to(0.0, -7.0);
            cr.line_to(0.0, 7.5);
            let _ = cr.stroke();
        }
        6 => {
            for (dx, dy) in [(-7.0, -7.0), (0.5, -7.0), (-7.0, 0.5), (0.5, 0.5)] {
                cr.rectangle(dx, dy, 6.5, 6.5);
            }
            let _ = cr.fill();
        }
        7 => {
            cr.arc(0.0, 0.0, 8.0, 0.0, 2.0 * PI);
            let _ = cr.stroke();
            cr.move_to(-2.5, -4.0);
            cr.line_to(1.0, -6.5);
            cr.line_to(1.0, 6.5);
            let _ = cr.stroke();
        }
        8 => {
            cr.arc(0.0, 0.0, 6.5, PI * 0.15, PI * 1.55);
            let _ = cr.stroke();
            cr.move_to(-7.0, -3.0);
            cr.line_to(-8.0, 3.5);
            cr.line_to(-2.0, 2.0);
            cr.close_path();
            let _ = cr.fill();
        }
        9 => {
            cr.rectangle(-7.0, -7.0, 14.0, 14.0);
            let _ = cr.stroke();
            cr.rectangle(-3.5, -7.0, 7.0, 4.5);
            let _ = cr.stroke();
            cr.rectangle(-4.5, 1.0, 9.0, 6.0);
            let _ = cr.stroke();
        }
        10 => {
            cr.move_to(-7.5, 0.0);
            cr.line_to(-2.0, 5.5);
            cr.line_to(7.5, -5.5);
            cr.set_line_width(2.6);
            let _ = cr.stroke();
        }
        _ => {
            cr.move_to(-6.5, -6.5);
            cr.line_to(6.5, 6.5);
            cr.move_to(6.5, -6.5);
            cr.line_to(-6.5, 6.5);
            let _ = cr.stroke();
        }
    }
    let _ = cr.restore();
}

fn draw_selection(cr: &Context, r: &Rect, scale: f64, w: f64, h: f64) {
    let x = r.x * scale;
    let y = r.y * scale;
    let rw = r.w * scale;
    let rh = r.h * scale;

    cr.rectangle(x, y, rw, rh);
    cr.set_source_rgba(ACCENT.0, ACCENT.1, ACCENT.2, 1.0);
    cr.set_line_width(1.5);
    let _ = cr.stroke();

    let hs = 4.0;
    for handle in Handle::ALL {
        let p = handle.pos(r);
        let px = p.0 * scale;
        let py = p.1 * scale;
        if px < -hs || py < -hs || px > w + hs || py > h + hs {
            continue;
        }
        cr.rectangle(px - hs, py - hs, hs * 2.0, hs * 2.0);
        cr.set_source_rgba(1.0, 1.0, 1.0, 1.0);
        let _ = cr.fill_preserve();
        cr.set_source_rgba(ACCENT.0, ACCENT.1, ACCENT.2, 1.0);
        cr.set_line_width(1.5);
        let _ = cr.stroke();
    }
}

fn draw_hint(cr: &Context, w: f64) {
    let text = "拖动选择区域    Esc 退出";
    let (tw, th) = text_size(cr, text, 14.0);
    let bw = tw + 32.0;
    let bh = th + 16.0;
    let x = (w - bw) / 2.0;
    let y = 26.0;
    round_rect(cr, x, y, bw, bh, 10.0);
    cr.set_source_rgba(0.06, 0.06, 0.07, 0.82);
    let _ = cr.fill();
    draw_text_ui(cr, x + 16.0, y + 8.0, text, 14.0, (1.0, 1.0, 1.0));
}

fn draw_size_label(cr: &Context, r: &Rect, scale: f64, w: f64, _h: f64) {
    let text = format!("{} × {}", r.w.round() as i64, r.h.round() as i64);
    let (tw, th) = text_size(cr, &text, 13.0);
    let pad_x = 8.0;
    let pad_y = 4.0;
    let bw = tw + pad_x * 2.0;
    let bh = th + pad_y * 2.0;
    let mut x = (r.x * scale).max(4.0);
    let mut y = (r.y * scale) - bh - 6.0;
    if y < 4.0 {
        y = (r.y * scale) + 6.0;
    }
    if x + bw > w - 4.0 {
        x = w - bw - 4.0;
    }
    round_rect(cr, x, y, bw, bh, 6.0);
    cr.set_source_rgba(0.0, 0.0, 0.0, 0.72);
    let _ = cr.fill();
    draw_text_ui(cr, x + pad_x, y + pad_y, &text, 13.0, (1.0, 1.0, 1.0));
}

fn draw_magnifier(
    cr: &Context,
    image: &ImageSurface,
    cursor: (f64, f64),
    scale: f64,
    w: f64,
    h: f64,
) {
    let cw = (cursor.0 * scale, cursor.1 * scale);
    let radius = 52.0;
    let mut cx = cw.0 + 86.0;
    let mut cy = cw.1 + 86.0;
    if cx + radius > w - 8.0 {
        cx = cw.0 - 86.0 - radius;
    }
    if cy + radius > h - 8.0 {
        cy = cw.1 - 86.0 - radius;
    }
    cx = cx.clamp(radius + 4.0, w - radius - 4.0);
    cy = cy.clamp(radius + 4.0, h - radius - 4.0);

    let _ = cr.save();
    cr.arc(cx, cy, radius, 0.0, 2.0 * PI);
    let _ = cr.clip();
    cr.set_source_rgba(0.05, 0.05, 0.05, 1.0);
    let _ = cr.paint();

    let zoom = scale * 4.0;
    let _ = cr.save();
    cr.translate(cx, cy);
    cr.scale(zoom, zoom);
    cr.translate(-cursor.0, -cursor.1);
    let _ = cr.set_source_surface(image, 0.0, 0.0);
    let _ = cr.paint();
    let _ = cr.restore();
    let _ = cr.restore();

    cr.arc(cx, cy, radius, 0.0, 2.0 * PI);
    cr.set_source_rgba(1.0, 1.0, 1.0, 0.85);
    cr.set_line_width(1.5);
    let _ = cr.stroke();

    // crosshair in the middle
    cr.set_source_rgba(ACCENT.0, ACCENT.1, ACCENT.2, 0.95);
    cr.set_line_width(1.0);
    cr.move_to(cx - radius, cy);
    cr.line_to(cx + radius, cy);
    cr.move_to(cx, cy - radius);
    cr.line_to(cx, cy + radius);
    let _ = cr.stroke();
}

fn draw_annotation(cr: &Context, a: &Annotation, pixelated: &ImageSurface) {
    cr.set_line_width(DRAW_WIDTH);
    cr.set_line_cap(LineCap::Round);
    cr.set_line_join(LineJoin::Round);
    cr.set_source_rgba(DRAW.0, DRAW.1, DRAW.2, 1.0);

    match a {
        Annotation::Rect { a, b } => {
            let r = Rect::from_points(*a, *b);
            cr.rectangle(r.x, r.y, r.w, r.h);
            let _ = cr.stroke();
        }
        Annotation::Ellipse { a, b } => {
            let cx = (a.0 + b.0) / 2.0;
            let cy = (a.1 + b.1) / 2.0;
            let rx = (a.0 - b.0).abs() / 2.0;
            let ry = (a.1 - b.1).abs() / 2.0;
            if rx > 0.1 && ry > 0.1 {
                let _ = cr.save();
                cr.translate(cx, cy);
                let _ = cr.scale(rx, ry);
                cr.arc(0.0, 0.0, 1.0, 0.0, 2.0 * PI);
                let _ = cr.restore();
                let _ = cr.stroke();
            }
        }
        Annotation::Arrow { a, b } => {
            let angle = (b.1 - a.1).atan2(b.0 - a.0);
            let len = 16.0;
            let spread = 0.48;
            let p1 = (
                b.0 - len * (angle - spread).cos(),
                b.1 - len * (angle - spread).sin(),
            );
            let p2 = (
                b.0 - len * (angle + spread).cos(),
                b.1 - len * (angle + spread).sin(),
            );
            cr.move_to(a.0, a.1);
            cr.line_to(b.0, b.1);
            let _ = cr.stroke();
            cr.move_to(b.0, b.1);
            cr.line_to(p1.0, p1.1);
            cr.move_to(b.0, b.1);
            cr.line_to(p2.0, p2.1);
            let _ = cr.stroke();
        }
        Annotation::Pen { points } => {
            if let Some((first, rest)) = points.split_first() {
                cr.move_to(first.0, first.1);
                for p in rest {
                    cr.line_to(p.0, p.1);
                }
                let _ = cr.stroke();
            }
        }
        Annotation::Text { pos, text, style } => {
            draw_text_ui(cr, pos.0, pos.1, text, style.size, style.color);
        }
        Annotation::Mosaic { a, b } => {
            let r = Rect::from_points(*a, *b);
            let _ = cr.save();
            cr.rectangle(r.x, r.y, r.w, r.h);
            let _ = cr.clip();
            let _ = cr.set_source_surface(pixelated, 0.0, 0.0);
            let _ = cr.paint();
            let _ = cr.restore();
        }
        Annotation::Number { pos, n } => {
            let radius = 14.0;
            cr.arc(pos.0, pos.1, radius, 0.0, 2.0 * PI);
            cr.set_source_rgba(DRAW.0, DRAW.1, DRAW.2, 1.0);
            let _ = cr.fill();
            let text = n.to_string();
            let (tw, th) = text_size(cr, &text, 18.0);
            draw_text_ui(
                cr,
                pos.0 - tw / 2.0,
                pos.1 - th / 2.0,
                &text,
                18.0,
                (1.0, 1.0, 1.0),
            );
        }
    }
}

fn make_layout(cr: &Context, text: &str, size: f64) -> pango::Layout {
    let layout = pangocairo::functions::create_layout(cr);
    let mut desc = pango::FontDescription::from_string(&format!("{FONT} Bold"));
    desc.set_absolute_size(size * pango::SCALE as f64);
    layout.set_font_description(Some(&desc));
    layout.set_text(text);
    layout
}

fn text_size(cr: &Context, text: &str, size: f64) -> (f64, f64) {
    let layout = make_layout(cr, text, size);
    let (w, h) = layout.pixel_size();
    (w as f64, h as f64)
}

pub fn draw_text_ui(
    cr: &Context,
    x: f64,
    y: f64,
    text: &str,
    size: f64,
    color: (f64, f64, f64),
) {
    let layout = make_layout(cr, text, size);
    cr.set_source_rgba(color.0, color.1, color.2, 1.0);
    cr.move_to(x, y);
    pangocairo::functions::show_layout(cr, &layout);
}

pub fn round_rect(cr: &Context, x: f64, y: f64, w: f64, h: f64, r: f64) {
    let r = r.min(w / 2.0).min(h / 2.0);
    cr.new_sub_path();
    cr.arc(x + w - r, y + r, r, -PI / 2.0, 0.0);
    cr.arc(x + w - r, y + h - r, r, 0.0, PI / 2.0);
    cr.arc(x + r, y + h - r, r, PI / 2.0, PI);
    cr.arc(x + r, y + r, r, PI, 1.5 * PI);
    cr.close_path();
}

/// Pre-pixelate the whole screenshot so mosaic previews and exports are cheap.
pub fn pixelate(image: &ImageSurface) -> ImageSurface {
    let w = image.width().max(1);
    let h = image.height().max(1);
    let block = 8;
    let sw = (w / block).max(1);
    let sh = (h / block).max(1);

    let small = ImageSurface::create(Format::ARgb32, sw, sh)
        .unwrap_or_else(|_| ImageSurface::create(Format::ARgb32, 1, 1).unwrap());
    if let Ok(c) = Context::new(&small) {
        c.scale(1.0 / block as f64, 1.0 / block as f64);
        let _ = c.set_source_surface(image, 0.0, 0.0);
        let _ = c.paint();
    }

    let out = ImageSurface::create(Format::ARgb32, w, h)
        .unwrap_or_else(|_| ImageSurface::create(Format::ARgb32, 1, 1).unwrap());
    if let Ok(c) = Context::new(&out) {
        c.scale(block as f64, block as f64);
        let pattern = cairo::SurfacePattern::create(&small);
        pattern.set_filter(Filter::Nearest);
        let _ = c.set_source(&pattern);
        let _ = c.paint();
    }
    out.flush();
    out
}

/// Flatten the selection plus annotations into a new image.
pub fn export(
    image: &ImageSurface,
    pixelated: &ImageSurface,
    sel: Rect,
    annotations: &[Annotation],
) -> Result<ImageSurface, String> {
    let iw = image.width() as f64;
    let ih = image.height() as f64;
    let x = sel.x.clamp(0.0, iw - 1.0).floor();
    let y = sel.y.clamp(0.0, ih - 1.0).floor();
    let w = sel.w.max(1.0).min(iw - x).floor();
    let h = sel.h.max(1.0).min(ih - y).floor();

    let out = ImageSurface::create(Format::ARgb32, w as i32, h as i32)
        .map_err(|e| format!("创建输出图层失败: {e}"))?;
    let cr = Context::new(&out).map_err(|e| format!("创建绘图上下文失败: {e}"))?;
    cr.translate(-x, -y);
    let _ = cr.set_source_surface(image, 0.0, 0.0);
    let _ = cr.paint();
    cr.rectangle(x, y, w, h);
    let _ = cr.clip();
    for a in annotations {
        draw_annotation(&cr, a, pixelated);
    }
    out.flush();
    Ok(out)
}
