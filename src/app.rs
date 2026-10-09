use std::path::PathBuf;
use std::sync::Arc;

use iced::futures::{SinkExt, StreamExt};
use iced::widget::{button, column, container, progress_bar, row, space, text};
use iced::{Center, Element, Fill, Subscription, Task, Theme, event, window};
use osu_tablet_optimizer::core::config::Config;
use osu_tablet_optimizer::core::history::History;
use osu_tablet_optimizer::core::library::Library;
use osu_tablet_optimizer::core::otd::OtdSettings;
use osu_tablet_optimizer::core::storage;

use crate::ui::{analysis, area, history, replays, setup, style};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Page {
    #[default]
    Replays,
    Analysis,
    Area,
    History,
    Setup,
}

impl Page {
    const ALL: [Page; 5] = [
        Page::Replays,
        Page::Analysis,
        Page::Area,
        Page::History,
        Page::Setup,
    ];

    fn label(self) -> &'static str {
        match self {
            Page::Replays => "Replays",
            Page::Analysis => "Analysis",
            Page::Area => "Area",
            Page::History => "History",
            Page::Setup => "Setup",
        }
    }
}

#[derive(Debug, Clone)]
pub enum Message {
    Navigate(Page),
    DismissNotice,
    Rescan,
    Scan(ScanEvent),
    FileDropped(PathBuf),
    Replays(replays::Message),
    Setup(setup::Message),
    Analysis(analysis::Message),
    Area(area::Message),
    History(history::Message),
}

#[derive(Debug, Clone)]
pub enum ScanEvent {
    Progress(usize, usize),
    Done {
        library: Arc<Library>,
        unreadable: usize,
        error: Option<String>,
    },
    // The scan thread stopped without a result (it panicked).
    Failed,
}

pub struct App {
    pub page: Page,
    pub config: Config,
    first_run: bool,
    pub library: Arc<Library>,
    pub scanning: Option<(usize, usize)>,
    // A rescan asked for while scanning (e.g. a folder added in Setup) runs right after.
    rescan_pending: bool,
    pub otd: Result<OtdSettings, String>,
    pub history: History,
    pub replays: replays::State,
    pub setup: setup::State,
    pub analysis: analysis::State,
    pub area: area::State,
    pub history_page: history::State,
    pub notice: Option<String>,
}

impl App {
    pub fn new() -> (App, Task<Message>) {
        let loaded = config_path().map(|path| Config::load(&path));
        let (config, first_run, mut notice) = match loaded {
            Some(Ok(Some(config))) => (config, false, None),
            Some(Ok(None)) | None => (Config::default(), true, None),
            Some(Err(error)) => (
                Config::default(),
                true,
                Some(set_aside(config_path(), &error, "defaults are used")),
            ),
        };
        let history = match history_path().map(|path| History::load(&path)) {
            Some(Ok(history)) => history,
            Some(Err(error)) => {
                notice = Some(set_aside(
                    history_path(),
                    &error,
                    "a new history is started",
                ));
                History::default()
            }
            None => History::default(),
        };
        let mut app = App {
            page: if first_run {
                Page::Setup
            } else {
                Page::Replays
            },
            otd: Err(String::new()),
            history,
            setup: setup::State::new(&config),
            config,
            first_run,
            library: Arc::default(),
            scanning: Some((0, 0)),
            rescan_pending: false,
            replays: replays::State::default(),
            analysis: analysis::State::default(),
            area: area::State::default(),
            history_page: history::State::default(),
            notice,
        };
        app.reload_otd();
        let scan = app.scan();
        (app, scan)
    }

    pub fn update(&mut self, message: Message) -> Task<Message> {
        match message {
            Message::Navigate(page) => self.page = page,
            Message::DismissNotice => self.notice = None,
            Message::Rescan if self.scanning.is_none() => {
                self.reload_otd();
                self.scanning = Some((0, 0));
                return self.scan();
            }
            Message::Rescan => self.rescan_pending = true,
            Message::Scan(ScanEvent::Progress(done, total)) => self.scanning = Some((done, total)),
            Message::Scan(ScanEvent::Done {
                library,
                unreadable,
                error,
            }) => {
                self.library = library;
                if self.first_run
                    && self.config.players.is_empty()
                    && let Some(player) = self.library.most_frequent_player()
                {
                    self.config.players = vec![player.to_string()];
                    self.setup = setup::State::new(&self.config);
                    self.save_config();
                }
                if let Some(problem) = error.or_else(|| {
                    (unreadable > 0).then(|| format!("{unreadable} files could not be read"))
                }) {
                    self.notice = Some(problem);
                }
                self.replays
                    .refresh(&self.library, &self.config, &self.history);
                return self.finish_scan();
            }
            Message::Scan(ScanEvent::Failed) => {
                self.notice = Some("Scanning the osu! folders failed; try Rescan.".to_string());
                return self.finish_scan();
            }
            Message::FileDropped(path) => replays::add_files(self, vec![path]),
            Message::Replays(message) => return replays::update(self, message),
            Message::Setup(message) => return setup::update(self, message),
            Message::Analysis(message) => return analysis::update(self, message),
            Message::Area(message) => return area::update(self, message),
            Message::History(message) => return history::update(self, message),
        }
        Task::none()
    }

    pub fn view(&self) -> Element<'_, Message> {
        let page = match self.page {
            Page::Replays => replays::view(self),
            Page::Setup => setup::view(self),
            Page::Analysis => analysis::view(self),
            Page::Area => area::view(self),
            Page::History => history::view(self),
        };
        let notice = self.notice.as_ref().map(|notice| {
            style::banner(
                style::MEH,
                row![
                    text(notice).width(Fill),
                    style::secondary("Dismiss").on_press(Message::DismissNotice),
                ]
                .spacing(12)
                .align_y(Center),
            )
        });

        row![
            self.sidebar(),
            container(column![notice, page].spacing(20))
                .padding([28, 32])
                .width(Fill)
                .height(Fill)
        ]
        .into()
    }

    fn sidebar(&self) -> Element<'_, Message> {
        let brand = row![
            style::ring(style::PINK, 26.0),
            column![
                text("Tablet Optimizer").size(17).font(style::SEMIBOLD),
                style::muted("for osu!"),
            ],
        ]
        .spacing(12)
        .align_y(Center);
        let nav = Page::ALL.map(|page| {
            let active = page == self.page;
            let marker = if active {
                style::ring(style::PINK, 10.0)
            } else {
                space().width(10).into()
            };
            button(row![marker, text(page.label())].spacing(12).align_y(Center))
                .width(Fill)
                .padding([9, 12])
                .style(style::nav(active))
                .on_press(Message::Navigate(page))
                .into()
        });

        container(
            column![
                brand,
                column(nav).spacing(4),
                space::vertical(),
                self.scan_status(),
            ]
            .spacing(32),
        )
        .padding([28, 16])
        .width(220)
        .height(Fill)
        .style(|_| container::Style::default().background(style::PANEL))
        .into()
    }

    fn scan_status(&self) -> Element<'_, Message> {
        let status: Element<'_, Message> = match self.scanning {
            Some((_, 0)) => style::muted("Looking for replays…").into(),
            Some((done, total)) => column![
                style::muted(format!("Reading {done} of {total} new files")),
                progress_bar(0.0..=total as f32, done as f32)
                    .girth(4)
                    .style(|_| progress_bar::Style {
                        background: style::LINE.into(),
                        bar: style::PINK.into(),
                        border: iced::border::rounded(2),
                    }),
            ]
            .spacing(8)
            .into(),
            None => column![
                style::muted("Played since the last scan?"),
                style::secondary("Rescan")
                    .width(Fill)
                    .on_press(Message::Rescan),
            ]
            .spacing(8)
            .into(),
        };
        column![style::divider(), status].spacing(16).into()
    }

    pub fn theme(&self) -> Theme {
        style::theme()
    }

    pub fn subscription(&self) -> Subscription<Message> {
        event::listen_with(|event, _, _| match event {
            iced::Event::Window(window::Event::FileDropped(path)) => {
                Some(Message::FileDropped(path))
            }
            _ => None,
        })
    }

    fn finish_scan(&mut self) -> Task<Message> {
        self.scanning = None;
        if std::mem::take(&mut self.rescan_pending) {
            return self.update(Message::Rescan);
        }
        Task::none()
    }

    pub fn save_config(&mut self) {
        if let Some(Err(error)) = config_path().map(|path| self.config.save(&path)) {
            self.notice = Some(format!("{error:#}"));
        }
    }

    pub fn save_history(&mut self) {
        if let Some(Err(error)) = history_path().map(|path| self.history.save(&path)) {
            self.notice = Some(format!("{error:#}"));
        }
    }

    // Reads OpenTabletDriver's settings again and logs the profile's area when it changed.
    pub fn reload_otd(&mut self) {
        self.otd = match self.config.otd_dir() {
            Some(dir) => OtdSettings::load(&dir).map_err(|error| format!("{error:#}")),
            None => Err("OpenTabletDriver settings not found; choose its folder.".to_string()),
        };
        let saved_at_ms = self.otd.as_ref().map_or(0, |settings| settings.saved_at_ms);
        let area = setup::profile(self)
            .filter(|profile| profile.absolute)
            .map(|profile| profile.mapping);
        if let Some(mapping) = area
            && self.history.observe(mapping, saved_at_ms)
        {
            self.save_history();
            self.replays
                .refresh(&self.library, &self.config, &self.history);
        }
    }

    // Loads the cached library, rescans every root on a background thread and saves the cache.
    pub fn scan(&self) -> Task<Message> {
        let roots = self.config.osu_roots();
        let cache = cache_path();
        Task::run(
            iced::stream::channel(16, async move |mut output| {
                let (sender, mut events) = iced::futures::channel::mpsc::unbounded();
                std::thread::spawn(move || {
                    let mut library = cache
                        .as_deref()
                        .and_then(|path| Library::load(path).ok())
                        .unwrap_or_default();
                    let unreadable = library.scan(&roots, &|done, total| {
                        let _ = sender.unbounded_send(ScanEvent::Progress(done, total));
                    });
                    let error = cache
                        .as_deref()
                        .and_then(|path| library.save(path).err())
                        .map(|error| format!("{error:#}"));
                    let _ = sender.unbounded_send(ScanEvent::Done {
                        library: Arc::new(library),
                        unreadable: unreadable.len(),
                        error,
                    });
                });
                let mut finished = false;
                while let Some(event) = events.next().await {
                    finished |= matches!(event, ScanEvent::Done { .. });
                    if output.send(event).await.is_err() {
                        return;
                    }
                }
                if !finished {
                    let _ = output.send(ScanEvent::Failed).await;
                }
            }),
            Message::Scan,
        )
    }
}

// The unreadable file is moved aside so saving later doesn't destroy it.
fn set_aside(path: Option<PathBuf>, error: &anyhow::Error, fallback: &str) -> String {
    match path.map(|path| storage::set_aside(&path)) {
        Some(Ok(aside)) => format!(
            "{error:#}; the file was moved to {} and {fallback}.",
            aside.display()
        ),
        _ => format!("{error:#}"),
    }
}

fn config_path() -> Option<PathBuf> {
    storage::app_dir().map(|dir| dir.join("config.json"))
}

fn history_path() -> Option<PathBuf> {
    storage::app_dir().map(|dir| dir.join("history.json"))
}

fn cache_path() -> Option<PathBuf> {
    storage::app_dir().map(|dir| dir.join("library.json"))
}
