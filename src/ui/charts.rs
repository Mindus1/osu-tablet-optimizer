use iced::widget::canvas::{self, Frame, Geometry, LineDash, Path, Stroke, Text};
use iced::{Color, Point, Rectangle, Renderer, Size, Theme, mouse};
use osu_tablet_optimizer::core::aim::{self, Sector};
use osu_tablet_optimizer::core::history;

// Errors within this many radii are drawn; the circle marks one radius (a hit).
const SCATTER_RANGE: f32 = 3.0;
// Gain and turn differences are a few percent or degrees; magnify them so the shape shows.
const MAGNIFICATION: f64 = 5.0;
// Largest outline radius in rings; the ring is sized so that it still fits the chart.
const MAX_SCALE: f64 = 1.4;
// Points and the area list share these, one per area in the order the areas were logged.
const AREA_COLORS: [Color; 6] = [
    Color::from_rgb(0.40, 0.65, 1.00),
    Color::from_rgb(1.00, 0.60, 0.25),
    Color::from_rgb(0.45, 0.85, 0.45),
    Color::from_rgb(0.95, 0.40, 0.55),
    Color::from_rgb(0.75, 0.55, 1.00),
    Color::from_rgb(0.95, 0.85, 0.35),
];
const UNKNOWN_AREA: Color = Color::from_rgb(0.5, 0.5, 0.5);
// Room on the left for the value labels, and around the points so edge ones show whole.
const TREND_GUTTER: f32 = 64.0;
const TREND_INSET: f32 = 4.0;

pub struct Scatter<'a> {
    pub errors: &'a [(f64, f64)],
}

pub struct Polar<'a> {
    pub sectors: &'a [Sector],
}

// Corners in tablet mm; the tablet is drawn when its size is known.
pub struct AreaPreview {
    pub tablet: Option<(f64, f64)>,
    pub current: [(f64, f64); 4],
    pub full: [(f64, f64); 4],
    pub capped: [(f64, f64); 4],
}

// One value per replay over time; values outside `range` sit on its edge.
pub struct Trend {
    pub points: Vec<history::Point>,
    pub range: (f64, f64),
    pub ideal: f64,
    // Labels for the top and bottom of the range.
    pub top: String,
    pub bottom: String,
}

pub fn area_color(area: Option<usize>) -> Color {
    area.map_or(UNKNOWN_AREA, |index| AREA_COLORS[index % AREA_COLORS.len()])
}

impl<Message> canvas::Program<Message> for Scatter<'_> {
    type State = ();

    fn draw(
        &self,
        _state: &(),
        renderer: &Renderer,
        theme: &Theme,
        bounds: Rectangle,
        _cursor: mouse::Cursor,
    ) -> Vec<Geometry> {
        let mut frame = Frame::new(renderer, bounds.size());
        let palette = theme.extended_palette();
        let center = frame.center();
        let scale = bounds.width.min(bounds.height) / 2.0 / SCATTER_RANGE;
        let axis = Stroke::default()
            .with_color(palette.background.strong.color)
            .with_width(1.0);
        frame.stroke(
            &Path::line(
                Point::new(0.0, center.y),
                Point::new(bounds.width, center.y),
            ),
            axis,
        );
        frame.stroke(
            &Path::line(
                Point::new(center.x, 0.0),
                Point::new(center.x, bounds.height),
            ),
            axis,
        );
        let dot = Color {
            a: 0.3,
            ..palette.background.base.text
        };
        for &(along, across) in self.errors {
            let (x, y) = (along as f32, across as f32);
            if x.abs() <= SCATTER_RANGE && y.abs() <= SCATTER_RANGE {
                let point = Point::new(center.x + x * scale - 1.0, center.y + y * scale - 1.0);
                frame.fill_rectangle(point, Size::new(2.0, 2.0), dot);
            }
        }
        frame.stroke(
            &Path::circle(center, scale),
            Stroke::default()
                .with_color(palette.primary.base.color)
                .with_width(1.5),
        );
        label(
            &mut frame,
            Point::new(4.0, center.y - 18.0),
            "undershoot",
            theme,
        );
        label(
            &mut frame,
            Point::new(bounds.width - 76.0, center.y - 18.0),
            "overshoot",
            theme,
        );
        vec![frame.into_geometry()]
    }
}

impl<Message> canvas::Program<Message> for Polar<'_> {
    type State = ();

    fn draw(
        &self,
        _state: &(),
        renderer: &Renderer,
        theme: &Theme,
        bounds: Rectangle,
        _cursor: mouse::Cursor,
    ) -> Vec<Geometry> {
        let mut frame = Frame::new(renderer, bounds.size());
        let palette = theme.extended_palette();
        let center = frame.center();
        let ring = bounds.width.min(bounds.height) / 2.0 / MAX_SCALE as f32;
        frame.stroke(
            &Path::circle(center, ring),
            Stroke::default()
                .with_color(palette.background.strong.color)
                .with_width(1.0),
        );
        let stroke = Stroke::default()
            .with_color(palette.primary.base.color)
            .with_width(2.0);
        for run in aim::outline_runs(self.sectors) {
            let closed = run.len() == self.sectors.len();
            let points: Vec<Point> = run
                .iter()
                .map(|sector| {
                    let angle = (sector.direction + sector.turn * MAGNIFICATION).to_radians();
                    let scale = (1.0 + (sector.gain - 1.0) * MAGNIFICATION).clamp(0.0, MAX_SCALE);
                    let radius = ring * scale as f32;
                    Point::new(
                        center.x + radius * angle.cos() as f32,
                        center.y + radius * angle.sin() as f32,
                    )
                })
                .collect();
            let outline = Path::new(|builder| {
                for (index, point) in points.iter().enumerate() {
                    if index == 0 {
                        builder.move_to(*point);
                    } else {
                        builder.line_to(*point);
                    }
                }
                if closed {
                    builder.close();
                }
            });
            frame.stroke(&outline, stroke);
        }
        vec![frame.into_geometry()]
    }
}

impl<Message> canvas::Program<Message> for AreaPreview {
    type State = ();

    fn draw(
        &self,
        _state: &(),
        renderer: &Renderer,
        theme: &Theme,
        bounds: Rectangle,
        _cursor: mouse::Cursor,
    ) -> Vec<Geometry> {
        let mut frame = Frame::new(renderer, bounds.size());
        let palette = theme.extended_palette();
        let mut points = [self.current, self.full, self.capped].concat();
        if let Some(size) = self.tablet {
            points.extend([(0.0, 0.0), size]);
        }
        let (min_x, max_x, min_y, max_y) = points.iter().fold(
            (
                f64::INFINITY,
                f64::NEG_INFINITY,
                f64::INFINITY,
                f64::NEG_INFINITY,
            ),
            |(min_x, max_x, min_y, max_y), &(x, y)| {
                (min_x.min(x), max_x.max(x), min_y.min(y), max_y.max(y))
            },
        );
        let scale = (f64::from(bounds.width) / (max_x - min_x))
            .min(f64::from(bounds.height) / (max_y - min_y));
        let offset = (
            (f64::from(bounds.width) - (max_x - min_x) * scale) / 2.0,
            (f64::from(bounds.height) - (max_y - min_y) * scale) / 2.0,
        );
        let to_point = |(x, y): (f64, f64)| {
            Point::new(
                (offset.0 + (x - min_x) * scale) as f32,
                (offset.1 + (y - min_y) * scale) as f32,
            )
        };

        if let Some((width, height)) = self.tablet {
            frame.fill_rectangle(
                to_point((0.0, 0.0)),
                Size::new((width * scale) as f32, (height * scale) as f32),
                palette.background.weak.color,
            );
        }
        let outline = |corners: &[(f64, f64); 4]| {
            Path::new(|builder| {
                builder.move_to(to_point(corners[0]));
                for &corner in &corners[1..] {
                    builder.line_to(to_point(corner));
                }
                builder.close();
            })
        };
        let primary = Stroke::default()
            .with_color(palette.primary.base.color)
            .with_width(2.0);
        frame.stroke(
            &outline(&self.current),
            Stroke::default()
                .with_color(palette.background.base.text)
                .with_width(1.0),
        );
        frame.stroke(
            &outline(&self.full),
            Stroke {
                line_dash: LineDash {
                    segments: &[6.0, 4.0],
                    offset: 0,
                },
                ..primary
            },
        );
        frame.stroke(&outline(&self.capped), primary);
        vec![frame.into_geometry()]
    }
}

impl<Message> canvas::Program<Message> for Trend {
    type State = ();

    fn draw(
        &self,
        _state: &(),
        renderer: &Renderer,
        theme: &Theme,
        bounds: Rectangle,
        _cursor: mouse::Cursor,
    ) -> Vec<Geometry> {
        let mut frame = Frame::new(renderer, bounds.size());
        let (low, high) = self.range;
        let height = bounds.height - 2.0 * TREND_INSET;
        let y = |value: f64| {
            TREND_INSET + ((high - value.clamp(low, high)) / (high - low)) as f32 * height
        };
        let first = self.points.first().map_or(0, |point| point.played_at_ms);
        let last = self.points.last().map_or(0, |point| point.played_at_ms);
        let span = (last - first).max(1) as f32;
        let width = bounds.width - TREND_GUTTER - TREND_INSET;
        let x = |ms: i64| TREND_GUTTER + (ms - first) as f32 / span * width;

        frame.stroke(
            &Path::line(
                Point::new(TREND_GUTTER, y(self.ideal)),
                Point::new(bounds.width, y(self.ideal)),
            ),
            Stroke::default()
                .with_color(theme.extended_palette().background.strong.color)
                .with_width(1.0),
        );
        for point in &self.points {
            frame.fill_rectangle(
                Point::new(x(point.played_at_ms) - 2.0, y(point.value) - 2.0),
                Size::new(4.0, 4.0),
                area_color(point.snapshot),
            );
        }
        label(&mut frame, Point::new(0.0, 0.0), &self.top, theme);
        label(
            &mut frame,
            Point::new(0.0, bounds.height - 16.0),
            &self.bottom,
            theme,
        );
        vec![frame.into_geometry()]
    }
}

fn label(frame: &mut Frame, position: Point, content: &str, theme: &Theme) {
    frame.fill_text(Text {
        content: content.to_string(),
        position,
        color: theme.extended_palette().background.base.text,
        size: 14.0.into(),
        ..Text::default()
    });
}
