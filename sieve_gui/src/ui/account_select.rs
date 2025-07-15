use std::{collections::HashMap, fmt::Debug, sync::Arc};

use iced::{
    Element, Length, Task,
    widget::{button, center, column, horizontal_space, row, scrollable, text},
};
use sieve_client::SieveClient;
use sqlx::SqlitePool;

#[derive(Debug, Clone)]
pub enum Message {
    Error(String),
    Accounts(Vec<Account>),
    Select(i64),
    Delete(i64),
    ConfirmDelete,
    Back,
    AddAccount,
    Opened(Arc<SieveClient>),
}

pub enum Action {
    None,
    Selected(Arc<SieveClient>),
    AddAccount,
    Run(Task<Message>),
}

pub struct AccountSelect {
    pool: SqlitePool,
    error: Option<String>,
    accounts: HashMap<i64, Account>,
    confirm_delete: Option<i64>,
}

#[derive(Clone)]
pub struct Account {
    id: i64,
    server: String,
    username: String,
    password: String,
}

impl Debug for Account {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Account")
            .field("server", &self.server)
            .field("username", &self.username)
            .finish()
    }
}

impl AccountSelect {
    pub fn new(pool: SqlitePool) -> (Self, Task<Message>) {
        let self_ = Self {
            pool,
            error: None,
            accounts: HashMap::new(),
            confirm_delete: None,
        };
        let task = self_.update_profiles();
        (self_, task)
    }

    pub fn update(&mut self, message: Message) -> Action {
        match message {
            Message::Error(err) => {
                self.error = Some(err);
                Action::None
            }
            Message::Accounts(accounts) => {
                self.error = None;
                if accounts.is_empty() {
                    Action::AddAccount
                } else {
                    self.accounts = accounts
                        .into_iter()
                        .map(|account| (account.id, account))
                        .collect();
                    Action::None
                }
            }
            Message::Delete(id) => {
                self.confirm_delete = Some(id);
                Action::None
            }
            Message::ConfirmDelete => {
                if let Some(id) = self.confirm_delete.take() {
                    Action::Run(self.delete_account(id))
                } else {
                    Action::None
                }
            }
            Message::Back => {
                self.error = None;
                self.confirm_delete = None;
                Action::None
            }
            Message::Select(id) => Action::Run(self.open_account(id)),
            Message::Opened(client) => Action::Selected(client),
            Message::AddAccount => Action::AddAccount,
        }
    }

    fn update_profiles(&self) -> Task<Message> {
        let pool = self.pool.clone();
        Task::future(async move {
            match sqlx::query_as!(
                Account,
                "SELECT id, server, username, password FROM accounts"
            )
            .fetch_all(&pool)
            .await
            {
                Ok(accounts) => Message::Accounts(accounts),
                Err(err) => Message::Error(err.to_string()),
            }
        })
    }

    pub fn view(&self) -> Element<Message> {
        if let Some(err) = &self.error {
            return center(
                column![
                    text("Error"),
                    text(format!("Error: {}", err)),
                    button(text("OK")).on_press(Message::Back)
                ]
                .spacing(10),
            )
            .into();
        }

        if let Some(id) = &self.confirm_delete {
            if let Some(account) = self.accounts.get(&id) {
                return center(
                    column![
                        text("Are you sure you want to delete this account?"),
                        text(format!("Server: {}", account.server)),
                        text(format!("Username: {}", account.username)),
                        row![
                            button(text("Yes")).on_press(Message::ConfirmDelete),
                            button(text("No")).on_press(Message::Back)
                        ]
                        .spacing(10),
                    ]
                    .spacing(10),
                )
                .into();
            }
        }

        column![
            scrollable(
                column(self.accounts.iter().map(|(_, account)| {
                    row![
                        button(text(&account.username))
                            .width(Length::Fill)
                            .on_press(Message::Select(account.id)),
                        button(text("Delete")).on_press(Message::Delete(account.id))
                    ]
                    .spacing(5)
                    .into()
                }))
                .spacing(10),
            )
            .height(Length::Fill),
            row![
                horizontal_space(),
                button(text("Add")).on_press(Message::AddAccount)
            ]
        ]
        .spacing(20)
        .padding(50)
        .into()
    }

    fn delete_account(&self, id: i64) -> Task<Message> {
        let pool = self.pool.clone();
        Task::future(async move {
            match sqlx::query!("DELETE FROM accounts WHERE id = $1", id)
                .execute(&pool)
                .await
            {
                Ok(_) => Message::Delete(id),
                Err(err) => Message::Error(err.to_string()),
            }
        })
        .chain(self.update_profiles())
    }

    fn open_account(&self, id: i64) -> Task<Message> {
        if let Some(account) = self.accounts.get(&id).cloned() {
            let account = account.clone();
            Task::future(async move {
                match SieveClient::connect(
                    account.server,
                    4190,
                    &account.username,
                    &account.password,
                )
                .await
                {
                    Ok(client) => Message::Opened(Arc::new(client)),
                    Err(err) => Message::Error(err.to_string()),
                }
            })
        } else {
            Task::done(Message::Error("Account not found".to_string()))
        }
    }
}
