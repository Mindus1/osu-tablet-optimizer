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
            size: Size::new(1280.0, 760.0),
            // Fits a 1080p screen at 150% scaling (1280 x 720 points, less taskbar and title bar)
            // while the Replays table keeps room for map titles.
            min_size: Some(Size::new(1180.0, 640.0)),
            ..window::Settings::default()
        })
        .run()
}
