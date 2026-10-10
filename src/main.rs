#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod app;
mod ui;

use app::App;
use iced::{Size, window};

fn main() -> iced::Result {
    iced::application(App::new, App::update, App::view)
        .title("osu! Tablet Optimizer")
        .theme(App::theme)
        .subscription(App::subscription)
        .font(include_bytes!("../assets/fonts/Exo2-Regular.ttf").as_slice())
        .font(include_bytes!("../assets/fonts/Exo2-SemiBold.ttf").as_slice())
        .default_font(ui::style::REGULAR)
        .window(window::Settings {
            // Both fit a 1080p screen at 150% scaling (1280 x 720 points, less taskbar and title
            // bar); the minimum still leaves the Replays table room for map titles.
            size: Size::new(1240.0, 640.0),
            min_size: Some(Size::new(1180.0, 640.0)),
            ..window::Settings::default()
        })
        .run()
}
