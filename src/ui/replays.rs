use std::collections::HashSet;
use std::fmt;
use std::path::PathBuf;

use chrono::{DateTime, Local, Utc};
use iced::widget::{checkbox, column, container, pick_list, row, space, text, text_input};
use iced::{Center, Color, Element, Fill, Right, Task};
use osu_tablet_optimizer::core::config::Config;
use osu_tablet_optimizer::core::history::History;
use osu_tablet_optimizer::core::library::Library;
use osu_tablet_optimizer::core::listing::{self, Period, Row};

use crate::app::{self, App, Page};
use crate::ui::{analysis, history, style};

// ponytail: a plain column of rows; switch to a virtualized list if 500 rows feel slow.
const MAX_ROWS: usize = 500;
const PLAYED_WIDTH: f32 = 130.0;
const PLAYER_WIDTH: f32 = 130.0;
const AREA_WIDTH: f32 = 170.0;
const MODS_WIDTH: f32 = 70.0;
const ACCURACY_WIDTH: f32 = 76.0;

#[derive(Debug, Default)]
pub struct State {
    period: Period,
    // None: every area.
    area: Option<usize>,
    search: String,
    rows: Vec<Row>,
    added: Vec<Row>,
    pub selected: HashSet<PathBuf>,
}

#[derive(Debug, Clone)]
pub enum Message {
    Period(Period),
    Area(Option<usize>),
    Assign(Option<usize>),
    Search(String),
    Toggle(PathBuf, bool),
    ToggleAll(bool),
    AddFiles,
    FilesPicked(Vec<PathBuf>),
    Analyze,
}

// A pick-list entry for an area; None stands for "every area" or "by date".
#[derive(Debug, Clone, PartialEq)]
struct AreaChoice {
    area: Option<usize>,
    label: String,
}

impl fmt::Display for AreaChoice {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.label)
    }
}

impl State {
    pub fn refresh(&mut self, library: &Library, config: &Config, history: &History) {
        let now_ms = Utc::now().timestamp_millis();
        self.rows = listing::rows(library, &config.players, self.period, &self.search, now_ms);
        if let Some(area) = self.area {
            self.rows
                .retain(|row| history.snapshot_of(&row.path, row.played_at_ms) == Some(area));
        }
    }

    fn shown(&self) -> impl Iterator<Item = &Row> {
        self.added.iter().chain(&self.rows).take(MAX_ROWS)
    }
}

pub fn update(app: &mut App, message: Message) -> Task<app::Message> {
    let state = &mut app.replays;
    match message {
        Message::Period(period) => {
            state.period = period;
            state.refresh(&app.library, &app.config, &app.history);
        }
        Message::Area(area) => {
            state.area = area;
            state.refresh(&app.library, &app.config, &app.history);
        }
        Message::Assign(area) => {
            app.history.assign(state.selected.iter().cloned(), area);
            app.save_history();
            app.replays.refresh(&app.library, &app.config, &app.history);
        }
        Message::Search(search) => {
            state.search = search;
            state.refresh(&app.library, &app.config, &app.history);
        }
        Message::Toggle(path, on) => select(&mut state.selected, path, on),
        Message::ToggleAll(on) => {
            let shown: Vec<PathBuf> = state.shown().map(|row| row.path.clone()).collect();
            for path in shown {
                select(&mut state.selected, path, on);
            }
        }
        Message::AddFiles => {
            let dialog = rfd::AsyncFileDialog::new()
                .add_filter("osu! replay", &["osr"])
                .pick_files();
            return Task::perform(dialog, |files| {
                let paths = files
                    .unwrap_or_default()
                    .iter()
                    .map(|file| file.path().to_path_buf())
                    .collect();
                app::Message::Replays(Message::FilesPicked(paths))
            });
        }
        Message::FilesPicked(paths) => add_files(app, paths),
        Message::Analyze => {
            app.page = Page::Analysis;
            return analysis::start(app);
        }
    }
    Task::none()
}

// Picked or dropped replays are listed above the library and selected right away.
pub fn add_files(app: &mut App, paths: Vec<PathBuf>) {
    let beatmaps = app.library.beatmaps_by_md5();
    for path in paths {
        match listing::row_for_file(&path, &beatmaps) {
            Ok(row) => {
                app.replays.selected.insert(row.path.clone());
                if !app.replays.added.iter().any(|added| added.path == row.path) {
                    app.replays.added.push(row);
                }
            }
            Err(error) => app.notice = Some(format!("{error:#}")),
        }
    }
}

pub fn view(app: &App) -> Element<'_, app::Message> {
    let state = &app.replays;
    let actions = row![
        style::secondary("Add files…").on_press(app::Message::Replays(Message::AddFiles)),
        style::primary(format!("Analyze {} selected", state.selected.len())).on_press_maybe(
            (!state.selected.is_empty()).then_some(app::Message::Replays(Message::Analyze))
        ),
    ]
    .spacing(8);
    let filters = row![
        pick_list(Period::ALL, Some(state.period), |period| {
            app::Message::Replays(Message::Period(period))
        })
        .padding([8, 12])
        .style(style::select),
        area_filter(app),
        text_input("Search maps", &state.search)
            .on_input(|search| app::Message::Replays(Message::Search(search)))
            .padding([8, 12])
            .width(280)
            .style(style::input),
    ]
    .spacing(8)
    .align_y(Center);

    column![
        style::page(
            "Replays",
            "Pick the plays to analyze, newest first.",
            actions,
            filters
        ),
        table(app),
    ]
    .spacing(16)
    .into()
}

fn table(app: &App) -> Element<'_, app::Message> {
    let state = &app.replays;
    let total = state.added.len() + state.rows.len();
    if total == 0 {
        let hint = if app.scanning.is_some() {
            "Your osu! folders are being scanned; replays show up here when it finishes."
        } else {
            "No replays match. Try another period or area, or check the players and osu! \
             folders in Setup."
        };
        return style::panel(style::muted(hint)).into();
    }

    let all_selected = state.shown().all(|row| state.selected.contains(&row.path));
    let header = row![
        checkbox(all_selected).on_toggle(|on| app::Message::Replays(Message::ToggleAll(on))),
        column_label("Played").width(PLAYED_WIDTH),
        column_label("Player").width(PLAYER_WIDTH),
        column_label("Map").width(Fill),
        column_label("Area").width(AREA_WIDTH),
        column_label("Mods").width(MODS_WIDTH),
        column_label("Accuracy")
            .width(ACCURACY_WIDTH)
            .align_x(Right),
    ]
    .spacing(16)
    .align_y(Center)
    .padding([12, 20]);
    let rows = column(state.shown().map(|row| {
        let area = app.history.snapshot_of(&row.path, row.played_at_ms);
        row_view(
            row,
            history::area_label(&app.history, area),
            state.selected.contains(&row.path),
        )
    }));
    let count = if total > MAX_ROWS {
        format!("Showing the newest {MAX_ROWS} of {total} replays.")
    } else {
        format!("{total} replays. Drop .osr files anywhere to add more.")
    };
    let assign = (!state.selected.is_empty() && !app.history.snapshots.is_empty()).then(|| {
        pick_list(
            area_choices(&app.history, "By date"),
            None::<AreaChoice>,
            |choice| app::Message::Replays(Message::Assign(choice.area)),
        )
        .placeholder("Assign selected to area…")
        .padding([8, 12])
        .style(style::select)
    });
    let footer = row![style::muted(count), space::horizontal(), assign]
        .align_y(Center)
        .padding([10, 20]);

    style::panel(column![
        header,
        style::divider(),
        style::scroll(rows).height(Fill),
        style::divider(),
        footer
    ])
    .padding(0)
    .height(Fill)
    .into()
}

fn row_view(row: &Row, area: String, selected: bool) -> Element<'_, app::Message> {
    let path = row.path.clone();
    let cells = iced::widget::row![
        checkbox(selected)
            .on_toggle(move |on| app::Message::Replays(Message::Toggle(path.clone(), on))),
        text(played_at(row.played_at_ms))
            .size(14)
            .color(style::MUTED)
            .width(PLAYED_WIDTH),
        text(&row.player).size(14).width(PLAYER_WIDTH),
        text(&row.title).size(14).width(Fill),
        text(area).size(14).color(style::MUTED).width(AREA_WIDTH),
        text(&row.mods).size(14).width(MODS_WIDTH),
        text(format!("{:.2}%", row.accuracy * 100.0))
            .size(14)
            .width(ACCURACY_WIDTH)
            .align_x(Right),
    ]
    .spacing(16)
    .align_y(Center);
    container(cells)
        .padding([8, 20])
        .width(Fill)
        .style(move |_| {
            let tint = selected.then_some(Color {
                a: 0.08,
                ..style::PINK
            });
            container::Style {
                background: tint.map(Into::into),
                ..container::Style::default()
            }
        })
        .into()
}

fn column_label(label: &str) -> iced::widget::Text<'_> {
    text(label)
        .size(13)
        .font(style::SEMIBOLD)
        .color(style::MUTED)
}

fn area_filter(app: &App) -> Element<'_, app::Message> {
    let choices = area_choices(&app.history, "All areas");
    let selected = choices
        .iter()
        .find(|choice| choice.area == app.replays.area)
        .cloned();
    pick_list(choices, selected, |choice| {
        app::Message::Replays(Message::Area(choice.area))
    })
    .padding([8, 12])
    .style(style::select)
    .into()
}

fn area_choices(history: &History, none: &str) -> Vec<AreaChoice> {
    let areas = (0..history.snapshots.len()).map(|index| AreaChoice {
        area: Some(index),
        label: history::area_label(history, Some(index)),
    });
    std::iter::once(AreaChoice {
        area: None,
        label: none.to_string(),
    })
    .chain(areas)
    .collect()
}

pub fn played_at(ms: i64) -> String {
    DateTime::from_timestamp_millis(ms)
        .map(|time| {
            time.with_timezone(&Local)
                .format("%Y-%m-%d %H:%M")
                .to_string()
        })
        .unwrap_or_default()
}

fn select(selected: &mut HashSet<PathBuf>, path: PathBuf, on: bool) {
    if on {
        selected.insert(path);
    } else {
        selected.remove(&path);
    }
}
