use std::collections::HashSet;
use std::path::PathBuf;
use std::sync::Arc;

use iced::widget::{button, canvas, column, row, scrollable, space, text};
use iced::{Center, Element, Task};
use osu_tablet_optimizer::core::aim::{self, Report};
use osu_tablet_optimizer::core::fit::{Estimate, Mapping, Settings};

use crate::app::{self, App};
use crate::ui::{charts, setup};

// Stands in for OpenTabletDriver's mapping when none is set up. Overshoot, rotation and the axis
// ratio don't depend on it; shear and the area suggestion do.
const NEUTRAL_MAPPING: Mapping = Mapping {
    area_width: 100.0,
    area_height: 56.25,
    rotation: 0.0,
    display_width: 1920.0,
    display_height: 1080.0,
};
const CHART_SIZE: f32 = 360.0;

#[derive(Debug, Default)]
pub struct State {
    running: bool,
    // Analyze was pressed during a run; run again once it ends.
    queued: bool,
    pub last: Option<Arc<Analysis>>,
}

#[derive(Debug)]
pub struct Analysis {
    selection: HashSet<PathBuf>,
    // OpenTabletDriver's mapping the fit ran against; None when the neutral one stood in.
    pub otd_mapping: Option<Mapping>,
    pub settings: Settings,
    pub report: Report,
}

#[derive(Debug, Clone)]
pub enum Message {
    Run,
    Done(Arc<Analysis>),
    Failed,
}

pub fn update(app: &mut App, message: Message) -> Task<app::Message> {
    match message {
        Message::Run => return start(app),
        Message::Done(analysis) => app.analysis.last = Some(analysis),
        Message::Failed => {
            app.notice = Some("The analysis failed unexpectedly.".to_string());
        }
    }
    app.analysis.running = false;
    if std::mem::take(&mut app.analysis.queued) {
        return start(app);
    }
    Task::none()
}

// Reads and fits the selected replays on a worker thread.
pub fn start(app: &mut App) -> Task<app::Message> {
    if app.replays.selected.is_empty() {
        return Task::none();
    }
    if app.analysis.running {
        app.analysis.queued = true;
        return Task::none();
    }
    app.analysis.running = true;
    let library = Arc::clone(&app.library);
    let selection = app.replays.selected.clone();
    let otd_mapping = otd_mapping(app);
    let settings = app.config.fit;
    let (sender, receiver) = iced::futures::channel::oneshot::channel();
    std::thread::spawn(move || {
        let paths: Vec<PathBuf> = selection.iter().cloned().collect();
        let mapping = otd_mapping.unwrap_or(NEUTRAL_MAPPING);
        let report = aim::analyze(&paths, &library.beatmaps_by_md5(), mapping, &settings);
        let _ = sender.send(Arc::new(Analysis {
            selection,
            otd_mapping,
            settings,
            report,
        }));
    });
    Task::perform(receiver, |analysis| {
        app::Message::Analysis(analysis.map_or(Message::Failed, Message::Done))
    })
}

fn otd_mapping(app: &App) -> Option<Mapping> {
    setup::profile(app).map(|profile| profile.mapping)
}

fn is_outdated(analysis: &Analysis, app: &App) -> bool {
    analysis.selection != app.replays.selected
        || analysis.otd_mapping != otd_mapping(app)
        || analysis.settings != app.config.fit
}

pub fn view(app: &App) -> Element<'_, app::Message> {
    let state = &app.analysis;
    let again = button("Analyze selected again").on_press_maybe(
        (!state.running && !app.replays.selected.is_empty())
            .then_some(app::Message::Analysis(Message::Run)),
    );
    let Some(analysis) = &state.last else {
        let hint = if state.running {
            "Analyzing…"
        } else {
            "Select replays on the Replays page, then press Analyze."
        };
        return column![text(hint), again].spacing(12).into();
    };

    let report = &analysis.report;
    let summary = text(format!(
        "{} replays analyzed, {} skipped, {} jumps used.{}",
        report.analyzed,
        report.skipped.len(),
        report.errors.len(),
        if state.running {
            " Analyzing again…"
        } else {
            ""
        }
    ));
    let numbers: Element<'_, app::Message> = match &report.suggestion {
        Some(suggestion) => row![
            metric(
                "Overshoot",
                format!("{:+.1}%", suggestion.overshoot.value * 100.0),
                format!(
                    "90%: {:+.2}% to {:+.2}%",
                    suggestion.overshoot.low * 100.0,
                    suggestion.overshoot.high * 100.0
                ),
                verdict(
                    suggestion.overshoot,
                    0.0,
                    "jumps land long",
                    "jumps land short"
                ),
            ),
            metric(
                "Rotation",
                format!("{:+.2}°", suggestion.rotation_bias.value),
                format!(
                    "90%: {:+.2}° to {:+.2}°",
                    suggestion.rotation_bias.low, suggestion.rotation_bias.high
                ),
                verdict(
                    suggestion.rotation_bias,
                    0.0,
                    "moves turn clockwise",
                    "moves turn counter-clockwise",
                ),
            ),
            metric(
                "Horizontal ÷ vertical",
                format!("{:.3}", suggestion.axis_ratio.value),
                format!(
                    "90%: {:.3} to {:.3}",
                    suggestion.axis_ratio.low, suggestion.axis_ratio.high
                ),
                verdict(
                    suggestion.axis_ratio,
                    1.0,
                    "horizontal moves run longer",
                    "horizontal moves run shorter",
                ),
            ),
            metric(
                "Shear",
                format!("{:+.2}°", suggestion.shear),
                String::new(),
                "skew OpenTabletDriver can't correct".to_string(),
            ),
        ]
        .spacing(32)
        .into(),
        None => {
            text("Not enough data for numbers: select at least 5 replays with jumps (50 usable).")
                .into()
        }
    };
    let charts = row![
        chart(
            "Each jump's error, along and across the move (circle = hit)",
            canvas(charts::Scatter {
                errors: &report.errors
            })
        ),
        chart(
            "Distance and turn by move direction (ring = perfect, magnified 5×)",
            canvas(charts::Polar {
                sectors: &report.sectors
            })
        ),
    ]
    .spacing(32);
    let skipped = column(report.skipped.iter().take(20).map(|(path, reason)| {
        let name = path.file_name().unwrap_or_default().to_string_lossy();
        text(format!("Skipped {name}: {reason}")).size(14).into()
    }))
    .spacing(4);

    let mut page = column![row![summary, space::horizontal(), again].align_y(Center)].spacing(24);
    if !state.running && is_outdated(analysis, app) {
        page = page.push(text(
            "The selection or setup changed since this analysis. Analyze again to update.",
        ));
    }
    scrollable(page.push(numbers).push(charts).push(skipped)).into()
}

fn metric<'a>(
    title: &'a str,
    value: String,
    interval: String,
    verdict: String,
) -> Element<'a, app::Message> {
    column![
        text(title).size(14),
        text(value).size(28),
        text(interval).size(14),
        text(verdict).size(14)
    ]
    .spacing(2)
    .into()
}

fn chart<'a>(
    title: &'a str,
    canvas: iced::widget::Canvas<impl canvas::Program<app::Message> + 'a, app::Message>,
) -> Element<'a, app::Message> {
    column![
        text(title).size(14),
        canvas.width(CHART_SIZE).height(CHART_SIZE)
    ]
    .spacing(8)
    .into()
}

fn verdict(estimate: Estimate, neutral: f64, above: &str, below: &str) -> String {
    if estimate.contains(neutral) {
        "no clear effect".to_string()
    } else if estimate.value > neutral {
        above.to_string()
    } else {
        below.to_string()
    }
}
