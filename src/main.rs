#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod app;
mod ui;

use app::App;

fn main() -> iced::Result {
    iced::application(App::new, App::update, App::view)
        .title("osu! Tablet Optimizer")
        .theme(App::theme)
        .subscription(App::subscription)
        .font(include_bytes!("../assets/fonts/Exo2-Regular.ttf").as_slice())
        .font(include_bytes!("../assets/fonts/Exo2-SemiBold.ttf").as_slice())
        .default_font(ui::style::REGULAR)
        .window_size((1280.0, 760.0))
        .run()
}
