use iced::widget::{button, column, container, row, text};
use iced::{Element, Fill, Theme};

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
}

#[derive(Default)]
pub struct App {
    page: Page,
}

impl App {
    pub fn update(&mut self, message: Message) {
        match message {
            Message::Navigate(page) => self.page = page,
        }
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
        let content = container(text(self.page.label()).size(24))
            .padding(16)
            .width(Fill)
            .height(Fill);

        row![sidebar, content].into()
    }

    pub fn theme(&self) -> Theme {
        Theme::Dark
    }
}
