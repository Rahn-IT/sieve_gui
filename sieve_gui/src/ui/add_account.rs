use std::sync::Arc;

use iced::{
    Element, Task,
    widget::{button, center, column, horizontal_space, row, text, text_input, vertical_space},
};
use sieve_client::SieveClient;
use sqlx::SqlitePool;

#[derive(Debug, Clone)]
pub enum Message {
    Server(String),
    Username(String),
    Password(String),
    Error(String),
    AccountAdded(Arc<SieveClient>),
    Back,
    Add,
}

pub enum Action {
    None,
    Run(Task<Message>),
    Added(SieveClient),
    Back,
}

#[derive(PartialEq)]
pub enum State {
    Input,
    Connecting,
    Error(String),
}

pub struct AddAccount {
    pool: SqlitePool,
    state: State,
    server: String,
    username: String,
    password: String,
}

impl AddAccount {
    pub fn new(pool: SqlitePool) -> (Self, Task<Message>) {
        (
            Self {
                pool,
                state: State::Input,
                server: String::new(),
                username: String::new(),
                password: String::new(),
            },
            text_input::focus("server"),
        )
    }

    pub fn update(&mut self, message: Message) -> Action {
        match message {
            Message::Server(server) => {
                self.server = server;
                Action::None
            }
            Message::Username(username) => {
                self.username = username;
                Action::None
            }
            Message::Password(password) => {
                self.password = password;
                Action::None
            }
            Message::Add => {
                if self.state == State::Connecting {
                    return Action::None;
                }
                self.state = State::Connecting;
                Action::Run(self.add_account())
            }
            Message::Error(err) => {
                self.state = State::Error(err);

                Action::None
            }
            Message::AccountAdded(client) => {
                if let Some(client) = Arc::into_inner(client) {
                    Action::Added(client)
                } else {
                    Action::None
                }
            }
            Message::Back => match &self.state {
                State::Input => Action::Back,
                State::Connecting => Action::None,
                State::Error(_) => {
                    self.state = State::Input;
                    Action::None
                }
            },
        }
    }

    pub fn view(&self) -> Element<Message> {
        match &self.state {
            State::Input => column![
                text_input("Server", &self.server)
                    .on_input(Message::Server)
                    .id("server"),
                text_input("Username", &self.username).on_input(Message::Username),
                text_input("Password", &self.password)
                    .secure(true)
                    .on_input(Message::Password)
                    .on_submit_maybe(if self.is_valid() {
                        Some(Message::Add)
                    } else {
                        None
                    }),
                vertical_space(),
                row![
                    horizontal_space(),
                    button(text("Back").center())
                        .on_press(Message::Back)
                        .width(100),
                    button(text("Add").center())
                        .on_press_maybe(if self.is_valid() {
                            Some(Message::Add)
                        } else {
                            None
                        })
                        .width(100)
                ]
                .spacing(20)
            ]
            .padding(50)
            .spacing(20)
            .into(),
            State::Connecting => center(text("Connecting...")).into(),
            State::Error(err) => column![
                center(text(format!("Error: {}", err))),
                row![
                    horizontal_space(),
                    button(text("Back")).on_press(Message::Back)
                ]
            ]
            .padding(50)
            .spacing(20)
            .into(),
        }
    }

    fn add_account(&mut self) -> Task<Message> {
        let server = self.server.clone();
        let username = self.username.clone();
        let password = self.password.clone();
        let pool = self.pool.clone();
        Task::future(async move {
            match SieveClient::connect(server.clone(), 4190, &username, &password).await {
                Ok(client) => {
                    match sqlx::query!(
                        "INSERT INTO accounts (server, username, password) VALUES (?, ?, ?)",
                        server,
                        username,
                        password
                    )
                    .execute(&pool)
                    .await
                    {
                        Ok(_) => Message::AccountAdded(Arc::new(client)),
                        Err(err) => Message::Error(err.to_string()),
                    }
                }
                Err(err) => Message::Error(err.to_string()),
            }
        })
    }

    fn is_valid(&self) -> bool {
        !self.server.is_empty() && !self.username.is_empty() && !self.password.is_empty()
    }
}
