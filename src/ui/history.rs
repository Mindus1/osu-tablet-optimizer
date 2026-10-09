use iced::widget::{canvas, column, pick_list, row, space, text, text_input};
use iced::{Center, Element, Fill, Task};
use osu_tablet_optimizer::core::fit::Mapping;
use osu_tablet_optimizer::core::history::{self, History, Metric, Point};

use crate::app::{self, App};
use crate::ui::{charts, replays, setup, style};

const CHART_HEIGHT: f32 = 320.0;
const SUMMARY_WIDTH: f32 = 200.0;
const NOTE: &str = "Each point is one replay measured on its own, so points scatter more than the \
                    Analysis page's numbers, and stopping at a circle's edge counts as a little \
                    undershoot.";

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
    const ABOUT: &str = "Whether a new area helped: every analyzed replay, colored by its area.";
    let history = &app.history;
    if history.snapshots.is_empty() {
        return style::page(
            "History",
            ABOUT,
            row![],
            style::panel(style::muted(
                "No tablet area logged yet. Once OpenTabletDriver's settings are found (see \
                 Setup), its area is logged here whenever it changes.",
            )),
        );
    }
    let metric = app.history_page.metric;
    let points = history.points(metric);
    let areas = column(
        (0..history.snapshots.len())
            .rev()
            .map(Some)
            .chain([None])
            .filter_map(|area| area_row(history, area, &points, metric))
            .enumerate()
            .flat_map(|(index, row)| {
                let divider = (index > 0).then(|| style::divider().into());
                divider.into_iter().chain([row])
            }),
    )
    .spacing(12);
    let chart: Element<'_, app::Message> = match (points.first(), points.last()) {
        (Some(first), Some(last)) => {
            let dates = row![
                style::muted(replays::played_at(first.played_at_ms)),
                space::horizontal(),
                style::muted(replays::played_at(last.played_at_ms)),
            ];
            let range = history::chart_range(&points, metric.ideal());
            let trend = charts::Trend {
                top: format_value(metric, range.1),
                bottom: format_value(metric, range.0),
                points,
                range,
                ideal: metric.ideal(),
            };
            style::panel(
                column![
                    canvas(trend).width(Fill).height(CHART_HEIGHT),
                    dates,
                    style::muted(NOTE),
                ]
                .spacing(10),
            )
            .into()
        }
        _ => style::panel(style::muted(
            "No analyzed replays yet. Each replay you analyze adds a point here.",
        ))
        .into(),
    };
    let pick = pick_list(Metric::ALL, Some(metric), |metric| {
        app::Message::History(Message::Metric(metric))
    })
    .padding([8, 12])
    .style(style::select);

    style::scroll(style::page(
        "History",
        ABOUT,
        pick,
        column![chart, style::titled("Areas", areas)].spacing(16),
    ))
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
        None => "No analyzed replays".to_string(),
    };
    let swatch = style::dot(charts::area_color(area), 10.0);
    let Some(index) = area else {
        return average.map(|_| {
            row![
                swatch,
                text("Before the first logged area").width(Fill),
                style::muted(summary).width(SUMMARY_WIDTH),
            ]
            .spacing(16)
            .align_y(Center)
            .into()
        });
    };
    let snapshot = &history.snapshots[index];
    Some(
        row![
            swatch,
            text_input("Name this area", &snapshot.name)
                .on_input(move |name| app::Message::History(Message::Rename(index, name)))
                .padding([6, 10])
                .width(200)
                .style(style::input),
            text(describe(&snapshot.mapping)).width(Fill),
            style::muted(format!("since {}", replays::played_at(snapshot.since_ms))).width(150),
            style::muted(summary).width(SUMMARY_WIDTH),
        ]
        .spacing(16)
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
