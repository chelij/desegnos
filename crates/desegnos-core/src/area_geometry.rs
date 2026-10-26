use crate::{Rect, Region, RegionShape, TargetBounds};

impl RegionShape {
    /// Page pixels. Ellipses use a bounded approximation shared with scope checks.
    pub fn polygon(&self, rect: Rect) -> Vec<[f64; 2]> {
        let points = match self {
            Self::Rect => vec![[0., 0.], [1., 0.], [1., 1.], [0., 1.]],
            Self::Ellipse => (0..32)
                .map(|i| {
                    let angle = f64::from(i) * std::f64::consts::TAU / 32.;
                    [0.5 + angle.cos() * 0.5, 0.5 + angle.sin() * 0.5]
                })
                .collect(),
            Self::Freehand { points } => points.clone(),
        };
        points
            .into_iter()
            .map(|[x, y]| [rect.x + x * rect.width, rect.y + y * rect.height])
            .collect()
    }
}

fn cross(a: [f64; 2], b: [f64; 2], p: [f64; 2]) -> f64 {
    (b[0] - a[0]) * (p[1] - a[1]) - (b[1] - a[1]) * (p[0] - a[0])
}

fn on_segment(p: [f64; 2], a: [f64; 2], b: [f64; 2]) -> bool {
    cross(a, b, p).abs() <= 1e-8
        && p[0] >= a[0].min(b[0])
        && p[0] <= a[0].max(b[0])
        && p[1] >= a[1].min(b[1])
        && p[1] <= a[1].max(b[1])
}

fn inside(p: [f64; 2], polygon: &[[f64; 2]]) -> bool {
    let mut inside = false;
    for (a, b) in polygon
        .iter()
        .zip(polygon.iter().cycle().skip(1))
        .take(polygon.len())
    {
        if on_segment(p, *a, *b) {
            return true;
        }
        if (a[1] > p[1]) != (b[1] > p[1])
            && p[0] < (b[0] - a[0]) * (p[1] - a[1]) / (b[1] - a[1]) + a[0]
        {
            inside = !inside;
        }
    }
    inside
}

fn intersects(a: [f64; 2], b: [f64; 2], c: [f64; 2], d: [f64; 2]) -> bool {
    let opposite = |x: f64, y: f64| x < 0. && y > 0. || x > 0. && y < 0.;
    opposite(cross(a, b, c), cross(a, b, d)) && opposite(cross(c, d, a), cross(c, d, b))
        || on_segment(a, c, d)
        || on_segment(b, c, d)
        || on_segment(c, a, b)
        || on_segment(d, a, b)
}

impl Region {
    /// Includes edge contact, enclosure and crossings with no contained vertex.
    pub fn overlaps_rect(&self, rect: Rect) -> bool {
        if rect.x + rect.width < self.rect.x
            || rect.x > self.rect.x + self.rect.width
            || rect.y + rect.height < self.rect.y
            || rect.y > self.rect.y + self.rect.height
        {
            return false;
        }
        if self.has_targets() || self.shape.is_rect() {
            return true;
        }
        let polygon = self.shape.polygon(self.rect);
        let corners = RegionShape::Rect.polygon(rect);
        corners.iter().any(|p| inside(*p, &polygon))
            || polygon.iter().any(|p| {
                p[0] >= rect.x
                    && p[0] <= rect.x + rect.width
                    && p[1] >= rect.y
                    && p[1] <= rect.y + rect.height
            })
            || polygon
                .iter()
                .zip(polygon.iter().cycle().skip(1))
                .take(polygon.len())
                .any(|(a, b)| {
                    corners
                        .iter()
                        .zip(corners.iter().cycle().skip(1))
                        .take(4)
                        .any(|(c, d)| intersects(*a, *b, *c, *d))
                })
    }

    pub fn elements_in_area(&self) -> impl Iterator<Item = &TargetBounds> {
        self.area_bounds.iter().filter(|bound| {
            !self.has_targets() && !self.shape.is_rect() && self.overlaps_rect(bound.rect)
        })
    }
}
