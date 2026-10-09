use iced::widget::canvas::{self, Frame, Geometry, Path, Stroke, Text};
use iced::{Color, Point, Rectangle, Renderer, Size, Theme, mouse};
use osu_tablet_optimizer::core::aim::{self, Sector};

// Errors within this many radii are drawn; the circle marks one radius (a hit).
const SCATTER_RANGE: f32 = 3.0;
// Gain and turn differences are a few percent or degrees; magnify them so the shape shows.
const MAGNIFICATION: f64 = 5.0;
// Largest outline radius in rings; the ring is sized so that it still fits the chart.
const MAX_SCALE: f64 = 1.4;

pub struct Scatter<'a> {
    pub errors: &'a [(f64, f64)],
}

pub struct Polar<'a> {
    pub sectors: &'a [Sector],
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

fn label(frame: &mut Frame, position: Point, content: &str, theme: &Theme) {
    frame.fill_text(Text {
        content: content.to_string(),
        position,
        color: theme.extended_palette().background.base.text,
        size: 14.0.into(),
        ..Text::default()
    });
}
