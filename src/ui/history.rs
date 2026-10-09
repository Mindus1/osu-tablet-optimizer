use iced::widget::{canvas, column, pick_list, row, scrollable, space, text, text_input};
use iced::{Center, Element, Task};
use osu_tablet_optimizer::core::fit::Mapping;
use osu_tablet_optimizer::core::history::{self, History, Metric, Point};

use crate::app::{self, App};
use crate::ui::{charts, replays, setup};

const CHART_SIZE: (f32, f32) = (760.0, 320.0);
const NOTE: &str = "Each point is one analyzed replay, measured on its own: points scatter more \
                    than the Analysis page's numbers, and stopping at a circle's edge counts as a \
                    little undershoot. Analyze replays to add them.";

#[derive(Debug, Default)]
pub struct State {
    metric: Metric,
}

#[derive(Debug, Clone)]
pub enum Message {
    Metric(Metric),
    Rename(usize, String),
}

pub fn update(app: &mut App, message: Message) -> Task<app::Message> {
    match message {
        Message::Metric(metric) => app.history_page.metric = metric,
        Message::Rename(index, name) => {
            if let Some(snapshot) = app.history.snapshots.get_mut(index) {
                snapshot.name = name;
                app.save_history();
            }
        }
    }
    Task::none()
}

pub fn view(app: &App) -> Element<'_, app::Message> {
    let history = &app.history;
    if history.snapshots.is_empty() {
        return text(
            "No tablet area logged yet. Once OpenTabletDriver's settings are found (see Setup), \
             its area is logged here whenever it changes.",
        )
        .into();
    }
    let metric = app.history_page.metric;
    let points = history.points(metric);
    let areas = column(
        (0..history.snapshots.len())
            .rev()
            .map(Some)
            .chain([None])
            .filter_map(|area| area_row(history, area, &points, metric)),
    )
    .spacing(8);
    let chart: Element<'_, app::Message> = match (points.first(), points.last()) {
        (Some(first), Some(last)) => {
            let dates = row![
                text(replays::played_at(first.played_at_ms)).size(14),
                space::horizontal(),
                text(replays::played_at(last.played_at_ms)).size(14),
            ]
            .width(CHART_SIZE.0);
            let range = history::chart_range(&points, metric.ideal());
            let trend = charts::Trend {
                top: format_value(metric, range.1),
                bottom: format_value(metric, range.0),
                points,
                range,
                ideal: metric.ideal(),
            };
            column![
                canvas(trend).width(CHART_SIZE.0).height(CHART_SIZE.1),
                dates
            ]
            .spacing(4)
            .into()
        }
        _ => text("No analyzed replays yet.").into(),
    };

    scrollable(
        column![
            row![
                text("Show"),
                pick_list(Metric::ALL, Some(metric), |metric| {
                    app::Message::History(Message::Metric(metric))
                }),
            ]
            .spacing(8)
            .align_y(Center),
            text(NOTE).size(14),
            chart,
            text("Areas").size(20),
            areas,
        ]
        .spacing(16),
    )
    .into()
}

// A snapshot's name, or its size and rotation until it has one.
pub fn area_label(history: &History, area: Option<usize>) -> String {
    match area.and_then(|index| history.snapshots.get(index)) {
        Some(snapshot) if !snapshot.name.is_empty() => snapshot.name.clone(),
        Some(snapshot) => describe(&snapshot.mapping),
        None => "unknown".to_string(),
    }
}

// One line per area; the unknown area only when it has replays.
fn area_row<'a>(
    history: &'a History,
    area: Option<usize>,
    points: &[Point],
    metric: Metric,
) -> Option<Element<'a, app::Message>> {
    let average = history::average(points, area);
    let summary = match average {
        Some((replays, mean)) => format!("{replays} replays, mean {}", format_value(metric, mean)),
        None => "no analyzed replays".to_string(),
    };
    let swatch = text("●").color(charts::area_color(area));
    let Some(index) = area else {
        return average.map(|_| {
            row![
                swatch,
                text("Before the first logged area").width(460),
                text(summary)
            ]
            .spacing(12)
            .into()
        });
    };
    let snapshot = &history.snapshots[index];
    Some(
        row![
            swatch,
            text_input("Name this area", &snapshot.name)
                .on_input(move |name| app::Message::History(Message::Rename(index, name)))
                .width(180),
            text(describe(&snapshot.mapping)).width(160),
            text(format!("since {}", replays::played_at(snapshot.since_ms))).width(200),
            text(summary),
        ]
        .spacing(12)
        .align_y(Center)
        .into(),
    )
}

fn describe(mapping: &Mapping) -> String {
    format!(
        "{} × {} mm, {}°",
        setup::format_number(mapping.area_width),
        setup::format_number(mapping.area_height),
        setup::format_number(mapping.rotation)
    )
}

fn format_value(metric: Metric, value: f64) -> String {
    match metric {
        Metric::Overshoot => format!("{:+.1}%", value * 100.0),
        Metric::Rotation => format!("{value:+.2}°"),
        Metric::AxisRatio => format!("{value:.3}"),
        Metric::Error => format!("{value:.2} radii"),
    }
}
