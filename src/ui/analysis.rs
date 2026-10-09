use std::collections::HashSet;
use std::path::PathBuf;
use std::sync::Arc;

use iced::widget::{canvas, column, row, text};
use iced::{Center, Color, Element, Fill, FillPortion, Shrink, Task};
use osu_tablet_optimizer::core::aim::{self, Report};
use osu_tablet_optimizer::core::fit::{Estimate, Mapping, Settings};

use crate::app::{self, App};
use crate::ui::{area, charts, setup, style};

// Stands in for OpenTabletDriver's mapping when none is set up. Overshoot, rotation and the axis
// ratio don't depend on it; shear and the area suggestion do.
const NEUTRAL_MAPPING: Mapping = Mapping {
    area_width: 100.0,
    area_height: 56.25,
    rotation: 0.0,
    display_width: 1920.0,
    display_height: 1080.0,
};
const CHART_HEIGHT: f32 = 340.0;
const MAX_SKIPPED: usize = 20;

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
        Message::Done(analysis) => {
            app.history.record(&analysis.report.measured);
            app.save_history();
            app.analysis.last = Some(analysis);
            // The last export message belongs to the previous analysis.
            app.area = area::State::default();
        }
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
    let Some(analysis) = &state.last else {
        let (about, content): (&str, Element<'_, app::Message>) = if state.running {
            (
                "Reading your replays and fitting your aim…",
                style::panel(style::muted(format!(
                    "Analyzing {} replays.",
                    app.replays.selected.len()
                )))
                .into(),
            )
        } else {
            (
                "How your jumps land compared to where the circles are.",
                style::panel(
                    column![
                        style::heading("No analysis yet"),
                        style::muted("Select replays on the Replays page, then press Analyze."),
                        style::primary("Go to Replays")
                            .on_press(app::Message::Navigate(app::Page::Replays)),
                    ]
                    .spacing(12),
                )
                .into(),
            )
        };
        return style::page("Analysis", about, row![], content);
    };

    let report = &analysis.report;
    let summary = format!(
        "{} replays read and {} jumps measured{}.",
        report.analyzed,
        report.errors.len(),
        if state.running {
            "; analyzing again…"
        } else {
            ""
        }
    );
    let outdated = !state.running && is_outdated(analysis, app);
    let again_label = format!("Analyze {} selected", app.replays.selected.len());
    let again = if outdated {
        style::primary(again_label)
    } else {
        style::secondary(again_label)
    }
    .on_press_maybe(
        (!state.running && !app.replays.selected.is_empty())
            .then_some(app::Message::Analysis(Message::Run)),
    );

    let numbers: Element<'_, app::Message> = match &report.suggestion {
        Some(suggestion) => style::panel(
            row![
                metric(
                    "Overshoot",
                    format!("{:+.1}%", suggestion.overshoot.value * 100.0),
                    format!(
                        "90% range {:+.1}% to {:+.1}%",
                        suggestion.overshoot.low * 100.0,
                        suggestion.overshoot.high * 100.0
                    ),
                    verdict(
                        suggestion.overshoot,
                        0.0,
                        "Jumps land long",
                        "Jumps land short"
                    ),
                ),
                style::separator(),
                metric(
                    "Rotation",
                    format!("{:+.2}°", suggestion.rotation_bias.value),
                    format!(
                        "90% range {:+.2}° to {:+.2}°",
                        suggestion.rotation_bias.low, suggestion.rotation_bias.high
                    ),
                    verdict(
                        suggestion.rotation_bias,
                        0.0,
                        "Moves turn clockwise",
                        "Moves turn counter-clockwise",
                    ),
                ),
                style::separator(),
                metric(
                    "Horizontal ÷ vertical",
                    format!("{:.3}", suggestion.axis_ratio.value),
                    format!(
                        "90% range {:.3} to {:.3}",
                        suggestion.axis_ratio.low, suggestion.axis_ratio.high
                    ),
                    verdict(
                        suggestion.axis_ratio,
                        1.0,
                        "Horizontal moves run longer",
                        "Horizontal moves run shorter",
                    ),
                ),
                style::separator(),
                metric(
                    "Shear",
                    format!("{:+.2}°", suggestion.shear),
                    "No range: shown for information".to_string(),
                    (
                        style::MUTED,
                        "Skew OpenTabletDriver can't correct".to_string(),
                    ),
                ),
            ]
            .spacing(24)
            .height(Shrink),
        )
        .into(),
        None => style::banner(
            style::MEH,
            text("Not enough data for numbers: select at least 5 replays with jumps (50 usable)."),
        )
        .into(),
    };
    let charts = row![
        chart(
            "Jump errors",
            "Where each jump landed, along and across the move. The pink circle is a hit.",
            canvas(charts::Scatter {
                errors: &report.errors
            })
        ),
        chart(
            "By direction",
            "Distance and turn per move direction, magnified 5×. The ring is perfect aim.",
            canvas(charts::Polar {
                sectors: &report.sectors
            })
        ),
    ]
    .spacing(16);

    let mut content = column![].spacing(16);
    if outdated {
        content = content.push(style::banner(
            style::MEH,
            text("The selection or setup changed since this analysis. Analyze again to update it."),
        ));
    }
    content = content.push(numbers).push(charts);
    if !report.skipped.is_empty() {
        content = content.push(skipped(&report.skipped));
    }
    style::scroll(style::page("Analysis", summary, again, content)).into()
}

fn metric<'a>(
    title: &'a str,
    value: String,
    range: String,
    (tone, verdict): (Color, String),
) -> Element<'a, app::Message> {
    column![
        style::muted(title),
        text(value).size(32).font(style::SEMIBOLD),
        style::muted(range),
        row![style::dot(tone, 8.0), text(verdict).size(13)]
            .spacing(8)
            .align_y(Center),
    ]
    .spacing(6)
    .width(Fill)
    .into()
}

fn chart<'a>(
    title: &'a str,
    about: &'a str,
    canvas: iced::widget::Canvas<impl canvas::Program<app::Message> + 'a, app::Message>,
) -> Element<'a, app::Message> {
    style::panel(
        column![
            style::heading(title),
            style::muted(about),
            canvas.width(Fill).height(CHART_HEIGHT)
        ]
        .spacing(8),
    )
    .into()
}

fn skipped(skipped: &[(PathBuf, String)]) -> Element<'_, app::Message> {
    let mut lines = column(skipped.iter().take(MAX_SKIPPED).map(|(path, reason)| {
        let name = path.file_name().unwrap_or_default().to_string_lossy();
        row![
            text(name).size(13).width(FillPortion(2)),
            style::muted(reason.as_str()).width(FillPortion(3)),
        ]
        .spacing(16)
        .into()
    }))
    .spacing(6);
    if skipped.len() > MAX_SKIPPED {
        lines = lines.push(style::muted(format!(
            "…and {} more.",
            skipped.len() - MAX_SKIPPED
        )));
    }
    style::titled("Skipped replays", lines).into()
}

// A clear effect is worth correcting (osu!'s 50 color); none is fine (its 300 color).
fn verdict(estimate: Estimate, neutral: f64, above: &str, below: &str) -> (Color, String) {
    if estimate.contains(neutral) {
        (style::GREAT, "No clear effect".to_string())
    } else if estimate.value > neutral {
        (style::MEH, above.to_string())
    } else {
        (style::MEH, below.to_string())
    }
}
