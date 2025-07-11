use std::sync::Arc;

use iced::{
    Subscription, Task,
    keyboard::{self, key::Named},
    widget::{center, focus_next, text},
};
use sieve_client::SieveClient;
use sqlx::{Sqlite, SqlitePool, migrate::MigrateDatabase};
use tokio::fs::create_dir_all;

use crate::ui::{account_select::AccountSelect, add_account::AddAccount, manage::Manage};

mod account_select;
mod add_account;
mod manage;

#[derive(Debug, Clone)]
pub enum MessageWrapper {
    Ui(Message),
    Error(String),
    Pool(Arc<SqlitePool>),
    Tab,
}

enum WrapperScreen {
    Loading,
    Error(String),
    Ui(UI),
}

pub struct UIWrapper {
    screen: WrapperScreen,
}

impl UIWrapper {
    pub fn start() -> (Self, Task<MessageWrapper>) {
        (
            Self {
                screen: WrapperScreen::Loading,
            },
            Task::future(async {
                if let Some(dirs) =
                    directories_next::ProjectDirs::from("de", "it-rahn", "sieve-gui")
                {
                    let data_dir = dirs.data_dir();

                    if let Err(err) = create_dir_all(data_dir).await {
                        return MessageWrapper::Error(format!(
                            "Failed to create data directory: {}",
                            err
                        ));
                    }

                    let mut db_path = data_dir.to_path_buf();
                    db_path.push("sieve_accounts.sqlite");

                    match tokio::fs::try_exists(&db_path).await {
                        Err(err) => {
                            return MessageWrapper::Error(format!(
                                "Failed to check if database exists: {}",
                                err
                            ));
                        }
                        Ok(false) => {
                            match Sqlite::create_database(db_path.to_string_lossy().as_ref()).await
                            {
                                Err(err) => {
                                    return MessageWrapper::Error(format!(
                                        "Failed to create database directory: {}",
                                        err
                                    ));
                                }
                                Ok(_) => {}
                            }
                        }
                        Ok(true) => {}
                    }

                    match SqlitePool::connect(db_path.as_os_str().to_string_lossy().as_ref()).await
                    {
                        Err(err) => {
                            MessageWrapper::Error(format!("Failed to connect to database: {}", err))
                        }
                        Ok(pool) => {
                            if let Err(err) = sqlx::migrate!("./migrations").run(&pool).await {
                                return MessageWrapper::Error(format!(
                                    "Failed to run migrations: {}",
                                    err
                                ));
                            }

                            MessageWrapper::Pool(Arc::new(pool))
                        }
                    }
                } else {
                    MessageWrapper::Error("Failed to get data directory".to_string())
                }
            }),
        )
    }

    pub fn update(&mut self, message: MessageWrapper) -> Task<MessageWrapper> {
        match message {
            MessageWrapper::Error(error) => {
                self.screen = WrapperScreen::Error(error);
                Task::none()
            }
            MessageWrapper::Pool(pool) => {
                if let Some(pool) = Arc::into_inner(pool) {
                    let (ui, task) = UI::new(pool);
                    self.screen = WrapperScreen::Ui(ui);
                    task.map(MessageWrapper::Ui)
                } else {
                    Task::none()
                }
            }
            MessageWrapper::Ui(message) => {
                if let WrapperScreen::Ui(ui) = &mut self.screen {
                    ui.update(message).map(MessageWrapper::Ui)
                } else {
                    Task::none()
                }
            }
            MessageWrapper::Tab => focus_next(),
        }
    }

    pub fn view(&self) -> iced::Element<MessageWrapper> {
        match &self.screen {
            WrapperScreen::Loading => center(text("Loading...")).into(),
            WrapperScreen::Error(error) => center(text(error)).into(),
            WrapperScreen::Ui(ui) => ui.view().map(MessageWrapper::Ui),
        }
    }

    pub fn subscription(&self) -> Subscription<MessageWrapper> {
        keyboard::on_key_press(|key, _modifiers| match key {
            keyboard::Key::Named(named) => match named {
                Named::Tab => Some(MessageWrapper::Tab),
                _ => None,
            },
            keyboard::Key::Character(_) => None,
            keyboard::Key::Unidentified => None,
        })
    }
}

#[derive(Debug, Clone)]
pub enum Message {
    AccountSelect(account_select::Message),
    AddAccount(add_account::Message),
    Manage(manage::Message),
}

pub enum Screen {
    AccountSelect(AccountSelect),
    AddAccount(AddAccount),
    Manage(Manage),
}

struct UI {
    pool: SqlitePool,
    screen: Screen,
}

impl UI {
    fn new(pool: SqlitePool) -> (Self, Task<Message>) {
        let (select, task) = AccountSelect::new(pool.clone());

        let ui = Self {
            pool,
            screen: Screen::AccountSelect(select),
        };
        (ui, task.map(Message::AccountSelect))
    }

    fn update(&mut self, message: Message) -> Task<Message> {
        match message {
            Message::AccountSelect(message) => {
                if let Screen::AccountSelect(select) = &mut self.screen {
                    match select.update(message) {
                        account_select::Action::None => Task::none(),
                        account_select::Action::Run(task) => task.map(Message::AccountSelect),
                        account_select::Action::Selected(client) => self.to_manage(client),
                        account_select::Action::AddAccount => {
                            let (add_account, task) = AddAccount::new(self.pool.clone());
                            self.screen = Screen::AddAccount(add_account);
                            task.map(Message::AddAccount)
                        }
                    }
                } else {
                    Task::none()
                }
            }
            Message::AddAccount(message) => {
                if let Screen::AddAccount(add_account) = &mut self.screen {
                    match add_account.update(message) {
                        add_account::Action::None => Task::none(),
                        add_account::Action::Back => self.to_account_select(),
                        add_account::Action::Added(client) => self.to_manage(client),
                        add_account::Action::Run(task) => task.map(Message::AddAccount),
                    }
                } else {
                    Task::none()
                }
            }
            Message::Manage(message) => {
                if let Screen::Manage(manage) = &mut self.screen {
                    match manage.update(message) {
                        manage::Action::None => Task::none(),
                    }
                } else {
                    Task::none()
                }
            }
        }
    }

    fn to_account_select(&mut self) -> Task<Message> {
        let (select, task) = AccountSelect::new(self.pool.clone());
        self.screen = Screen::AccountSelect(select);
        task.map(Message::AccountSelect)
    }

    fn to_manage(&mut self, client: SieveClient) -> Task<Message> {
        let (manage, task) = Manage::new(client);
        self.screen = Screen::Manage(manage);
        task.map(Message::Manage)
    }

    fn view(&self) -> iced::Element<Message> {
        match &self.screen {
            Screen::AccountSelect(select) => select.view().map(Message::AccountSelect),
            Screen::AddAccount(add_account) => add_account.view().map(Message::AddAccount),
            Screen::Manage(manage) => manage.view().map(Message::Manage),
        }
    }
}
