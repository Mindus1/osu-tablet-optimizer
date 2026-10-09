#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod app;
mod ui;

use app::App;

fn main() -> iced::Result {
    iced::application(App::new, App::update, App::view)
        .title("osu! Tablet Optimizer")
        .theme(App::theme)
        .subscription(App::subscription)
        .window_size((1200.0, 800.0))
        .run()
}
