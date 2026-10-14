//! Keeping LiteSheet up to date: a check shortly after launch (and every few
//! hours), a background download when a new version is out, and a button in
//! the menu bar to restart into it. If the user just quits instead, the new
//! version is installed then and is there next time.

use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::{Receiver, Sender, channel};
use std::time::{Duration, Instant};

use egui::{Color32, RichText};
use serde::{Deserialize, Serialize};

use super::app::App;
use crate::update::{self, Release};

const FIRST_CHECK: Duration = Duration::from_secs(3);
const CHECK_EVERY: Duration = Duration::from_secs(6 * 60 * 60);

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
struct Prefs {
    /// Download and install new versions without asking.
    auto_install: bool,
}

impl Default for Prefs {
    fn default() -> Self {
        Self { auto_install: true }
    }
}

impl Prefs {
    fn load() -> Self {
        crate::config::file("updates.json")
            .and_then(|p| std::fs::read_to_string(p).ok())
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or_default()
    }

    fn save(&self) {
        if let Some(path) = crate::config::file("updates.json") {
            if let Some(dir) = path.parent() {
                let _ = std::fs::create_dir_all(dir);
            }
            if let Ok(json) = serde_json::to_string_pretty(self) {
                let _ = std::fs::write(path, json);
            }
        }
    }
}

pub(crate) enum UpdateState {
    Idle,
    Checking {
        manual: bool,
    },
    /// A newer version exists but isn't downloaded (automatic installs are
    /// off, or this copy of the app can't replace itself).
    Available(Release),
    Downloading(Release, Arc<AtomicU64>),
    /// Downloaded and verified; installs when the app quits.
    Ready(Release, PathBuf),
}

enum Event {
    Checked {
        result: Result<Release, String>,
        manual: bool,
    },
    Downloaded(Result<PathBuf, String>),
}

pub(crate) struct Updater {
    pub(crate) state: UpdateState,
    prefs: Prefs,
    /// When to check next; checks only run in the real app, never in tests.
    next_check: Option<Instant>,
    /// Restart into the new version after quitting (rather than just install it).
    restart: bool,
    installed: bool,
    tx: Sender<Event>,
    rx: Receiver<Event>,
}

impl Updater {
    pub(crate) fn new() -> Self {
        let enabled = !cfg!(test) && std::env::var_os("LITESHEET_NO_UPDATE_CHECK").is_none();
        let (tx, rx) = channel();
        Self {
            state: UpdateState::Idle,
            prefs: Prefs::load(),
            next_check: enabled.then(|| Instant::now() + FIRST_CHECK),
            restart: false,
            installed: false,
            tx,
            rx,
        }
    }
}

impl App {
    /// Run due checks and handle finished checks and downloads.
    pub(crate) fn poll_updates(&mut self, ctx: &egui::Context) {
        if let Some(at) = self.updater.next_check {
            let now = Instant::now();
            if now >= at {
                self.updater.next_check = Some(now + CHECK_EVERY);
                if matches!(self.updater.state, UpdateState::Idle) {
                    self.check_for_updates(false, ctx);
                }
            } else {
                ctx.request_repaint_after(at - now);
            }
        }
        while let Ok(event) = self.updater.rx.try_recv() {
            match event {
                Event::Checked { result, manual } => self.on_checked(result, manual, ctx),
                Event::Downloaded(Ok(app)) => {
                    if let UpdateState::Downloading(release, _) = &self.updater.state {
                        let version = release.version.clone();
                        self.updater.state = UpdateState::Ready(release.clone(), app);
                        self.notify(format!(
                            "LiteSheet {version} is ready. Restart to update, or it installs when you quit."
                        ));
                    }
                }
                Event::Downloaded(Err(e)) => {
                    if let UpdateState::Downloading(release, _) = &self.updater.state {
                        self.updater.state = UpdateState::Available(release.clone());
                    }
                    self.error(e);
                }
            }
        }
    }

    pub(crate) fn check_for_updates(&mut self, manual: bool, ctx: &egui::Context) {
        match &self.updater.state {
            UpdateState::Checking { .. } | UpdateState::Downloading(..) => return,
            UpdateState::Ready(release, _) if manual => {
                let v = release.version.clone();
                self.notify(format!("LiteSheet {v} is ready: restart to update"));
                return;
            }
            _ => {}
        }
        self.updater.state = UpdateState::Checking { manual };
        let (client, tx, ctx) = (self.http.clone(), self.updater.tx.clone(), ctx.clone());
        self.rt.spawn(async move {
            let result = update::latest(&client).await;
            let _ = tx.send(Event::Checked { result, manual });
            ctx.request_repaint();
        });
    }

    fn on_checked(&mut self, result: Result<Release, String>, manual: bool, ctx: &egui::Context) {
        let release = match result {
            Ok(r) => r,
            Err(e) => {
                self.updater.state = UpdateState::Idle;
                if manual {
                    self.error(e);
                }
                return;
            }
        };
        if !update::is_newer(&release.version, update::current_version()) {
            self.updater.state = UpdateState::Idle;
            if manual {
                self.notify(format!(
                    "You're up to date: LiteSheet {} is the latest version",
                    update::current_version()
                ));
            }
            return;
        }
        let can_install = release.download.is_some() && update::installed_bundle().is_ok();
        self.updater.state = UpdateState::Available(release);
        if can_install && (self.updater.prefs.auto_install || manual) {
            self.download_update(ctx);
        }
    }

    fn download_update(&mut self, ctx: &egui::Context) {
        let UpdateState::Available(release) = &self.updater.state else {
            return;
        };
        let release = release.clone();
        let progress = Arc::new(AtomicU64::new(0));
        let (client, tx, ctx) = (self.http.clone(), self.updater.tx.clone(), ctx.clone());
        let (r, p) = (release.clone(), progress.clone());
        self.rt.spawn(async move {
            let result = update::download(&client, &r, p).await;
            let _ = tx.send(Event::Downloaded(result));
            ctx.request_repaint();
        });
        self.updater.state = UpdateState::Downloading(release, progress);
    }

    /// Called as the app quits: hand a downloaded update to the installer.
    pub(crate) fn install_update_on_exit(&mut self) {
        if self.updater.installed {
            return;
        }
        let UpdateState::Ready(_, app) = &self.updater.state else {
            return;
        };
        if let Ok(bundle) = update::installed_bundle()
            && update::install_on_exit(app, &bundle, self.updater.restart).is_ok()
        {
            self.updater.installed = true;
        }
    }

    /// The update button in the menu bar (nothing while there's no update).
    pub(crate) fn update_button(&mut self, ui: &mut egui::Ui) {
        let ctx = ui.ctx().clone();
        let green = Color32::from_rgb(0x1F, 0x9D, 0x55);
        match &self.updater.state {
            UpdateState::Idle => {}
            UpdateState::Checking { manual } => {
                if *manual {
                    ui.spinner();
                }
            }
            UpdateState::Downloading(release, progress) => {
                let got = progress.load(Ordering::Relaxed);
                let text = if release.size > 0 {
                    format!("Updating… {}%", got * 100 / release.size.max(1))
                } else {
                    "Updating…".into()
                };
                ui.label(RichText::new(text).weak());
                ui.spinner();
                ctx.request_repaint_after(Duration::from_millis(250));
            }
            UpdateState::Ready(release, _) => {
                let button = egui::Button::new(
                    RichText::new(format!("⬆ Restart to update to {}", release.version))
                        .color(Color32::WHITE),
                )
                .fill(green);
                if ui
                    .add(button)
                    .on_hover_text(
                        "The new version is downloaded. Restart now, or it installs when you quit.",
                    )
                    .clicked()
                {
                    self.updater.restart = true;
                    self.request_quit(&ctx);
                }
            }
            UpdateState::Available(release) => {
                let release = release.clone();
                let installable = release.download.is_some() && update::installed_bundle().is_ok();
                let button = egui::Button::new(
                    RichText::new(format!("⬆ LiteSheet {} available", release.version))
                        .color(Color32::WHITE),
                )
                .fill(green);
                let hint = match update::installed_bundle() {
                    Ok(_) => "Download and install the new version".to_string(),
                    Err(why) => format!("{why}. Click to open the download page."),
                };
                if ui.add(button).on_hover_text(hint).clicked() {
                    if installable {
                        self.download_update(&ctx);
                    } else {
                        ctx.open_url(egui::OpenUrl::new_tab(&release.page));
                    }
                }
            }
        }
    }

    /// The update items in the Help menu.
    pub(crate) fn update_menu(&mut self, ui: &mut egui::Ui) {
        let ctx = ui.ctx().clone();
        if ui.button("Check for Updates…").clicked() {
            self.check_for_updates(true, &ctx);
        }
        let mut auto = self.updater.prefs.auto_install;
        if ui
            .checkbox(&mut auto, "Install Updates Automatically")
            .changed()
        {
            self.updater.prefs.auto_install = auto;
            self.updater.prefs.save();
        }
        ui.label(
            RichText::new(format!("LiteSheet {}", update::current_version()))
                .weak()
                .small(),
        );
    }

    #[cfg(test)]
    pub(crate) fn fake_update_check(&mut self, release: Release, manual: bool) {
        let ctx = egui::Context::default();
        self.on_checked(Ok(release), manual, &ctx);
    }
}
