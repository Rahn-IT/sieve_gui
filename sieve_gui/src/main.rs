// #![windows_subsystem = "windows"]

use iced::application;

use crate::ui::UIWrapper;

mod ui;

fn main() {
    application(UIWrapper::start, UIWrapper::update, UIWrapper::view)
        .subscription(UIWrapper::subscription)
        .run()
        .unwrap();
}
