use iced::widget::{canvas, column, row, text};
use iced::{Center, Element, Fill, FillPortion, Task};
use osu_tablet_optimizer::core::fit::{Mapping, Suggestion};
use osu_tablet_optimizer::core::otd::{self, Profile};

use crate::app::{self, App};
use crate::ui::{charts, setup, style};

const PREVIEW_HEIGHT: f32 = 280.0;

#[derive(Debug, Default)]
pub struct State {
    // Outcome of the last export: what to tell the user, as saved or failed.
    exported: Option<Result<String, String>>,
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
        Ok(path) => Ok(format!(
            "Saved preset {}. In OpenTabletDriver, choose it under File → Presets \
             (Refresh presets first if it's running).",
            path.file_stem().unwrap_or_default().to_string_lossy()
        )),
        Err(error) => Err(format!("{error:#}")),
    });
    // A refused export usually means OTD's settings changed; show them as they are now.
    app.reload_otd();
    Task::none()
}

pub fn view(app: &App) -> Element<'_, app::Message> {
    const ABOUT: &str = "Your suggested tablet area, next to the one you use now.";
    let proposal = match proposal(app) {
        Ok(proposal) => proposal,
        Err(reason) => {
            return style::page("Area", ABOUT, row![], style::panel(style::muted(reason)));
        }
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
    .width(Fill)
    .height(PREVIEW_HEIGHT);
    let legend = row![
        legend_entry(style::swatch(style::MUTED), "Now"),
        legend_entry(style::swatch(style::PINK), "Capped step"),
        legend_entry(style::dashed_swatch(style::PINK), "Full fit"),
    ]
    .spacing(20);
    let mut preview_panel = column![preview, legend].spacing(16);
    if tablet_size.is_none() {
        preview_panel = preview_panel.push(style::muted(
            "Enter the tablet's size in Setup to see the tablet around the areas.",
        ));
    }

    let current = (profile.mapping, profile.center);
    let capped = (suggestion.capped, center(&suggestion.capped));
    let full = (suggestion.full, center(&suggestion.full));
    let table = column![
        table_row("", "Now", "Capped step", "Full fit", true),
        style::divider(),
        table_row(
            "Width",
            &millimeters(current.0.area_width, None),
            &millimeters(capped.0.area_width, Some(current.0.area_width)),
            &millimeters(full.0.area_width, Some(current.0.area_width)),
            false,
        ),
        table_row(
            "Height",
            &millimeters(current.0.area_height, None),
            &millimeters(capped.0.area_height, Some(current.0.area_height)),
            &millimeters(full.0.area_height, Some(current.0.area_height)),
            false,
        ),
        table_row(
            "Rotation",
            &format!("{:.2}°", current.0.rotation),
            &format!("{:.2}°", capped.0.rotation),
            &format!("{:.2}°", full.0.rotation),
            false,
        ),
        table_row(
            "Center",
            &position(current.1),
            &position(capped.1),
            &position(full.1),
            false,
        ),
    ]
    .spacing(10);

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
    let mut notes = column![].spacing(8);
    if !kept.is_empty() {
        notes = notes.push(style::muted(format!(
            "Kept, because the analysis shows no clear effect: {}.",
            kept.join(", ")
        )));
    }
    let settings = app.config.fit;
    notes = notes.push(style::muted(format!(
        "The capped step changes the size by at most {}% and the rotation by at most {}°. \
         Play a few sessions with it, then analyze again.",
        settings.max_size_step * 100.0,
        settings.max_rotation_step_deg
    )));

    let nothing_to_change = suggestion.full == profile.mapping;
    let actions: Element<'_, app::Message> = if nothing_to_change {
        row![].into()
    } else {
        row![
            style::secondary("Export full fit")
                .on_press(app::Message::Area(Message::Export(Step::Full))),
            style::primary("Export capped step")
                .on_press(app::Message::Area(Message::Export(Step::Capped))),
        ]
        .spacing(8)
        .into()
    };

    let mut content = column![].spacing(16);
    if nothing_to_change {
        content = content.push(style::banner(
            style::GREAT,
            text("Your area fits your aim: nothing to change."),
        ));
    }
    let elsewhere = app.analysis.last.as_ref().map_or(0, |analysis| {
        app.history.from_other_areas(&analysis.report.measured)
    });
    if elsewhere > 0 {
        content = content.push(style::banner(
            style::MEH,
            text(format!(
                "{elsewhere} of the analyzed replays were played with an older area. The \
                 suggestion corrects your current area, so filter the Replays page to it and \
                 analyze again."
            )),
        ));
    }
    if let Some(exported) = &app.area.exported {
        let (tone, message) = match exported {
            Ok(message) => (style::OK, message),
            Err(message) => (style::MISS, message),
        };
        content = content.push(style::banner(tone, text(message.as_str())));
    }
    content = content.push(
        row![
            style::panel(preview_panel).width(FillPortion(5)),
            column![style::panel(table), notes]
                .spacing(16)
                .width(FillPortion(6)),
        ]
        .spacing(16),
    );
    style::scroll(style::page("Area", ABOUT, actions, content)).into()
}

fn legend_entry<'a>(
    swatch: Element<'a, app::Message>,
    label: &'a str,
) -> Element<'a, app::Message> {
    row![swatch, style::muted(label)]
        .spacing(8)
        .align_y(Center)
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
    header: bool,
) -> Element<'a, app::Message> {
    let cell = |value: &str| {
        let cell = text(value.to_string()).size(14).width(Fill);
        if header {
            cell.font(style::SEMIBOLD).color(style::MUTED).size(13)
        } else {
            cell
        }
    };
    row![
        style::muted(label).width(70),
        cell(current),
        cell(capped),
        cell(full),
    ]
    .spacing(12)
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
