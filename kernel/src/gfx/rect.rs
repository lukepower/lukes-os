use core::cmp::{max, min};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Rect {
    pub x: i32,
    pub y: i32,
    pub width: u32,
    pub height: u32,
}

impl Rect {
    pub const fn new(x: i32, y: i32, width: u32, height: u32) -> Self {
        Self { x, y, width, height }
    }

    pub fn right(&self) -> i32 {
        self.x + self.width as i32
    }

    pub fn bottom(&self) -> i32 {
        self.y + self.height as i32
    }

    pub fn contains_point(&self, px: i32, py: i32) -> bool {
        px >= self.x && px < self.right() && py >= self.y && py < self.bottom()
    }

    pub fn union(&self, other: &Rect) -> Rect {
        if self.width == 0 || self.height == 0 {
            return *other;
        }
        if other.width == 0 || other.height == 0 {
            return *self;
        }

        let x1 = min(self.x, other.x);
        let y1 = min(self.y, other.y);
        let x2 = max(self.right(), other.right());
        let y2 = max(self.bottom(), other.bottom());

        Rect {
            x: x1,
            y: y1,
            width: (x2 - x1).max(0) as u32,
            height: (y2 - y1).max(0) as u32,
        }
    }

    pub fn clamp_to(&self, bounds: &Rect) -> Option<Rect> {
        let x1 = max(self.x, bounds.x);
        let y1 = max(self.y, bounds.y);
        let x2 = min(self.right(), bounds.right());
        let y2 = min(self.bottom(), bounds.bottom());

        if x2 > x1 && y2 > y1 {
            Some(Rect {
                x: x1,
                y: y1,
                width: (x2 - x1) as u32,
                height: (y2 - y1) as u32,
            })
        } else {
            None
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub struct DirtyRegion {
    bounding_box: Option<Rect>,
}

impl DirtyRegion {
    pub const fn empty() -> Self {
        Self { bounding_box: None }
    }

    pub fn mark_dirty(&mut self, rect: Rect) {
        if rect.width == 0 || rect.height == 0 {
            return;
        }
        match self.bounding_box {
            Some(ref mut current) => {
                *current = current.union(&rect);
            }
            None => {
                self.bounding_box = Some(rect);
            }
        }
    }

    pub fn take(&mut self) -> Option<Rect> {
        self.bounding_box.take()
    }

    pub fn is_dirty(&self) -> bool {
        self.bounding_box.is_some()
    }
}
