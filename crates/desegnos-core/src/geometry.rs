use crate::Rect;
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct Point {
    pub x: f64,
    pub y: f64,
}

#[derive(Debug, Clone, Copy)]
pub(crate) enum Segment {
    Move(Point),
    Line(Point),
    Quad(Point, Point),
    Cubic(Point, Point, Point),
    Close,
}

#[derive(Debug, Clone, Default)]
pub(crate) struct Path {
    pub segments: Vec<Segment>,
}

impl Path {
    pub(crate) fn move_to(&mut self, point: Point) -> &mut Self {
        self.segments.push(Segment::Move(point));
        self
    }

    pub(crate) fn line_to(&mut self, point: Point) -> &mut Self {
        self.segments.push(Segment::Line(point));
        self
    }

    pub(crate) fn quad_to(&mut self, control: Point, point: Point) -> &mut Self {
        self.segments.push(Segment::Quad(control, point));
        self
    }

    pub(crate) fn cubic_to(&mut self, first: Point, second: Point, point: Point) -> &mut Self {
        self.segments.push(Segment::Cubic(first, second, point));
        self
    }

    pub(crate) fn close(&mut self) -> &mut Self {
        self.segments.push(Segment::Close);
        self
    }

    pub(crate) fn svg_data(&self) -> String {
        let mut data = String::new();
        for segment in &self.segments {
            match segment {
                Segment::Move(point) => data.push_str(&format!("M{:.3} {:.3}", point.x, point.y)),
                Segment::Line(point) => data.push_str(&format!("L{:.3} {:.3}", point.x, point.y)),
                Segment::Quad(control, point) => data.push_str(&format!(
                    "Q{:.3} {:.3} {:.3} {:.3}",
                    control.x, control.y, point.x, point.y
                )),
                Segment::Cubic(first, second, point) => data.push_str(&format!(
                    "C{:.3} {:.3} {:.3} {:.3} {:.3} {:.3}",
                    first.x, first.y, second.x, second.y, point.x, point.y
                )),
                Segment::Close => data.push('Z'),
            }
        }
        data
    }

    pub(crate) fn rect(rect: Rect, radius: f64) -> Self {
        let mut path = Path::default();
        let x = rect.x;
        let y = rect.y;
        let width = rect.width.max(0.0);
        let height = rect.height.max(0.0);
        let radius = clamp_radius(rect, radius);
        if radius <= 0.01 || width <= 0.02 || height <= 0.02 {
            path.move_to(Point { x, y })
                .line_to(Point { x: x + width, y })
                .line_to(Point {
                    x: x + width,
                    y: y + height,
                })
                .line_to(Point { x, y: y + height })
                .close();
            return path;
        }
        // Each corner is one quadratic curve through the sharp corner, matching the canvas's
        // CSS corner radius closely enough for exported output.
        path.move_to(Point { x: x + radius, y })
            .line_to(Point {
                x: x + width - radius,
                y,
            })
            .quad_to(
                Point { x: x + width, y },
                Point {
                    x: x + width,
                    y: y + radius,
                },
            )
            .line_to(Point {
                x: x + width,
                y: y + height - radius,
            })
            .quad_to(
                Point {
                    x: x + width,
                    y: y + height,
                },
                Point {
                    x: x + width - radius,
                    y: y + height,
                },
            )
            .line_to(Point {
                x: x + radius,
                y: y + height,
            })
            .quad_to(
                Point { x, y: y + height },
                Point {
                    x,
                    y: y + height - radius,
                },
            )
            .line_to(Point { x, y: y + radius })
            .quad_to(Point { x, y }, Point { x: x + radius, y });
        path.close();
        path
    }

    pub(crate) fn ellipse(rect: Rect) -> Self {
        const KAPPA: f64 = 0.552_284_749_831;
        let mut path = Path::default();
        let cx = rect.x + rect.width / 2.0;
        let cy = rect.y + rect.height / 2.0;
        let rx = rect.width / 2.0;
        let ry = rect.height / 2.0;
        let ox = rx * KAPPA;
        let oy = ry * KAPPA;
        path.move_to(Point { x: cx + rx, y: cy })
            .cubic_to(
                Point {
                    x: cx + rx,
                    y: cy + oy,
                },
                Point {
                    x: cx + ox,
                    y: cy + ry,
                },
                Point { x: cx, y: cy + ry },
            )
            .cubic_to(
                Point {
                    x: cx - ox,
                    y: cy + ry,
                },
                Point {
                    x: cx - rx,
                    y: cy + oy,
                },
                Point { x: cx - rx, y: cy },
            )
            .cubic_to(
                Point {
                    x: cx - rx,
                    y: cy - oy,
                },
                Point {
                    x: cx - ox,
                    y: cy - ry,
                },
                Point { x: cx, y: cy - ry },
            )
            .cubic_to(
                Point {
                    x: cx + ox,
                    y: cy - ry,
                },
                Point {
                    x: cx + rx,
                    y: cy - oy,
                },
                Point { x: cx + rx, y: cy },
            )
            .close();
        path
    }
}

pub(crate) fn clamp_radius(rect: Rect, radius: f64) -> f64 {
    radius
        .max(0.0)
        .min(rect.width.max(0.0) / 2.0)
        .min(rect.height.max(0.0) / 2.0)
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Rgba {
    pub r: u8,
    pub g: u8,
    pub b: u8,
    pub a: f64,
}

impl Rgba {
    pub(crate) const BLACK: Self = Self {
        r: 0,
        g: 0,
        b: 0,
        a: 1.0,
    };

    /// `#rgb`, `#rrggbb`, `#rrggbbaa` or `transparent`; the editor validates the same forms.
    pub(crate) fn parse(value: &str) -> Option<Self> {
        if value.eq_ignore_ascii_case("transparent") {
            return Some(Self {
                a: 0.0,
                ..Self::BLACK
            });
        }
        let hex = value.strip_prefix('#')?;
        let digit = |index: usize| -> Option<u8> {
            let byte = *hex.as_bytes().get(index)?;
            (byte as char).to_digit(16).map(|value| value as u8)
        };
        let (r, g, b, a) = match hex.len() {
            3 => (digit(0)? * 17, digit(1)? * 17, digit(2)? * 17, 1.0),
            6 => (
                digit(0)? * 16 + digit(1)?,
                digit(2)? * 16 + digit(3)?,
                digit(4)? * 16 + digit(5)?,
                1.0,
            ),
            8 => (
                digit(0)? * 16 + digit(1)?,
                digit(2)? * 16 + digit(3)?,
                digit(4)? * 16 + digit(5)?,
                f64::from(digit(6)? * 16 + digit(7)?) / 255.0,
            ),
            _ => return None,
        };
        Some(Self { r, g, b, a })
    }

    pub fn svg_colour(self) -> String {
        if self.a >= 1.0 {
            format!("#{:02x}{:02x}{:02x}", self.r, self.g, self.b)
        } else {
            format!("rgba({},{},{},{:.3})", self.r, self.g, self.b, self.a)
        }
    }

    pub fn components(self) -> (f64, f64, f64) {
        (
            f64::from(self.r) / 255.0,
            f64::from(self.g) / 255.0,
            f64::from(self.b) / 255.0,
        )
    }
}
