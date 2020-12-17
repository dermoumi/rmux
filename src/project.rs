use crate::command::de_command_list;
use crate::config::Config;
use crate::project_template::ProjectTemplate;
use crate::startup_window::StartupWindow;
use crate::utils::{parse_command, valid_tmux_identifier};
use crate::window::Window;
use crate::working_dir::de_working_dir;

use serde::{de, Deserialize, Serialize};
use shell_words::{quote, split};

use std::error::Error;
use std::ffi::OsString;
use std::path::PathBuf;

#[derive(Serialize, Debug, PartialEq, Clone)]
pub struct Project {
    pub session_name: Option<String>,
    pub tmux_command: Option<String>,
    pub tmux_options: Option<String>,
    pub tmux_socket: Option<String>,
    pub working_dir: Option<PathBuf>,
    pub window_base_index: usize,
    pub pane_base_index: usize,
    pub startup_window: StartupWindow,
    pub startup_pane: Option<usize>,
    pub on_start: Vec<String>,
    pub on_first_start: Vec<String>,
    pub on_restart: Vec<String>,
    pub on_exit: Vec<String>,
    pub on_stop: Vec<String>,
    pub on_create: Vec<String>,
    pub post_create: Vec<String>,
    pub on_pane_create: Vec<String>,
    pub post_pane_create: Vec<String>,
    pub pane_commands: Vec<String>,
    pub attach: bool,
    pub template: ProjectTemplate,
    pub windows: Vec<Window>,
}

impl Project {
    pub fn prepare(self, config: &Config, project_name: &str, force_attach: Option<bool>) -> Self {
        let mut project = Self {
            session_name: self.session_name.or(Some(project_name.into())),
            ..self
        };

        if let Some(attach) = force_attach {
            project.attach = attach;
        }

        if let Some(tmux_command) = &config.tmux_command {
            project.tmux_command = Some(tmux_command.to_string_lossy().into());
        } else if project.tmux_command.is_none() {
            project.tmux_command = Some("tmux".into());
        }

        project
    }

    pub fn check(&self) -> Result<(), Box<dyn Error>> {
        // Make sure session name is valid
        if let Some(session_name) = &self.session_name {
            valid_tmux_identifier(session_name)?;
        }

        // Make sure start up window exists
        match &self.startup_window {
            StartupWindow::Index(index) => {
                if *index >= self.window_base_index + self.windows.len()
                    || *index < self.window_base_index
                {
                    Err(format!(
                        "startup_window: there is no window with index {}",
                        index
                    ))?;
                }
            }
            StartupWindow::Name(name) => {
                if self
                    .windows
                    .iter()
                    .find(|w| match &w.name {
                        Some(window_name) => window_name == name,
                        _ => false,
                    })
                    .is_none()
                {
                    Err(format!(
                        "startup_window: there is no window with name {:?}",
                        name
                    ))?;
                }
            }
            _ => {}
        }

        // Make sure working_dir exists and is a directory
        if let Some(path) = &self.working_dir {
            if !path.is_dir() {
                Err(format!(
                    "project working_dir {:?} is not a directory or does not exist",
                    path
                ))?;
            }
        }

        // Run checks for each window
        self.windows
            .iter()
            .map(|w| w.check())
            .collect::<Result<_, _>>()
    }

    // Separates tmux_command into the command itself + an array of arguments
    // The arguments are then merged with the passed arguments
    // Also appends tmux_socket and tmux_options as arguments while at it
    pub fn get_tmux_command(
        &self,
        args: Vec<OsString>,
    ) -> Result<(OsString, Vec<OsString>), Box<dyn Error>> {
        let command = OsString::from(self.tmux_command.as_ref().ok_or("tmux command not set")?);

        // Build tmux_socket arguments
        let socket_args: Vec<OsString> = match &self.tmux_socket {
            Some(tmux_socket) => vec![OsString::from("-L"), OsString::from(tmux_socket)],
            None => vec![],
        };

        // Convert tmux_options ot OsString
        let mut extra_args: Vec<OsString> = match &self.tmux_options {
            Some(tmux_options) => split(&tmux_options)?
                .into_iter()
                .map(|o| OsString::from(o))
                .collect(),
            None => vec![],
        };

        // Append all args together
        let mut full_args = socket_args;
        full_args.append(&mut extra_args);
        full_args.append(&mut args.to_owned());

        // Use utiliy to split command and append args to the split arguments
        parse_command(&command, &full_args)
    }

    // Sanitizes tmux_command for use in the template file
    pub fn get_tmux_command_for_template(&self) -> Result<String, Box<dyn Error>> {
        let (command, args) = self.get_tmux_command(vec![])?;

        Ok(vec![command.to_string_lossy().into()]
            .into_iter()
            .chain(
                args.into_iter()
                    .map(|s| quote(&String::from(s.to_string_lossy())).into()),
            )
            .collect::<Vec<String>>()
            .join(" "))
    }

    fn default_window_base_index() -> usize {
        1
    }

    fn default_pane_base_index() -> usize {
        1
    }

    fn default_windows() -> Vec<Window> {
        vec![Window::default()]
    }

    fn default_attach() -> bool {
        true
    }

    fn de_window_base_index<'de, D>(deserializer: D) -> Result<usize, D::Error>
    where
        D: de::Deserializer<'de>,
    {
        let opt: Option<usize> = de::Deserialize::deserialize(deserializer)?;
        Ok(opt.unwrap_or(Self::default_window_base_index()))
    }

    fn de_pane_base_index<'de, D>(deserializer: D) -> Result<usize, D::Error>
    where
        D: de::Deserializer<'de>,
    {
        let opt: Option<usize> = de::Deserialize::deserialize(deserializer)?;
        Ok(opt.unwrap_or(Self::default_pane_base_index()))
    }

    fn de_windows<'de, D>(deserializer: D) -> Result<Vec<Window>, D::Error>
    where
        D: de::Deserializer<'de>,
    {
        #[derive(Deserialize, Debug)]
        #[serde(untagged)]
        enum WindowList {
            Empty,
            List(Vec<Window>),
            Single(Window),
        };

        let window_list: WindowList = de::Deserialize::deserialize(deserializer)?;

        Ok(match window_list {
            WindowList::List(windows) => windows,
            WindowList::Single(window) => vec![window],
            WindowList::Empty => Self::default_windows(),
        })
    }
}

impl Default for Project {
    fn default() -> Self {
        Self {
            session_name: None,
            tmux_command: None,
            tmux_options: None,
            tmux_socket: None,
            working_dir: None,
            window_base_index: Self::default_window_base_index(),
            pane_base_index: Self::default_pane_base_index(),
            startup_window: StartupWindow::default(),
            startup_pane: None,
            on_start: vec![],
            on_first_start: vec![],
            on_restart: vec![],
            on_exit: vec![],
            on_stop: vec![],
            on_create: vec![],
            post_create: vec![],
            on_pane_create: vec![],
            post_pane_create: vec![],
            pane_commands: vec![],
            attach: true,
            template: ProjectTemplate::default(),
            windows: Self::default_windows(),
        }
    }
}

impl From<Option<Project>> for Project {
    fn from(project: Option<Project>) -> Self {
        project.unwrap_or_default()
    }
}

impl<'de> Deserialize<'de> for Project {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: de::Deserializer<'de>,
    {
        #[derive(Deserialize, Debug)]
        #[serde(deny_unknown_fields)]
        struct ProjectProxy {
            #[serde(default, alias = "name")]
            session_name: Option<String>,
            #[serde(default)]
            tmux_command: Option<String>,
            #[serde(default)]
            tmux_options: Option<String>,
            #[serde(default, alias = "socket_name")]
            tmux_socket: Option<String>,
            #[serde(default, alias = "root", deserialize_with = "de_working_dir")]
            working_dir: Option<PathBuf>,
            #[serde(
                default = "Project::default_window_base_index",
                deserialize_with = "Project::de_window_base_index"
            )]
            window_base_index: usize,
            #[serde(
                default = "Project::default_pane_base_index",
                deserialize_with = "Project::de_pane_base_index"
            )]
            pane_base_index: usize,
            #[serde(default)]
            startup_window: StartupWindow,
            #[serde(default)]
            startup_pane: Option<usize>,
            #[serde(
                default,
                alias = "on_project_start",
                deserialize_with = "de_command_list"
            )]
            on_start: Vec<String>,
            #[serde(
                default,
                alias = "on_project_first_start",
                deserialize_with = "de_command_list"
            )]
            on_first_start: Vec<String>,
            #[serde(
                default,
                alias = "on_project_restart",
                deserialize_with = "de_command_list"
            )]
            on_restart: Vec<String>,
            #[serde(
                default,
                alias = "on_project_exit",
                deserialize_with = "de_command_list"
            )]
            on_exit: Vec<String>,
            #[serde(
                default,
                alias = "on_project_stop",
                deserialize_with = "de_command_list"
            )]
            on_stop: Vec<String>,
            #[serde(default, deserialize_with = "de_command_list")]
            on_create: Vec<String>,
            #[serde(default, deserialize_with = "de_command_list")]
            post_create: Vec<String>,
            #[serde(default, deserialize_with = "de_command_list")]
            on_pane_create: Vec<String>,
            #[serde(default, deserialize_with = "de_command_list")]
            post_pane_create: Vec<String>,
            #[serde(
                default,
                alias = "pre_window",
                alias = "pane_command",
                deserialize_with = "de_command_list"
            )]
            pane_commands: Vec<String>,
            #[serde(default, alias = "tmux_attached")]
            attach: Option<bool>,
            #[serde(default, alias = "tmux_detached")]
            detached: Option<bool>,
            #[serde(default)]
            template: ProjectTemplate,
            #[serde(
                default = "Project::default_windows",
                alias = "window",
                deserialize_with = "Project::de_windows"
            )]
            windows: Vec<Window>,
        }

        let opt: Option<ProjectProxy> = de::Deserialize::deserialize(deserializer)?;

        Ok(match opt {
            None => Self::default(),
            Some(project) => {
                let attach = match project.attach {
                    Some(attach) => match project.detached {
                        Some(_) => Err(de::Error::custom(
                            "cannot set both 'attach' and 'detached' fields",
                        ))?,
                        None => attach,
                    },
                    None => match project.detached {
                        Some(detached) => !detached,
                        None => Self::default_attach(),
                    },
                };

                Self {
                    session_name: project.session_name,
                    tmux_command: project.tmux_command,
                    tmux_options: project.tmux_options,
                    tmux_socket: project.tmux_socket,
                    working_dir: project.working_dir,
                    window_base_index: project.window_base_index,
                    pane_base_index: project.pane_base_index,
                    startup_window: project.startup_window,
                    startup_pane: project.startup_pane,
                    on_start: project.on_start,
                    on_first_start: project.on_first_start,
                    on_restart: project.on_restart,
                    on_exit: project.on_exit,
                    on_stop: project.on_stop,
                    on_create: project.on_create,
                    post_create: project.post_create,
                    on_pane_create: project.on_pane_create,
                    post_pane_create: project.post_pane_create,
                    pane_commands: project.pane_commands,
                    attach,
                    template: project.template,
                    windows: project.windows,
                }
            }
        })
    }
}

#[cfg(test)]
#[path = "test/project.rs"]
mod tests;