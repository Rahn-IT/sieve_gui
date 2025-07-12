use std::sync::Arc;

use iced::{
    Element, Length, Task,
    widget::{Container, button, center, column, container, row, scrollable, text},
};
use sieve_client::SieveClient;

#[derive(Debug, Clone)]
pub enum Message {
    Back,
    RefreshScripts,
    ScriptsLoaded(Result<Vec<(String, bool)>, String>),
    ScriptSelected(String),
    ScriptContentLoaded(Result<String, String>),
}

pub enum Action {
    None,
    Back,
    Run(Task<Message>),
}

#[derive(Debug, Clone)]
struct ScriptInfo {
    name: String,
    is_active: bool,
}

pub struct Manage {
    client: Arc<SieveClient>,
    scripts: Option<Vec<ScriptInfo>>,
    selected_script: Option<String>,
    script_content: Option<String>,
    error_message: Option<String>,
}

impl Manage {
    pub fn new(client: Arc<SieveClient>) -> (Self, Task<Message>) {
        let manage = Self {
            client: client.clone(),
            scripts: None,
            selected_script: None,
            script_content: None,
            error_message: None,
        };

        let task = manage.refresh_scripts();

        (manage, task)
    }

    pub fn update(&mut self, message: Message) -> Action {
        match message {
            Message::ScriptsLoaded(result) => {
                match result {
                    Ok(scripts) => {
                        self.scripts = Some(
                            scripts
                                .into_iter()
                                .map(|(name, is_active)| ScriptInfo { name, is_active })
                                .collect(),
                        );
                        self.error_message = None;
                    }
                    Err(err) => {
                        self.error_message = Some(err);
                        self.scripts = Some(Vec::new());
                    }
                }
                Action::None
            }
            Message::ScriptSelected(script_name) => {
                if self.selected_script.as_ref() != Some(&script_name) {
                    self.selected_script = Some(script_name.clone());
                    self.script_content = None;
                    self.error_message = None;

                    Action::Run(self.load_script_content(script_name))
                } else {
                    Action::None
                }
            }
            Message::ScriptContentLoaded(result) => {
                match result {
                    Ok(content) => {
                        self.script_content = Some(content);
                    }
                    Err(err) => {
                        self.error_message = Some(err);
                        self.script_content = None;
                    }
                }

                Action::None
            }
            Message::RefreshScripts => {
                self.scripts = None;
                self.error_message = None;

                Action::Run(self.refresh_scripts())
            }
            Message::Back => Action::Back,
        }
    }

    pub fn view(&self) -> Element<Message> {
        let left_panel = self.view_script_list();
        let right_panel = self.view_script_content();

        row![left_panel, right_panel]
            .spacing(10)
            .height(Length::Fill)
            .padding(10)
            .into()
    }

    fn view_script_list(&self) -> Container<Message> {
        // Header with refresh button
        let header = row![
            button("Back").on_press(Message::Back),
            text("Scripts").size(20),
            button("Refresh").on_press(Message::RefreshScripts)
        ]
        .spacing(15);

        let main_content: Element<Message> = match &self.scripts {
            None => text("Loading scripts...").size(14).into(),
            Some(scripts) => {
                if scripts.is_empty() {
                    text("No scripts found").size(14).into()
                } else {
                    column(scripts.iter().map(|script| {
                        let is_selected = self.selected_script.as_ref() == Some(&script.name);

                        let script_text = if script.is_active {
                            format!("● {} (active)", script.name)
                        } else {
                            script.name.clone()
                        };

                        let script_button = button(text(script_text).size(14))
                            .width(Length::Fill)
                            .padding([8, 12])
                            .on_press(Message::ScriptSelected(script.name.clone()));

                        if is_selected {
                            script_button.style(button::primary).into()
                        } else {
                            script_button.style(button::text).into()
                        }
                    }))
                    .into()
                }
            }
        };

        let content = column![header, main_content].spacing(10);

        container(content)
            .width(350)
            .height(Length::Fill)
            .padding(15)
            .style(container::rounded_box)
    }

    fn view_script_content(&self) -> Container<Message> {
        let content: Element<Message> = if let Some(err) = &self.error_message {
            text(format!("Error: {}", err)).size(14).into()
        } else if let Some(script_name) = &self.selected_script {
            // Header
            let header = text(format!("Script: {}", script_name)).size(20);

            // Content
            let content_display: Element<Message> = match &self.script_content {
                None => text("No content available").size(14).into(),
                Some(content) => {
                    if content.is_empty() {
                        text("No content available").size(14).into()
                    } else {
                        scrollable(text(content).font(iced::Font::MONOSPACE).size(13))
                            .width(Length::Fill)
                            .height(Length::Fill)
                            .into()
                    }
                }
            };

            column![header, content_display].spacing(10).into()
        } else {
            // No script selected

            center(text("Select a script from the list to view its content").size(16)).into()
        };

        container(content)
            .padding(15)
            .width(Length::Fill)
            .height(Length::Fill)
            .style(container::rounded_box)
    }

    // Helper method to get a task for loading script content
    fn load_script_content(&self, script_name: String) -> Task<Message> {
        let client = self.client.clone();
        Task::future(async move {
            match client.get_script(&script_name).await {
                Ok(content) => Message::ScriptContentLoaded(Ok(content)),
                Err(e) => Message::ScriptContentLoaded(Err(format!(
                    "Failed to load script '{}': {}",
                    script_name, e
                ))),
            }
        })
    }

    // Helper method to get a task for refreshing scripts
    fn refresh_scripts(&self) -> Task<Message> {
        let client = self.client.clone();
        Task::future(async move {
            match client.list_scripts().await {
                Ok(scripts) => Message::ScriptsLoaded(Ok(scripts)),
                Err(e) => Message::ScriptsLoaded(Err(format!("Failed to load scripts: {}", e))),
            }
        })
    }
}
