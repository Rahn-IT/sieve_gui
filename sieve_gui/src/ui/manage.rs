use iced::{
    Task,
    widget::{center, text},
};
use sieve_client::SieveClient;

#[derive(Debug, Clone)]
pub enum Message {}

pub enum Action {
    None,
}

pub struct Manage {
    client: SieveClient,
}

impl Manage {
    pub fn new(client: SieveClient) -> (Self, Task<Message>) {
        (Self { client }, Task::none())
    }

    pub fn update(&mut self, message: Message) -> Action {
        Action::None
    }

    pub fn view(&self) -> iced::Element<Message> {
        center(text("Manage")).into()
    }
}
