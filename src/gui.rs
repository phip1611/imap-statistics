// SPDX-License-Identifier: MIT OR Apache-2.0

//! A window to enter the email accounts and to generate the report, for
//! people who do not use a terminal.
//!
//! The work runs in a thread, which sends its progress over a channel, so
//! that the window stays responsive.

use std::fs;
use std::path::{self, Path, PathBuf};
use std::process;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver};
use std::thread;

use anyhow::Context as _;
use eframe::egui::{self, Color32, RichText};
use jiff::Timestamp;
use serde::Serialize;

use crate::config::Config;
use crate::fetch::Cancelled;
use crate::pdf::PDF_FILE;
use crate::report::REPORT_FILE;
use crate::run::{self, Outcome};

/// Port of IMAP over TLS, as in the config.
const DEFAULT_PORT: u16 = 993;
/// The data directory of a new config, relative to the config file, as in
/// the example config.
const DEFAULT_DATA_DIR: &str = "imap-statistics-data";
/// The number of senders and recipients in each list of top ones, as the
/// CLI's default.
const TOP: usize = 5;
/// IMAP servers of common providers, by the domain of the address, so that
/// most people need not know them. All use TLS on port 993.
const KNOWN_HOSTS: [(&str, &str); 10] = [
    ("gmail.com", "imap.gmail.com"),
    ("googlemail.com", "imap.gmail.com"),
    ("gmx.de", "imap.gmx.net"),
    ("gmx.net", "imap.gmx.net"),
    ("web.de", "imap.web.de"),
    ("icloud.com", "imap.mail.me.com"),
    ("me.com", "imap.mail.me.com"),
    ("yahoo.com", "imap.mail.yahoo.com"),
    ("posteo.de", "posteo.de"),
    ("mailbox.org", "imap.mailbox.org"),
];

/// Opens the window and returns when it is closed.
///
/// # Arguments
///
/// - `config`: The path of the config file. It need not exist yet.
pub fn run(config: PathBuf) -> anyhow::Result<()> {
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("Email statistics")
            .with_inner_size([640.0, 720.0])
            .with_min_inner_size([480.0, 480.0]),
        ..Default::default()
    };
    eframe::run_native(
        "imap-statistics",
        options,
        Box::new(|cc| {
            // Larger text than egui's default, which is small for many.
            cc.egui_ctx.set_zoom_factor(1.25);
            Ok(Box::new(App::new(config)))
        }),
    )
    .map_err(|error| anyhow::anyhow!("failed to open the window: {error}"))
}

/// An email account as the form shows it.
#[derive(Debug, Default, Serialize)]
struct Account {
    /// The name of the account's directory in the data directory. New
    /// accounts get one from their address.
    name: String,
    host: String,
    port: u16,
    username: String,
    password: String,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    addresses: Vec<String>,
    /// The other addresses as the user types them, separated by commas.
    #[serde(skip)]
    addresses_text: String,
}

/// The config file as the form shows it.
#[derive(Debug, Serialize)]
struct Form {
    /// The data directory, as in the file: relative to the config file, or
    /// absolute.
    data_dir: String,
    #[serde(rename = "source")]
    sources: Vec<Account>,
}

impl Form {
    /// Returns the form of a config file, or an empty form if there is
    /// none.
    ///
    /// # Arguments
    ///
    /// - `path`: The path of the config file.
    fn load(path: &Path) -> anyhow::Result<Self> {
        if !path.exists() {
            return Ok(Self::new());
        }
        let config = Config::load(path)?;
        let sources = config
            .sources
            .into_iter()
            .map(|source| Account {
                name: source.name,
                host: source.host,
                port: source.port,
                username: source.username,
                password: source.password.expose().to_owned(),
                addresses_text: source.addresses.join(", "),
                addresses: source.addresses,
            })
            .collect();
        // Loading joins a relative data directory with the directory of the
        // file, which saving must not do again.
        let base = path.parent().unwrap_or_else(|| Path::new(""));
        let data_dir = config
            .data_dir
            .strip_prefix(base)
            .unwrap_or(&config.data_dir);
        Ok(Self {
            data_dir: data_dir.to_string_lossy().into_owned(),
            sources,
        })
    }

    /// Returns the form of a new config file, with one empty account.
    fn new() -> Self {
        Self {
            data_dir: DEFAULT_DATA_DIR.to_owned(),
            sources: vec![Account::new()],
        }
    }

    /// Checks the form and writes it as config file, then returns the
    /// config.
    ///
    /// # Arguments
    ///
    /// - `path`: The path of the config file.
    fn save(&mut self, path: &Path) -> anyhow::Result<Config> {
        for index in 0..self.sources.len() {
            if self.sources[index].name.trim().is_empty() {
                self.sources[index].name = self.unique_name(index);
            }
            let account = &mut self.sources[index];
            account.addresses = account
                .addresses_text
                .split(',')
                .map(str::trim)
                .filter(|address| !address.is_empty())
                .map(str::to_owned)
                .collect();
        }
        let text = toml::to_string(self).context("failed to write config")?;
        let base = path.parent().unwrap_or_else(|| Path::new(""));
        let config = Config::parse(&text, base)?;
        fs::create_dir_all(base)
            .with_context(|| format!("failed to create {}", base.display()))?;
        fs::write(path, text)
            .with_context(|| format!("failed to write {}", path.display()))?;
        Ok(config)
    }

    /// Returns a valid name for an account, from its address, that no
    /// other account has.
    ///
    /// # Arguments
    ///
    /// - `index`: The index of the account.
    fn unique_name(&self, index: usize) -> String {
        let base: String = self.sources[index]
            .username
            .trim()
            .to_lowercase()
            .chars()
            .map(|c| if c.is_ascii_alphanumeric() { c } else { '-' })
            .collect();
        let base = if base.is_empty() { "account" } else { &base };
        let taken = |name: &str| self.sources.iter().any(|s| s.name == name);
        let mut name = base.to_owned();
        let mut number = 2;
        while taken(&name) {
            name = format!("{base}-{number}");
            number += 1;
        }
        name
    }
}

impl Account {
    /// Returns an empty account with the default port.
    fn new() -> Self {
        Self {
            port: DEFAULT_PORT,
            ..Self::default()
        }
    }

    /// Fills in the server of a known provider, if none is set yet.
    fn suggest_host(&mut self) {
        let domain = self.username.rsplit_once('@').map(|(_, d)| d);
        let known = KNOWN_HOSTS.iter().find(|(d, _)| {
            domain.is_some_and(|domain| domain.eq_ignore_ascii_case(d))
        });
        if let Some((_, host)) = known
            && self.host.is_empty()
        {
            (*host).clone_into(&mut self.host);
        }
    }
}

/// A message of the thread that does the work.
#[derive(Debug)]
enum Message {
    /// A line of progress.
    Line(String),
    /// The work is done.
    Done(Finished),
}

/// How a run ended.
#[derive(Debug)]
enum Finished {
    /// The report was written, maybe with failed accounts.
    Report(Outcome),
    /// The user cancelled the run.
    Cancelled,
    /// The run failed, with the error.
    Failed(String),
}

/// Something that saves the settings, which waits for a confirmation if
/// that removes comments.
#[derive(Debug, Clone, Copy)]
enum Action {
    Save,
    Run,
}

/// What the window shows below the form.
#[derive(Debug)]
enum State {
    /// Nothing ran yet.
    Idle,
    /// The settings were saved, and nothing ran yet.
    Saved,
    /// The work runs, and sends its messages.
    Running {
        receiver: Receiver<Message>,
        /// Set to stop the work at the next safe point.
        cancel: Arc<AtomicBool>,
    },
    /// The last run is done.
    Done(Finished),
}

/// The state of the window.
#[derive(Debug)]
struct App {
    /// The path of the config file.
    path: PathBuf,
    form: Form,
    /// Whether to use the emails of earlier runs only.
    offline: bool,
    /// The error of loading or saving the config, if any.
    error: Option<String>,
    state: State,
    /// The progress of the last run.
    log: Vec<String>,
    /// Whether the config file has comments, which saving removes.
    comments: bool,
    /// The action that waits for the confirmation to remove the comments.
    pending: Option<Action>,
}

impl App {
    /// Loads the config file into the form.
    ///
    /// # Arguments
    ///
    /// - `path`: The path of the config file.
    fn new(path: PathBuf) -> Self {
        // Absolute, so that the window can show where the file is.
        let path = path::absolute(&path).unwrap_or(path);
        let (form, error) = match Form::load(&path) {
            Ok(form) => (form, None),
            Err(error) => (Form::new(), Some(format!("{error:#}"))),
        };
        let comments =
            fs::read_to_string(&path).is_ok_and(|text| has_comments(&text));
        Self {
            path,
            form,
            offline: false,
            error,
            state: State::Idle,
            log: Vec::new(),
            comments,
            pending: None,
        }
    }

    /// Runs an action, or asks first if saving removes comments.
    ///
    /// # Arguments
    ///
    /// - `action`: The action.
    /// - `ctx`: The context of the window.
    fn request(&mut self, action: Action, ctx: &egui::Context) {
        if self.comments {
            self.pending = Some(action);
        } else {
            self.perform(action, ctx);
        }
    }

    /// Runs an action.
    ///
    /// # Arguments
    ///
    /// - `action`: The action.
    /// - `ctx`: The context of the window.
    fn perform(&mut self, action: Action, ctx: &egui::Context) {
        match action {
            Action::Save => {
                if self.save().is_some() {
                    self.state = State::Saved;
                }
            }
            Action::Run => self.start(ctx),
        }
    }

    /// Asks whether to remove the comments of the config file, if an
    /// action waits for that.
    ///
    /// # Arguments
    ///
    /// - `ctx`: The context of the window.
    fn confirm(&mut self, ctx: &egui::Context) {
        let Some(action) = self.pending else {
            return;
        };
        let modal =
            egui::Modal::new(egui::Id::unique("comments")).show(ctx, |ui| {
                ui.heading("Remove the comments?");
                ui.label(
                    "The settings file has comments, e.g. from the example \
                 config. Saving keeps all settings, but removes the comments.",
                );
                ui.horizontal(|ui| {
                    let save = ui.button("Save anyway").clicked();
                    let cancel = ui.button("Cancel").clicked();
                    (save, cancel)
                })
                .inner
            });
        let (save, cancel) = modal.inner;
        if save {
            self.pending = None;
            self.perform(action, ctx);
        } else if cancel || modal.should_close() {
            self.pending = None;
        }
    }

    /// Saves the config and starts the work in a thread.
    ///
    /// # Arguments
    ///
    /// - `ctx`: The context of the window, to repaint it on progress.
    fn start(&mut self, ctx: &egui::Context) {
        let Some(config) = self.save() else {
            return;
        };
        self.log.clear();
        let (sender, receiver) = mpsc::channel();
        let offline = self.offline;
        let ctx = ctx.clone();
        let cancel = Arc::new(AtomicBool::new(false));
        let cancelled = Arc::clone(&cancel);
        thread::spawn(move || {
            let mut log = |line: &str| {
                // The window may be closed already, then nobody listens.
                let _ = sender.send(Message::Line(line.to_owned()));
                ctx.request_repaint();
            };
            let result = run::run(
                &config,
                offline,
                None,
                TOP,
                Timestamp::now(),
                &cancelled,
                &mut log,
            );
            let finished = match result {
                Ok(outcome) => Finished::Report(outcome),
                Err(error) if error.is::<Cancelled>() => Finished::Cancelled,
                Err(error) => Finished::Failed(format!("{error:#}")),
            };
            let _ = sender.send(Message::Done(finished));
            ctx.request_repaint();
        });
        self.state = State::Running { receiver, cancel };
    }

    /// Takes the messages of the thread that does the work.
    fn receive(&mut self) {
        let State::Running { receiver, .. } = &self.state else {
            return;
        };
        let mut done = None;
        for message in receiver.try_iter() {
            match message {
                Message::Line(line) => self.log.push(line),
                Message::Done(result) => done = Some(result),
            }
        }
        if let Some(result) = done {
            self.state = State::Done(result);
        }
    }

    /// Shows where the settings are, and the data directory.
    ///
    /// # Arguments
    ///
    /// - `ui`: The UI to show it in.
    fn settings(&mut self, ui: &mut egui::Ui) {
        ui.heading("Settings file");
        let state = if self.path.exists() {
            "Your settings, including the passwords, are stored in this \
             file. Keep it private."
        } else {
            "Your settings, including the passwords, will be stored in this \
             file. Keep it private."
        };
        ui.label(state);
        let path = RichText::new(self.path.display().to_string()).monospace();
        ui.add(egui::Label::new(path).wrap());
        if let Some(dir) = self.path.parent()
            && dir.exists()
            && ui.button("Open its folder").clicked()
        {
            open(dir);
        }
        egui::Grid::new("settings").num_columns(2).show(ui, |ui| {
            ui.label("Folder for data and report")
                .on_hover_text("Relative to the folder of the settings file.");
            ui.add(
                egui::TextEdit::singleline(&mut self.form.data_dir)
                    .desired_width(f32::INFINITY),
            );
            ui.end_row();
        });
    }

    /// Saves the form, and shows an error if that fails.
    ///
    /// Returns the config if it was saved.
    fn save(&mut self) -> Option<Config> {
        match self.form.save(&self.path) {
            Ok(config) => {
                self.error = None;
                self.comments = false;
                Some(config)
            }
            Err(error) => {
                self.error = Some(format!("{error:#}"));
                None
            }
        }
    }

    /// Shows the form with the email accounts.
    ///
    /// # Arguments
    ///
    /// - `ui`: The UI to show it in.
    fn accounts(&mut self, ui: &mut egui::Ui) {
        ui.heading("1. Your email accounts");
        ui.label(
            "The password is often an app password, e.g. for Gmail with \
             2-step verification.",
        );
        let mut remove = None;
        let single = self.form.sources.len() == 1;
        for (index, account) in self.form.sources.iter_mut().enumerate() {
            ui.group(|ui| {
                egui::Grid::new(index).num_columns(2).show(ui, |ui| {
                    ui.label("Email address");
                    let address = ui.add(
                        egui::TextEdit::singleline(&mut account.username)
                            .hint_text("me@example.com")
                            .desired_width(f32::INFINITY),
                    );
                    if address.lost_focus() {
                        account.suggest_host();
                    }
                    ui.end_row();
                    ui.label("Password");
                    ui.add(
                        egui::TextEdit::singleline(&mut account.password)
                            .password(true)
                            .desired_width(f32::INFINITY),
                    );
                    ui.end_row();
                    ui.label("Server");
                    ui.horizontal(|ui| {
                        ui.add(
                            egui::TextEdit::singleline(&mut account.host)
                                .hint_text("imap.example.com"),
                        );
                        ui.label("Port");
                        ui.add(egui::DragValue::new(&mut account.port));
                    });
                    ui.end_row();
                });
                egui::CollapsingHeader::new("More")
                    .id_salt(("more", index))
                    .show(ui, |ui| {
                        egui::Grid::new(("more-grid", index))
                            .num_columns(2)
                            .show(ui, |ui| {
                                ui.label("Name").on_hover_text(
                                    "The name of the account's folder in the \
                                     data folder. Empty: from the address.",
                                );
                                ui.text_edit_singleline(&mut account.name);
                                ui.end_row();
                                ui.label("Other addresses").on_hover_text(
                                    "Your aliases or old addresses, separated \
                                     by commas. Emails from them count as \
                                     sent.",
                                );
                                ui.text_edit_singleline(
                                    &mut account.addresses_text,
                                );
                                ui.end_row();
                            });
                    });
                if !single && ui.button("Remove this account").clicked() {
                    remove = Some(index);
                }
            });
        }
        if let Some(index) = remove {
            self.form.sources.remove(index);
        }
        ui.horizontal(|ui| {
            if ui.button("Add another account").clicked() {
                self.form.sources.push(Account::new());
            }
            if ui.button("Save settings").clicked() {
                self.request(Action::Save, ui.ctx());
            }
        });
    }

    /// Shows the button to start, and the progress or result.
    ///
    /// # Arguments
    ///
    /// - `ui`: The UI to show it in.
    fn actions(&mut self, ui: &mut egui::Ui) {
        ui.add_space(8.0);
        ui.heading("2. Create the report");
        ui.label(
            "This only reads your emails. It never changes or deletes \
             anything. The first time takes a while for large mailboxes.",
        );
        ui.checkbox(
            &mut self.offline,
            "Use only the emails that were read before (no connection)",
        );
        let big = |text: &str, ui: &egui::Ui| {
            egui::Button::new(RichText::new(text).size(22.0).strong())
                .min_size(egui::vec2(ui.available_width(), 48.0))
        };
        // The button stops the run while it runs.
        let cancel = match &self.state {
            State::Running { cancel, .. } => Some(Arc::clone(cancel)),
            _ => None,
        };
        if let Some(cancel) = &cancel {
            let stopping = cancel.load(Ordering::Relaxed);
            if ui.add_enabled(!stopping, big("Cancel", ui)).clicked() {
                cancel.store(true, Ordering::Relaxed);
            }
        } else if ui.add(big("Create report", ui)).clicked() {
            self.request(Action::Run, ui.ctx());
        }
        if let Some(error) = &self.error {
            ui.colored_label(ui.visuals().error_fg_color, error);
        }
        match &self.state {
            State::Idle => {}
            State::Saved => {
                ui.colored_label(success(ui), "Settings saved.");
            }
            State::Running { cancel, .. } => {
                let status = if cancel.load(Ordering::Relaxed) {
                    "Stopping after the current step"
                } else {
                    self.log.last().map_or("Starting", String::as_str)
                };
                ui.horizontal(|ui| {
                    ui.spinner();
                    ui.label(status);
                });
            }
            State::Done(Finished::Cancelled) => {
                ui.label(
                    "Cancelled. The next run continues where this one \
                     stopped.",
                );
            }
            State::Done(Finished::Report(outcome)) => {
                if outcome.failed == 0 {
                    ui.colored_label(success(ui), "The report is ready.");
                } else {
                    ui.colored_label(
                        ui.visuals().error_fg_color,
                        format!(
                            "{} account(s) failed, see the details above. The \
                             report covers their emails of earlier runs only.",
                            outcome.failed
                        ),
                    );
                }
                let dir = outcome.report_dir.clone();
                ui.horizontal(|ui| {
                    if ui.button("Open the report folder").clicked() {
                        open(&dir);
                    }
                    let pdf = dir.join(PDF_FILE);
                    if pdf.exists() && ui.button("Open the PDF").clicked() {
                        open(&pdf);
                    }
                    if ui.button("Open report.md").clicked() {
                        open(&dir.join(REPORT_FILE));
                    }
                });
            }
            State::Done(Finished::Failed(error)) => {
                ui.colored_label(ui.visuals().error_fg_color, error);
            }
        }
        ui.add_space(8.0);
    }

    /// Shows the progress of the last run.
    ///
    /// # Arguments
    ///
    /// - `ui`: The UI to show it in.
    fn details(&self, ui: &mut egui::Ui) {
        egui::CollapsingHeader::new("Details of the last run")
            .default_open(false)
            .show(ui, |ui| {
                egui::ScrollArea::vertical()
                    .max_height(200.0)
                    .stick_to_bottom(true)
                    .show(ui, |ui| {
                        for line in &self.log {
                            let line = RichText::new(line).monospace();
                            ui.add(egui::Label::new(line).wrap());
                        }
                    });
            });
    }
}

impl eframe::App for App {
    /// Shows the window.
    ///
    /// # Arguments
    ///
    /// - `ui`: The UI of the whole window.
    /// - `_frame`: The frame of the window.
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        self.receive();
        self.confirm(ui.ctx());
        // Always visible, however many accounts there are.
        egui::Panel::bottom("actions").show(ui, |ui| self.actions(ui));
        egui::CentralPanel::default().show(ui, |ui| {
            egui::ScrollArea::vertical().show(ui, |ui| {
                self.accounts(ui);
                ui.add_space(16.0);
                self.settings(ui);
                ui.add_space(16.0);
                self.details(ui);
            });
        });
    }
}

/// Checks whether the text of a config file has comments.
///
/// Only whole-line comments count. A comment after a value is rare, and
/// the warning is a courtesy.
///
/// # Arguments
///
/// - `text`: The text of the config file.
fn has_comments(text: &str) -> bool {
    text.lines().any(|line| line.trim_start().starts_with('#'))
}

/// Returns the color of success messages, readable in dark and light mode.
///
/// # Arguments
///
/// - `ui`: The UI that shows the message.
fn success(ui: &egui::Ui) -> Color32 {
    if ui.visuals().dark_mode {
        Color32::LIGHT_GREEN
    } else {
        Color32::DARK_GREEN
    }
}

/// Opens a file or directory with the default program of the OS.
///
/// Errors are ignored, as the window shows the path anyway.
///
/// # Arguments
///
/// - `path`: The file or directory.
fn open(path: &Path) {
    let program = if cfg!(windows) {
        "explorer"
    } else if cfg!(target_os = "macos") {
        "open"
    } else {
        "xdg-open"
    };
    let _ = process::Command::new(program).arg(path).spawn();
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_util::TempDir;

    /// Saving what was loaded must keep the file as it was, also the
    /// relative data directory, which loading resolves.
    #[test]
    fn saves_the_loaded_config_unchanged() {
        let dir = TempDir::new("gui-round-trip");
        let path = dir.path().join("settings").join("imap-statistics.toml");
        let mut form = Form::new();
        form.sources[0].username = "Me@GMail.com".into();
        form.sources[0].password = "secret".into();
        form.sources[0].suggest_host();
        form.sources[0].addresses_text = "old@example.com, ".into();
        form.save(&path).expect("form should be saved");
        let first = fs::read_to_string(&path).expect("file should exist");

        let mut loaded = Form::load(&path).expect("form should be loaded");
        let config = loaded.save(&path).expect("form should be saved again");

        assert_eq!(fs::read_to_string(&path).expect("file"), first);
        assert_eq!(loaded.data_dir, DEFAULT_DATA_DIR);
        assert_eq!(
            config.data_dir,
            dir.path().join("settings").join(DEFAULT_DATA_DIR)
        );
        let source = &config.sources[0];
        assert_eq!(source.name, "me-gmail-com");
        assert_eq!(source.host, "imap.gmail.com");
        assert_eq!(source.addresses, ["old@example.com"]);
    }

    /// Two accounts of the same address need different directories.
    #[test]
    fn derives_unique_names() {
        let mut form = Form::new();
        form.sources[0].name = "me-example-com".into();
        form.sources.push(Account {
            username: "me@example.com".into(),
            ..Account::new()
        });
        assert_eq!(form.unique_name(1), "me-example-com-2");
    }
}
