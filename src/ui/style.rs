use iced::border::{self, Border};
use iced::font::Weight;
use iced::theme::Palette;
use iced::widget::text::IntoFragment;
use iced::widget::{
    Button, Container, Rule, Scrollable, Text, button, column, container, pick_list, row, rule,
    scrollable, space, text, text_input,
};
use iced::{Background, Center, Color, Element, Fill, Font, Theme};

// Plum-tinted darks after osu!lazer's overlays, osu! pink as the one accent, and osu!'s
// hit-judgement colors (300, 100, 50, miss) as the tones for good, saved, worth fixing and broken.
pub const INK: Color = Color::from_rgb8(0x17, 0x13, 0x1A);
pub const PANEL: Color = Color::from_rgb8(0x21, 0x1B, 0x25);
pub const RAISED: Color = Color::from_rgb8(0x2C, 0x24, 0x31);
pub const LINE: Color = Color::from_rgb8(0x3A, 0x31, 0x40);
pub const TEXT: Color = Color::from_rgb8(0xF2, 0xEC, 0xF4);
pub const MUTED: Color = Color::from_rgb8(0xA3, 0x97, 0xA8);
pub const PINK: Color = Color::from_rgb8(0xFF, 0x66, 0xAB);
pub const GREAT: Color = Color::from_rgb8(0x66, 0xCC, 0xFF);
pub const OK: Color = Color::from_rgb8(0xB3, 0xD9, 0x44);
pub const MEH: Color = Color::from_rgb8(0xFF, 0xCC, 0x22);
pub const MISS: Color = Color::from_rgb8(0xFF, 0x4F, 0x64);

// Exo 2 was osu!lazer's first UI typeface.
pub const REGULAR: Font = Font::with_name("Exo 2");
pub const SEMIBOLD: Font = Font {
    weight: Weight::Semibold,
    ..REGULAR
};

const RADIUS: f32 = 8.0;
const PANEL_RADIUS: f32 = 12.0;

pub fn theme() -> Theme {
    Theme::custom(
        "osu! Tablet Optimizer",
        Palette {
            background: INK,
            text: TEXT,
            primary: PINK,
            success: OK,
            warning: MEH,
            danger: MISS,
        },
    )
}

// Title and purpose on the left, the page's actions on the right, then its content.
pub fn page<'a, M: 'a>(
    title: &'a str,
    about: impl IntoFragment<'a>,
    actions: impl Into<Element<'a, M>>,
    content: impl Into<Element<'a, M>>,
) -> Element<'a, M> {
    let header = row![
        column![text(title).size(28).font(SEMIBOLD), muted(about)].spacing(2),
        space::horizontal(),
        actions.into(),
    ]
    .spacing(12)
    .align_y(Center);
    column![header, content.into()].spacing(24).into()
}

pub fn panel<'a, M: 'a>(content: impl Into<Element<'a, M>>) -> Container<'a, M> {
    container(content)
        .padding(20)
        .width(Fill)
        .style(|_| container::Style {
            background: Some(PANEL.into()),
            border: Border {
                color: LINE,
                width: 1.0,
                radius: PANEL_RADIUS.into(),
            },
            ..container::Style::default()
        })
}

pub fn titled<'a, M: 'a>(title: &'a str, content: impl Into<Element<'a, M>>) -> Container<'a, M> {
    panel(column![heading(title), content.into()].spacing(14))
}

// A tinted strip for messages that need attention.
pub fn banner<'a, M: 'a>(tone: Color, content: impl Into<Element<'a, M>>) -> Container<'a, M> {
    container(
        row![dot(tone, 8.0), content.into()]
            .spacing(12)
            .align_y(Center),
    )
    .padding([10, 16])
    .width(Fill)
    .style(move |_| container::Style {
        background: Some(Color { a: 0.10, ..tone }.into()),
        border: Border {
            color: Color { a: 0.35, ..tone },
            width: 1.0,
            radius: RADIUS.into(),
        },
        text_color: Some(TEXT),
        ..container::Style::default()
    })
}

// A scroll area with a thin scrollbar kept clear of the content.
pub fn scroll<'a, M: 'a>(content: impl Into<Element<'a, M>>) -> Scrollable<'a, M> {
    scrollable(content).direction(scrollable::Direction::Vertical(
        thin_scrollbar().spacing(12),
    ))
}

// The scrollbar floats over the content's edge instead, so scrolled rows keep the width of a
// header row outside the scroll area.
pub fn overlay_scroll<'a, M: 'a>(content: impl Into<Element<'a, M>>) -> Scrollable<'a, M> {
    scrollable(content).direction(scrollable::Direction::Vertical(thin_scrollbar()))
}

fn thin_scrollbar() -> scrollable::Scrollbar {
    scrollable::Scrollbar::new().width(6).scroller_width(6)
}

pub fn heading<'a>(content: impl IntoFragment<'a>) -> Text<'a> {
    text(content).size(17).font(SEMIBOLD)
}

pub fn muted<'a>(content: impl IntoFragment<'a>) -> Text<'a> {
    text(content).size(13).color(MUTED)
}

pub fn dot<'a, M: 'a>(color: Color, size: f32) -> Element<'a, M> {
    circle(color, size, None)
}

// The approach circle: the app's mark for "this one".
pub fn ring<'a, M: 'a>(color: Color, size: f32) -> Element<'a, M> {
    circle(Color::TRANSPARENT, size, Some(color))
}

fn circle<'a, M: 'a>(fill: Color, size: f32, outline: Option<Color>) -> Element<'a, M> {
    container(space())
        .width(size)
        .height(size)
        .style(move |_| container::Style {
            background: Some(fill.into()),
            border: Border {
                color: outline.unwrap_or(fill),
                width: if outline.is_some() { 2.0 } else { 0.0 },
                radius: (size / 2.0).into(),
            },
            ..container::Style::default()
        })
        .into()
}

// Short solid and dashed lines for chart legends.
pub fn swatch<'a, M: 'a>(color: Color) -> Element<'a, M> {
    bar(color, 18.0)
}

pub fn dashed_swatch<'a, M: 'a>(color: Color) -> Element<'a, M> {
    row((0..3).map(|_| bar(color, 4.0))).spacing(3).into()
}

fn bar<'a, M: 'a>(color: Color, width: f32) -> Element<'a, M> {
    container(space())
        .width(width)
        .height(2)
        .style(move |_| container::Style::default().background(color))
        .into()
}

pub fn divider<'a>() -> Rule<'a> {
    rule::horizontal(1).style(line)
}

pub fn separator<'a>() -> Rule<'a> {
    rule::vertical(1).style(line)
}

fn line(_: &Theme) -> rule::Style {
    rule::Style {
        color: LINE,
        radius: 0.0.into(),
        fill_mode: rule::FillMode::Full,
        snap: true,
    }
}

pub fn primary<'a, M: 'a>(label: impl IntoFragment<'a>) -> Button<'a, M> {
    button(text(label).font(SEMIBOLD))
        .padding([8, 16])
        .style(|theme, status| button::Style {
            border: border::rounded(RADIUS),
            ..button::primary(theme, status)
        })
}

pub fn secondary<'a, M: 'a>(label: impl IntoFragment<'a>) -> Button<'a, M> {
    button(text(label)).padding([8, 16]).style(|_, status| {
        let (background, text_color) = match status {
            button::Status::Active => (RAISED, TEXT),
            button::Status::Hovered | button::Status::Pressed => (LINE, TEXT),
            button::Status::Disabled => (PANEL, MUTED),
        };
        button::Style {
            background: Some(background.into()),
            text_color,
            border: Border {
                color: LINE,
                width: 1.0,
                radius: RADIUS.into(),
            },
            ..button::Style::default()
        }
    })
}

pub fn nav(active: bool) -> impl Fn(&Theme, button::Status) -> button::Style {
    move |_, status| {
        let hovered = matches!(status, button::Status::Hovered | button::Status::Pressed);
        button::Style {
            background: (active || hovered).then_some(Background::Color(RAISED)),
            text_color: if active || hovered { TEXT } else { MUTED },
            border: border::rounded(RADIUS),
            ..button::Style::default()
        }
    }
}

pub fn input(theme: &Theme, status: text_input::Status) -> text_input::Style {
    let edge = match status {
        text_input::Status::Focused { .. } => PINK,
        text_input::Status::Hovered => MUTED,
        text_input::Status::Active | text_input::Status::Disabled => LINE,
    };
    text_input::Style {
        background: INK.into(),
        border: Border {
            color: edge,
            width: 1.0,
            radius: RADIUS.into(),
        },
        placeholder: MUTED,
        ..text_input::default(theme, status)
    }
}

pub fn select(theme: &Theme, status: pick_list::Status) -> pick_list::Style {
    let edge = match status {
        pick_list::Status::Opened { .. } => PINK,
        pick_list::Status::Hovered => MUTED,
        pick_list::Status::Active => LINE,
    };
    pick_list::Style {
        background: INK.into(),
        border: Border {
            color: edge,
            width: 1.0,
            radius: RADIUS.into(),
        },
        placeholder_color: MUTED,
        handle_color: MUTED,
        ..pick_list::default(theme, status)
    }
}
