use std::collections::HashSet;
use std::path::PathBuf;

use chrono::{DateTime, Local, Utc};
use iced::widget::{
    button, checkbox, column, pick_list, progress_bar, row, scrollable, space, text, text_input,
};
use iced::{Center, Element, Fill, Task};
use osu_tablet_optimizer::core::config::Config;
use osu_tablet_optimizer::core::library::Library;
use osu_tablet_optimizer::core::listing::{self, Period, Row};

use crate::app::{self, App, Page};

// ponytail: a plain column of rows; switch to a virtualized list if 500 rows feel slow.
const MAX_ROWS: usize = 500;

#[derive(Debug, Default)]
pub struct State {
    period: Period,
    search: String,
    rows: Vec<Row>,
    added: Vec<Row>,
    pub selected: HashSet<PathBuf>,
}

#[derive(Debug, Clone)]
pub enum Message {
    Period(Period),
    Search(String),
    Toggle(PathBuf, bool),
    ToggleAll(bool),
    AddFiles,
    FilesPicked(Vec<PathBuf>),
    Analyze,
}

impl State {
    pub fn refresh(&mut self, library: &Library, config: &Config) {
        let now_ms = Utc::now().timestamp_millis();
        self.rows = listing::rows(library, &config.players, self.period, &self.search, now_ms);
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
            state.refresh(&app.library, &app.config);
        }
        Message::Search(search) => {
            state.search = search;
            state.refresh(&app.library, &app.config);
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
        Message::Analyze => app.page = Page::Analysis,
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
    let analyze = button(text(format!("Analyze {} selected", state.selected.len())))
        .on_press_maybe(
            (!state.selected.is_empty()).then_some(app::Message::Replays(Message::Analyze)),
        );
    let toolbar = row![
        pick_list(Period::ALL, Some(state.period), |period| {
            app::Message::Replays(Message::Period(period))
        }),
        text_input("Search maps", &state.search)
            .on_input(|search| app::Message::Replays(Message::Search(search)))
            .width(260),
        button("Add files…").on_press(app::Message::Replays(Message::AddFiles)),
        scan_status(app),
        space::horizontal(),
        analyze,
    ]
    .spacing(8)
    .align_y(Center);

    let all_selected = state.shown().next().is_some()
        && state.shown().all(|row| state.selected.contains(&row.path));
    let header = row![
        checkbox(all_selected).on_toggle(|on| app::Message::Replays(Message::ToggleAll(on))),
        text("Played").width(140),
        text("Player").width(140),
        text("Map").width(Fill),
        text("Mods").width(80),
        text("Accuracy").width(80),
    ]
    .spacing(12);
    let rows = column(
        state
            .shown()
            .map(|row| row_view(row, state.selected.contains(&row.path))),
    )
    .spacing(4);
    let total = state.added.len() + state.rows.len();
    let footer = if total > MAX_ROWS {
        format!("Showing {MAX_ROWS} of {total} replays")
    } else {
        format!("{total} replays · drop .osr files anywhere to add them")
    };

    column![
        toolbar,
        header,
        scrollable(rows).height(Fill),
        text(footer).size(14)
    ]
    .spacing(12)
    .into()
}

fn row_view(row: &Row, selected: bool) -> Element<'_, app::Message> {
    let path = row.path.clone();
    iced::widget::row![
        checkbox(selected)
            .on_toggle(move |on| app::Message::Replays(Message::Toggle(path.clone(), on))),
        text(played_at(row.played_at_ms)).width(140),
        text(&row.player).width(140),
        text(&row.title).width(Fill),
        text(&row.mods).width(80),
        text(format!("{:.2}%", row.accuracy * 100.0)).width(80),
    ]
    .spacing(12)
    .into()
}

fn scan_status(app: &App) -> Element<'_, app::Message> {
    match app.scanning {
        Some((_, 0)) => text("Scanning osu! folders…").into(),
        Some((done, total)) => row![
            progress_bar(0.0..=total as f32, done as f32).length(160),
            text(format!("{done} / {total} files"))
        ]
        .spacing(8)
        .align_y(Center)
        .into(),
        None => button("Rescan").on_press(app::Message::Rescan).into(),
    }
}

fn played_at(ms: i64) -> String {
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
