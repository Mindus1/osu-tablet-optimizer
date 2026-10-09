use std::fmt;
use std::path::PathBuf;

use iced::widget::{button, column, pick_list, row, scrollable, text, text_input};
use iced::{Center, Element, Task};
use osu_tablet_optimizer::core::config::Config;
use osu_tablet_optimizer::core::{library, tablets};

use crate::app::{self, App};

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
            Number::MinJump => "Shortest jump analysed (circle radii)",
            Number::MaxError => "Largest miss still analysed (circle radii)",
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
        Message::Profile(profile) => app.config.profile = profile.index,
        Message::Players(players) => {
            app.config.players = players
                .split(',')
                .map(str::trim)
                .filter(|name| !name.is_empty())
                .map(str::to_string)
                .collect();
            app.setup.players = players;
            app.replays.refresh(&app.library, &app.config);
        }
        Message::DetectPlayer => {
            if let Some(player) = app.library.most_frequent_player() {
                app.config.players = vec![player.to_string()];
                app.setup.players = player.to_string();
                app.replays.refresh(&app.library, &app.config);
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
        let status = if root.is_dir() { "" } else { "  (not found)" };
        text(format!("{}{status}", root.display())).into()
    }))
    .spacing(4);
    let osu = column![
        heading("osu! folders"),
        osu_roots,
        row![
            button("Add folder…").on_press(app::Message::Setup(Message::AddOsuFolder)),
            button("Detect").on_press(app::Message::Setup(Message::DetectOsuFolders)),
            button("Rescan").on_press_maybe(app.scanning.is_none().then_some(app::Message::Rescan)),
        ]
        .spacing(8),
    ]
    .spacing(8);

    let players = column![
        heading("Players"),
        text("Only these players' replays are listed (comma-separated; empty lists everyone).")
            .size(14),
        row![
            text_input("player names", &app.setup.players)
                .on_input(|players| app::Message::Setup(Message::Players(players)))
                .width(320),
            button("Detect").on_press(app::Message::Setup(Message::DetectPlayer)),
        ]
        .spacing(8)
        .align_y(Center),
    ]
    .spacing(8);

    let analysis = column![heading("Analysis")]
        .extend(Number::ANALYSIS.map(|number| number_input(app, number)))
        .spacing(8);

    scrollable(
        column![
            osu,
            otd_section(app),
            tablet_section(app),
            players,
            analysis
        ]
        .spacing(24)
        .padding([0, 16]),
    )
    .into()
}

fn otd_section(app: &App) -> Element<'_, app::Message> {
    let folder = match app.config.otd_dir() {
        Some(dir) => text(dir.display().to_string()),
        None => text("Not found"),
    };
    let mut section = column![
        heading("OpenTabletDriver"),
        row![
            folder,
            button("Choose folder…").on_press(app::Message::Setup(Message::ChooseOtdFolder)),
        ]
        .spacing(8)
        .align_y(Center),
    ]
    .spacing(8);
    match &app.otd {
        Err(error) => section = section.push(text(error.clone())),
        Ok(settings) => {
            if !settings.is_supported() {
                section = section.push(text(format!(
                    "OpenTabletDriver {} is untested; this app reads the 0.6 settings format.",
                    settings.revision
                )));
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
            section = section.push(pick_list(profiles, selected, |profile| {
                app::Message::Setup(Message::Profile(profile))
            }));
            if settings
                .profiles
                .get(app.config.profile)
                .is_some_and(|profile| !profile.absolute)
            {
                section = section.push(text(
                    "This profile is not in absolute mode; area suggestions need absolute mode.",
                ));
            }
        }
    }
    section.into()
}

fn tablet_section(app: &App) -> Element<'_, app::Message> {
    let tablet = app
        .otd
        .as_ref()
        .ok()
        .and_then(|settings| settings.profiles.get(app.config.profile))
        .map(|profile| profile.tablet.as_str());
    let line = match (
        tablet,
        app.config.tablet_size,
        tablet.and_then(tablets::tablet_size),
    ) {
        (_, Some(_), _) => format!("{} — size entered by you.", tablet.unwrap_or("Tablet")),
        (Some(name), None, Some((width, height))) => {
            format!("{name} — {width} × {height} mm, from OpenTabletDriver's tablet list.")
        }
        (Some(name), None, None) => {
            format!("{name} — not in OpenTabletDriver's tablet list; enter its active area.")
        }
        (None, None, _) => {
            "No OpenTabletDriver profile yet; choose its folder above or enter the tablet's size."
                .to_string()
        }
    };
    column![
        heading("Tablet"),
        text(line),
        number_input(app, Number::TabletWidth),
        number_input(app, Number::TabletHeight),
        button("Use OpenTabletDriver's size").on_press_maybe(
            app.config
                .tablet_size
                .is_some()
                .then_some(app::Message::Setup(Message::UseTableSize))
        ),
    ]
    .spacing(8)
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
        text(number.label()).width(320),
        text_input(&placeholder, app.setup.text(number))
            .on_input(move |value| app::Message::Setup(Message::Number(number, value)))
            .width(120),
    ]
    .spacing(8)
    .align_y(Center)
    .into()
}

fn heading(label: &str) -> Element<'_, app::Message> {
    text(label).size(20).into()
}

// The size in use: the user's own entry first, then the bundled table.
pub fn tablet_size(app: &App) -> Option<(f64, f64)> {
    app.config.tablet_size.or_else(|| {
        let settings = app.otd.as_ref().ok()?;
        tablets::tablet_size(&settings.profiles.get(app.config.profile)?.tablet)
    })
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

fn format_number(value: f64) -> String {
    let rounded = (value * 1000.0).round() / 1000.0;
    rounded.to_string()
}
