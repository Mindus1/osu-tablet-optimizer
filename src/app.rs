use std::path::PathBuf;
use std::sync::Arc;

use iced::futures::{SinkExt, StreamExt};
use iced::widget::{button, column, container, row, text};
use iced::{Element, Fill, Subscription, Task, Theme, event, window};
use osu_tablet_optimizer::core::config::Config;
use osu_tablet_optimizer::core::library::Library;
use osu_tablet_optimizer::core::otd::OtdSettings;
use osu_tablet_optimizer::core::storage;

use crate::ui::{replays, setup};

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
    Rescan,
    Scan(ScanEvent),
    FileDropped(PathBuf),
    Replays(replays::Message),
    Setup(setup::Message),
}

#[derive(Debug, Clone)]
pub enum ScanEvent {
    Progress(usize, usize),
    Done {
        library: Arc<Library>,
        unreadable: usize,
        error: Option<String>,
    },
}

pub struct App {
    pub page: Page,
    pub config: Config,
    first_run: bool,
    pub library: Arc<Library>,
    pub scanning: Option<(usize, usize)>,
    pub otd: Result<OtdSettings, String>,
    pub replays: replays::State,
    pub setup: setup::State,
    pub notice: Option<String>,
}

impl App {
    pub fn new() -> (App, Task<Message>) {
        let loaded = config_path().map(|path| Config::load(&path));
        let (config, first_run, notice) = match loaded {
            Some(Ok(Some(config))) => (config, false, None),
            Some(Ok(None)) | None => (Config::default(), true, None),
            Some(Err(error)) => (Config::default(), false, Some(format!("{error:#}"))),
        };
        let mut app = App {
            page: if first_run {
                Page::Setup
            } else {
                Page::Replays
            },
            otd: Err(String::new()),
            setup: setup::State::new(&config),
            config,
            first_run,
            library: Arc::default(),
            scanning: Some((0, 0)),
            replays: replays::State::default(),
            notice,
        };
        app.reload_otd();
        let scan = app.scan();
        (app, scan)
    }

    pub fn update(&mut self, message: Message) -> Task<Message> {
        match message {
            Message::Navigate(page) => self.page = page,
            Message::Rescan if self.scanning.is_none() => {
                self.scanning = Some((0, 0));
                return self.scan();
            }
            Message::Rescan => {}
            Message::Scan(ScanEvent::Progress(done, total)) => self.scanning = Some((done, total)),
            Message::Scan(ScanEvent::Done {
                library,
                unreadable,
                error,
            }) => {
                self.scanning = None;
                self.library = library;
                if self.first_run
                    && self.config.players.is_empty()
                    && let Some(player) = self.library.most_frequent_player()
                {
                    self.config.players = vec![player.to_string()];
                    self.setup = setup::State::new(&self.config);
                    self.save_config();
                }
                self.notice = error.or_else(|| {
                    (unreadable > 0).then(|| format!("{unreadable} files could not be read"))
                });
                self.replays.refresh(&self.library, &self.config);
            }
            Message::FileDropped(path) => replays::add_files(self, vec![path]),
            Message::Replays(message) => return replays::update(self, message),
            Message::Setup(message) => return setup::update(self, message),
        }
        Task::none()
    }

    pub fn view(&self) -> Element<'_, Message> {
        let nav = Page::ALL.map(|page| {
            let style = if page == self.page {
                button::primary
            } else {
                button::text
            };
            button(page.label())
                .width(Fill)
                .style(style)
                .on_press(Message::Navigate(page))
                .into()
        });
        let sidebar = column(nav).spacing(4).padding(8).width(160);
        let page = match self.page {
            Page::Replays => replays::view(self),
            Page::Setup => setup::view(self),
            other => text(other.label()).size(24).into(),
        };
        let mut content = column![page].spacing(8);
        if let Some(notice) = &self.notice {
            content = content.push(text(notice).size(14));
        }

        row![
            sidebar,
            container(content).padding(16).width(Fill).height(Fill)
        ]
        .into()
    }

    pub fn theme(&self) -> Theme {
        Theme::Dark
    }

    pub fn subscription(&self) -> Subscription<Message> {
        event::listen_with(|event, _, _| match event {
            iced::Event::Window(window::Event::FileDropped(path)) => {
                Some(Message::FileDropped(path))
            }
            _ => None,
        })
    }

    pub fn save_config(&mut self) {
        if let Some(Err(error)) = config_path().map(|path| self.config.save(&path)) {
            self.notice = Some(format!("{error:#}"));
        }
    }

    pub fn reload_otd(&mut self) {
        self.otd = match self.config.otd_dir() {
            Some(dir) => OtdSettings::load(&dir).map_err(|error| format!("{error:#}")),
            None => Err("OpenTabletDriver settings not found; choose its folder.".to_string()),
        };
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
                while let Some(event) = events.next().await {
                    if output.send(event).await.is_err() {
                        break;
                    }
                }
            }),
            Message::Scan,
        )
    }
}

fn config_path() -> Option<PathBuf> {
    storage::app_dir().map(|dir| dir.join("config.json"))
}

fn cache_path() -> Option<PathBuf> {
    storage::app_dir().map(|dir| dir.join("library.json"))
}
