pub const DEFAULT_TEXT_SIZE: f64 = 22.0;
pub const DEFAULT_TEXT_COLOR: (f64, f64, f64) = (1.0, 0.23, 0.19);

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TextStyle {
    pub size: f64,
    pub color: (f64, f64, f64),
}

impl Default for TextStyle {
    fn default() -> Self {
        Self {
            size: DEFAULT_TEXT_SIZE,
            color: DEFAULT_TEXT_COLOR,
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Tool {
    Pointer,
    Rect,
    Ellipse,
    Arrow,
    Pen,
    Text,
    Mosaic,
    Number,
}

impl Tool {
    pub fn from_id(id: usize) -> Option<Tool> {
        Some(match id {
            0 => Tool::Pointer,
            1 => Tool::Rect,
            2 => Tool::Ellipse,
            3 => Tool::Arrow,
            4 => Tool::Pen,
            5 => Tool::Text,
            6 => Tool::Mosaic,
            7 => Tool::Number,
            _ => return None,
        })
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Rect {
    pub x: f64,
    pub y: f64,
    pub w: f64,
    pub h: f64,
}

impl Rect {
    pub fn from_points(a: (f64, f64), b: (f64, f64)) -> Self {
        Rect {
            x: a.0.min(b.0),
            y: a.1.min(b.1),
            w: (a.0 - b.0).abs(),
            h: (a.1 - b.1).abs(),
        }
    }

    pub fn right(&self) -> f64 {
        self.x + self.w
    }

    pub fn bottom(&self) -> f64 {
        self.y + self.h
    }

    pub fn contains(&self, p: (f64, f64)) -> bool {
        p.0 >= self.x && p.0 <= self.right() && p.1 >= self.y && p.1 <= self.bottom()
    }

    pub fn clamp_to(&self, max_w: f64, max_h: f64) -> Rect {
        let x = self.x.max(0.0).min(max_w);
        let y = self.y.max(0.0).min(max_h);
        let w = self.w.max(0.0).min(max_w - x);
        let h = self.h.max(0.0).min(max_h - y);
        Rect { x, y, w, h }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Handle {
    NW,
    N,
    NE,
    E,
    SE,
    S,
    SW,
    W,
}

impl Handle {
    pub const ALL: [Handle; 8] = [
        Handle::NW,
        Handle::N,
        Handle::NE,
        Handle::E,
        Handle::SE,
        Handle::S,
        Handle::SW,
        Handle::W,
    ];

    pub fn pos(self, r: &Rect) -> (f64, f64) {
        match self {
            Handle::NW => (r.x, r.y),
            Handle::N => (r.x + r.w / 2.0, r.y),
            Handle::NE => (r.right(), r.y),
            Handle::E => (r.right(), r.y + r.h / 2.0),
            Handle::SE => (r.right(), r.bottom()),
            Handle::S => (r.x + r.w / 2.0, r.bottom()),
            Handle::SW => (r.x, r.bottom()),
            Handle::W => (r.x, r.y + r.h / 2.0),
        }
    }

    pub fn apply(self, r: &Rect, p: (f64, f64)) -> Rect {
        let mut x0 = r.x;
        let mut y0 = r.y;
        let mut x1 = r.right();
        let mut y1 = r.bottom();
        match self {
            Handle::NW | Handle::W | Handle::SW => x0 = p.0,
            Handle::NE | Handle::E | Handle::SE => x1 = p.0,
            Handle::N | Handle::S => {}
        }
        match self {
            Handle::NW | Handle::N | Handle::NE => y0 = p.1,
            Handle::SW | Handle::S | Handle::SE => y1 = p.1,
            Handle::W | Handle::E => {}
        }
        Rect::from_points((x0, y0), (x1, y1))
    }
}

#[derive(Clone, Debug)]
pub enum Annotation {
    Rect {
        a: (f64, f64),
        b: (f64, f64),
    },
    Ellipse {
        a: (f64, f64),
        b: (f64, f64),
    },
    Arrow {
        a: (f64, f64),
        b: (f64, f64),
    },
    Pen {
        points: Vec<(f64, f64)>,
    },
    Text {
        pos: (f64, f64),
        text: String,
        style: TextStyle,
    },
    Mosaic {
        a: (f64, f64),
        b: (f64, f64),
    },
    Number {
        pos: (f64, f64),
        n: u32,
    },
}

impl Annotation {
    pub fn is_meaningful(&self) -> bool {
        match self {
            Annotation::Rect { a, b }
            | Annotation::Ellipse { a, b }
            | Annotation::Arrow { a, b }
            | Annotation::Mosaic { a, b } => {
                (a.0 - b.0).abs() > 2.0 || (a.1 - b.1).abs() > 2.0
            }
            Annotation::Pen { points } => points.len() > 1,
            Annotation::Text { text, .. } => !text.trim().is_empty(),
            Annotation::Number { .. } => true,
        }
    }
}
