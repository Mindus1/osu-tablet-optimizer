use std::fmt;
use std::path::PathBuf;

use iced::widget::{column, pick_list, row, text, text_input};
use iced::{Center, Element, Fill, Task};
use osu_tablet_optimizer::core::config::Config;
use osu_tablet_optimizer::core::{library, otd, tablets};

use crate::app::{self, App};
use crate::ui::style;

const MAX_WIDTH: f32 = 760.0;
const LABEL_WIDTH: f32 = 320.0;

pub const RELATIVE_MODE: &str =
    "This profile is not in absolute mode; area suggestions need absolute mode.";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Number {
    MinJump,
    MaxError,
    SizeStep,
    RotationStep,
    TabletWidth,
    TabletHeight,
}

impl Number {
    const ANALYSIS: [Number; 4] = [
        Number::MinJump,
        Number::MaxError,
        Number::SizeStep,
        Number::RotationStep,
    ];

    fn label(self) -> &'static str {
        match self {
            Number::MinJump => "Shortest jump analyzed (circle radii)",
            Number::MaxError => "Largest miss still analyzed (circle radii)",
            Number::SizeStep => "Largest size change per step (%)",
            Number::RotationStep => "Largest rotation change per step (°)",
            Number::TabletWidth => "Width (mm)",
            Number::TabletHeight => "Height (mm)",
        }
    }

    fn get(self, config: &Config) -> Option<f64> {
        match self {
            Number::MinJump => Some(config.fit.min_jump_radii),
            Number::MaxError => Some(config.fit.max_error_radii),
            Number::SizeStep => Some(config.fit.max_size_step * 100.0),
            Number::RotationStep => Some(config.fit.max_rotation_step_deg),
            Number::TabletWidth => config.tablet_size.map(|size| size.0),
            Number::TabletHeight => config.tablet_size.map(|size| size.1),
        }
    }

    // `tablet` is the size in use, so editing one side keeps the other.
    fn set(self, config: &mut Config, value: f64, tablet: (f64, f64)) {
        match self {
            Number::MinJump => config.fit.min_jump_radii = value,
            Number::MaxError => config.fit.max_error_radii = value,
            Number::SizeStep => config.fit.max_size_step = value / 100.0,
            Number::RotationStep => config.fit.max_rotation_step_deg = value,
            Number::TabletWidth => config.tablet_size = Some((value, tablet.1)),
            Number::TabletHeight => config.tablet_size = Some((tablet.0, value)),
        }
    }
}

// Text as typed, so half-entered numbers are not overwritten while editing.
#[derive(Debug)]
pub struct State {
    players: String,
    numbers: Vec<(Number, String)>,
}

#[derive(Debug, Clone)]
pub enum Message {
    AddOsuFolder,
    OsuFolderPicked(Option<PathBuf>),
    DetectOsuFolders,
    ChooseOtdFolder,
    OtdFolderPicked(Option<PathBuf>),
    Profile(Profile),
    Players(String),
    DetectPlayer,
    Number(Number, String),
    UseTableSize,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Profile {
    index: usize,
    label: String,
}

impl fmt::Display for Profile {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.label)
    }
}

impl State {
    pub fn new(config: &Config) -> State {
        let numbers = Number::ANALYSIS
            .into_iter()
            .chain([Number::TabletWidth, Number::TabletHeight])
            .map(|number| {
                let text = number.get(config).map(format_number).unwrap_or_default();
                (number, text)
            })
            .collect();
        State {
            players: config.players.join(", "),
            numbers,
        }
    }

    fn text(&self, number: Number) -> &str {
        self.numbers
            .iter()
            .find(|(n, _)| *n == number)
            .map_or("", |(_, text)| text)
    }
}

pub fn update(app: &mut App, message: Message) -> Task<app::Message> {
    match message {
        Message::AddOsuFolder => return pick_folder(Message::OsuFolderPicked),
        Message::OsuFolderPicked(Some(folder)) => {
            let mut roots = app.config.osu_roots();
            for root in library::roots_for(&folder) {
                if !roots.contains(&root) {
                    roots.push(root);
                }
            }
            app.config.osu_roots = Some(roots);
            return rescan(app);
        }
        Message::DetectOsuFolders => {
            app.config.osu_roots = None;
            return rescan(app);
        }
        Message::ChooseOtdFolder => return pick_folder(Message::OtdFolderPicked),
        Message::OtdFolderPicked(Some(folder)) => {
            app.config.otd_dir = Some(folder);
            app.config.profile = 0;
            app.reload_otd();
        }
        Message::OsuFolderPicked(None) | Message::OtdFolderPicked(None) => return Task::none(),
        Message::Profile(profile) => {
            app.config.profile = profile.index;
            app.reload_otd();
        }
        Message::Players(players) => {
            app.config.players = players
                .split(',')
                .map(str::trim)
                .filter(|name| !name.is_empty())
                .map(str::to_string)
                .collect();
            app.setup.players = players;
            app.replays.refresh(&app.library, &app.config, &app.history);
        }
        Message::DetectPlayer => {
            if let Some(player) = app.library.most_frequent_player() {
                app.config.players = vec![player.to_string()];
                app.setup.players = player.to_string();
                app.replays.refresh(&app.library, &app.config, &app.history);
            }
        }
        Message::Number(number, value) => {
            if let Ok(parsed) = value.trim().parse::<f64>()
                && parsed.is_finite()
                && parsed > 0.0
            {
                let tablet = tablet_size(app).unwrap_or((parsed, parsed));
                number.set(&mut app.config, parsed, tablet);
            }
            if let Some(entry) = app.setup.numbers.iter_mut().find(|(n, _)| *n == number) {
                entry.1 = value;
            }
        }
        Message::UseTableSize => {
            app.config.tablet_size = None;
            app.setup = State::new(&app.config);
        }
    }
    app.save_config();
    Task::none()
}

pub fn view(app: &App) -> Element<'_, app::Message> {
    let osu_roots = column(app.config.osu_roots().into_iter().map(|root| {
        let found = root.is_dir();
        row![
            style::dot(if found { style::OK } else { style::MISS }, 8.0),
            text(root.display().to_string()).size(14),
            (!found).then(|| text("not found").size(13).color(style::MISS)),
        ]
        .spacing(10)
        .align_y(Center)
        .into()
    }))
    .spacing(8);
    let osu = section(
        "osu! folders",
        "Where your replays and beatmaps are. osu!stable and osu!lazer are found on their own.",
        column![
            osu_roots,
            row![
                style::secondary("Add folder…")
                    .on_press(app::Message::Setup(Message::AddOsuFolder)),
                style::secondary("Detect again")
                    .on_press(app::Message::Setup(Message::DetectOsuFolders)),
                style::secondary("Rescan")
                    .on_press_maybe(app.scanning.is_none().then_some(app::Message::Rescan)),
            ]
            .spacing(8),
        ]
        .spacing(14),
    );

    let players = section(
        "Players",
        "Only these players' replays are listed. Separate names with commas; leave it empty to \
         list everyone.",
        row![
            text_input("Player names", &app.setup.players)
                .on_input(|players| app::Message::Setup(Message::Players(players)))
                .padding([8, 12])
                .width(320)
                .style(style::input),
            style::secondary("Detect").on_press(app::Message::Setup(Message::DetectPlayer)),
        ]
        .spacing(8)
        .align_y(Center),
    );

    let analysis = section(
        "Analysis",
        "Which jumps count, and how far one suggested step may move your area.",
        column(Number::ANALYSIS.map(|number| number_input(app, number))).spacing(10),
    );

    style::scroll(style::page(
        "Setup",
        "Where your replays and tablet settings live, and how the analysis treats them.",
        row![],
        column![
            osu,
            otd_section(app),
            tablet_section(app),
            players,
            analysis
        ]
        .spacing(16)
        .max_width(MAX_WIDTH),
    ))
    .into()
}

fn otd_section(app: &App) -> Element<'_, app::Message> {
    let folder = match app.config.otd_dir() {
        Some(dir) => text(dir.display().to_string()).size(14),
        None => text("Not found").size(14).color(style::MISS),
    };
    let mut content = column![
        row![
            folder.width(Fill),
            style::secondary("Choose folder…")
                .on_press(app::Message::Setup(Message::ChooseOtdFolder)),
        ]
        .spacing(12)
        .align_y(Center),
    ]
    .spacing(12);
    match &app.otd {
        Err(error) => content = content.push(style::banner(style::MISS, text(error.clone()))),
        Ok(settings) => {
            if !settings.is_supported() {
                content = content.push(style::banner(
                    style::MEH,
                    text(format!(
                        "OpenTabletDriver {} is untested; this app reads the 0.6 settings format.",
                        settings.revision
                    )),
                ));
            }
            let profiles: Vec<Profile> = settings
                .profiles
                .iter()
                .enumerate()
                .map(|(index, profile)| Profile {
                    index,
                    label: profile.tablet.clone(),
                })
                .collect();
            let selected = profiles.get(app.config.profile).cloned();
            content = content.push(
                row![
                    text("Profile").size(14).width(LABEL_WIDTH),
                    pick_list(profiles, selected, |profile| {
                        app::Message::Setup(Message::Profile(profile))
                    })
                    .padding([8, 12])
                    .style(style::select),
                ]
                .spacing(12)
                .align_y(Center),
            );
            if profile(app).is_some_and(|profile| !profile.absolute) {
                content = content.push(style::banner(style::MEH, text(RELATIVE_MODE)));
            }
        }
    }
    section(
        "OpenTabletDriver",
        "Its settings are only read. For a portable install, choose its program folder.",
        content,
    )
}

fn tablet_section(app: &App) -> Element<'_, app::Message> {
    let tablet = profile(app).map(|profile| profile.tablet.as_str());
    let source = match (
        tablet,
        app.config.tablet_size,
        tablet.and_then(tablets::tablet_size),
    ) {
        (_, Some(_), _) => "Size entered by you.".to_string(),
        (Some(_), None, Some((width, height))) => {
            format!("{width} × {height} mm, from OpenTabletDriver's tablet list.")
        }
        (Some(_), None, None) => {
            "Not in OpenTabletDriver's tablet list; enter its active area below.".to_string()
        }
        (None, None, _) => {
            "No OpenTabletDriver profile yet; choose its folder above or enter the tablet's size."
                .to_string()
        }
    };
    section(
        "Tablet",
        "The active area's size keeps exported areas on the tablet.",
        column![
            column![
                tablet.map(|name| text(name).size(15).font(style::SEMIBOLD)),
                style::muted(source),
            ]
            .spacing(2),
            number_input(app, Number::TabletWidth),
            number_input(app, Number::TabletHeight),
            style::secondary("Use OpenTabletDriver's size").on_press_maybe(
                app.config
                    .tablet_size
                    .is_some()
                    .then_some(app::Message::Setup(Message::UseTableSize))
            ),
        ]
        .spacing(10),
    )
}

fn section<'a>(
    title: &'a str,
    about: &'a str,
    content: impl Into<Element<'a, app::Message>>,
) -> Element<'a, app::Message> {
    style::panel(
        column![
            column![style::heading(title), style::muted(about)].spacing(4),
            content.into()
        ]
        .spacing(16),
    )
    .into()
}

fn number_input(app: &App, number: Number) -> Element<'_, app::Message> {
    let size = tablet_size(app);
    let placeholder = match number {
        Number::TabletWidth => size.map(|size| format_number(size.0)),
        Number::TabletHeight => size.map(|size| format_number(size.1)),
        _ => None,
    }
    .unwrap_or_default();
    row![
        text(number.label()).size(14).width(LABEL_WIDTH),
        text_input(&placeholder, app.setup.text(number))
            .on_input(move |value| app::Message::Setup(Message::Number(number, value)))
            .padding([6, 10])
            .width(120)
            .style(style::input),
    ]
    .spacing(12)
    .align_y(Center)
    .into()
}

pub fn profile(app: &App) -> Option<&otd::Profile> {
    app.otd.as_ref().ok()?.profiles.get(app.config.profile)
}

// The size in use: the user's own entry first, then the bundled table.
pub fn tablet_size(app: &App) -> Option<(f64, f64)> {
    app.config
        .tablet_size
        .or_else(|| tablets::tablet_size(&profile(app)?.tablet))
}

fn pick_folder(picked: fn(Option<PathBuf>) -> Message) -> Task<app::Message> {
    Task::perform(rfd::AsyncFileDialog::new().pick_folder(), move |folder| {
        app::Message::Setup(picked(folder.map(|folder| folder.path().to_path_buf())))
    })
}

fn rescan(app: &mut App) -> Task<app::Message> {
    app.save_config();
    Task::done(app::Message::Rescan)
}

pub fn format_number(value: f64) -> String {
    let rounded = (value * 1000.0).round() / 1000.0;
    rounded.to_string()
}
