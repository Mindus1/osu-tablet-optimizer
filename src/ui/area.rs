use iced::widget::{button, canvas, column, row, text};
use iced::{Element, Task};
use osu_tablet_optimizer::core::fit::{Mapping, Suggestion};
use osu_tablet_optimizer::core::otd::{self, Profile};

use crate::app::{self, App};
use crate::ui::{charts, setup};

const PREVIEW_SIZE: (f32, f32) = (400.0, 250.0);
const COLUMN_WIDTH: f32 = 150.0;

#[derive(Debug, Default)]
pub struct State {
    // Outcome of the last export.
    exported: Option<String>,
}

#[derive(Debug, Clone, Copy)]
pub enum Message {
    Export(Step),
}

#[derive(Debug, Clone, Copy)]
pub enum Step {
    Capped,
    Full,
}

// A suggestion that applies to the selected OTD profile as it is now.
struct Proposal<'a> {
    profile: &'a Profile,
    suggestion: &'a Suggestion,
}

pub fn update(app: &mut App, message: Message) -> Task<app::Message> {
    let Message::Export(step) = message;
    let (Ok(proposal), Some(data_dir)) = (proposal(app), app.config.otd_dir()) else {
        return Task::none();
    };
    let (mapping, label) = match step {
        Step::Capped => (proposal.suggestion.capped, "capped"),
        Step::Full => (proposal.suggestion.full, "full"),
    };
    let base = format!("oto-{}-{label}", chrono::Local::now().format("%Y-%m-%d"));
    let exported = otd::export_preset(
        &data_dir,
        app.config.profile,
        &proposal.profile.mapping,
        mapping,
        setup::tablet_size(app),
        &base,
    );
    app.area.exported = Some(match exported {
        Ok(path) => format!(
            "Saved preset {}. In OpenTabletDriver, choose it under File → Presets \
             (Refresh presets first if it's running).",
            path.file_stem().unwrap_or_default().to_string_lossy()
        ),
        Err(error) => format!("{error:#}"),
    });
    // A refused export usually means OTD's settings changed; show them as they are now.
    app.reload_otd();
    Task::none()
}

pub fn view(app: &App) -> Element<'_, app::Message> {
    let proposal = match proposal(app) {
        Ok(proposal) => proposal,
        Err(reason) => return text(reason).into(),
    };
    let (profile, suggestion) = (proposal.profile, proposal.suggestion);
    let tablet_size = setup::tablet_size(app);
    let center = |mapping: &Mapping| otd::preset_center(profile, mapping, tablet_size);
    let preview = canvas(charts::AreaPreview {
        tablet: tablet_size,
        current: otd::corners(profile.center, &profile.mapping),
        full: otd::corners(center(&suggestion.full), &suggestion.full),
        capped: otd::corners(center(&suggestion.capped), &suggestion.capped),
    })
    .width(PREVIEW_SIZE.0)
    .height(PREVIEW_SIZE.1);
    let mut legend = "Thin: current area. Solid: capped step. Dashed: full fit.".to_string();
    if tablet_size.is_none() {
        legend.push_str(" Enter the tablet size in Setup to see the tablet.");
    }

    let current = (profile.mapping, profile.center);
    let capped = (suggestion.capped, center(&suggestion.capped));
    let full = (suggestion.full, center(&suggestion.full));
    let table = column![
        table_row("", "Current", "Capped step", "Full fit"),
        table_row(
            "Width",
            &millimeters(current.0.area_width, None),
            &millimeters(capped.0.area_width, Some(current.0.area_width)),
            &millimeters(full.0.area_width, Some(current.0.area_width)),
        ),
        table_row(
            "Height",
            &millimeters(current.0.area_height, None),
            &millimeters(capped.0.area_height, Some(current.0.area_height)),
            &millimeters(full.0.area_height, Some(current.0.area_height)),
        ),
        table_row(
            "Rotation",
            &format!("{:.2}°", current.0.rotation),
            &format!("{:.2}°", capped.0.rotation),
            &format!("{:.2}°", full.0.rotation),
        ),
        table_row(
            "Center",
            &position(current.1),
            &position(capped.1),
            &position(full.1),
        ),
    ]
    .spacing(6);

    let kept: Vec<&str> = [
        ("size", suggestion.size.contains(1.0)),
        ("aspect ratio", suggestion.aspect.contains(1.0)),
        (
            "rotation",
            suggestion.rotation.contains(profile.mapping.rotation),
        ),
    ]
    .into_iter()
    .filter_map(|(name, kept)| kept.then_some(name))
    .collect();
    let mut notes = column![].spacing(6);
    if !kept.is_empty() {
        notes = notes.push(text(format!(
            "Kept, because the analysis shows no clear effect: {}.",
            kept.join(", ")
        )));
    }
    let settings = app.config.fit;
    notes = notes.push(text(format!(
        "The capped step changes the size by at most {}% and the rotation by at most {}°. \
         Play a few sessions with it, then analyze again.",
        settings.max_size_step * 100.0,
        settings.max_rotation_step_deg
    )));

    let actions: Element<'_, app::Message> = if suggestion.full == profile.mapping {
        text("Your area fits your aim: nothing to change.").into()
    } else {
        row![
            button("Export capped step")
                .on_press(app::Message::Area(Message::Export(Step::Capped))),
            button("Export full fit").on_press(app::Message::Area(Message::Export(Step::Full))),
        ]
        .spacing(12)
        .into()
    };
    let mut details = column![table, notes, actions].spacing(20);
    if let Some(exported) = &app.area.exported {
        details = details.push(text(exported));
    }

    row![
        column![preview, text(legend).size(14)]
            .spacing(8)
            .width(PREVIEW_SIZE.0),
        details
    ]
    .spacing(32)
    .into()
}

fn proposal(app: &App) -> Result<Proposal<'_>, &'static str> {
    let analysis = app.analysis.last.as_ref().ok_or(
        "Select replays on the Replays page and press Analyze; the suggested area shows up here.",
    )?;
    let suggestion =
        analysis.report.suggestion.as_ref().ok_or(
            "The last analysis had too little data for a suggestion; see the Analysis page.",
        )?;
    let analyzed = analysis.otd_mapping.ok_or(
        "The last analysis ran without OpenTabletDriver's settings. Choose its folder in Setup, then analyze again.",
    )?;
    if analysis.settings != app.config.fit {
        return Err(
            "The analysis numbers in Setup changed since the last analysis; analyze again.",
        );
    }
    let profile =
        setup::profile(app).ok_or("OpenTabletDriver's settings aren't loaded; check Setup.")?;
    if !profile.absolute {
        return Err(setup::RELATIVE_MODE);
    }
    if profile.mapping != analyzed {
        return Err("Your tablet area changed since the last analysis; analyze again.");
    }
    Ok(Proposal {
        profile,
        suggestion,
    })
}

fn table_row<'a>(
    label: &'a str,
    current: &str,
    capped: &str,
    full: &str,
) -> Element<'a, app::Message> {
    row![
        text(label).width(80),
        text(current.to_string()).width(COLUMN_WIDTH),
        text(capped.to_string()).width(COLUMN_WIDTH),
        text(full.to_string()).width(COLUMN_WIDTH),
    ]
    .into()
}

fn millimeters(value: f64, current: Option<f64>) -> String {
    match current {
        Some(current) => format!("{value:.2} mm ({:+.1}%)", (value / current - 1.0) * 100.0),
        None => format!("{value:.2} mm"),
    }
}

fn position((x, y): (f64, f64)) -> String {
    format!("{x:.1}, {y:.1} mm")
}
