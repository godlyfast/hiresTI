//! Root Relm4 component. Owns the `AppModel` and composes the four
//! top-level region components (header, sidebar, content stack, mini
//! player) plus the modal login dialog. Each child emits an `Output`
//! enum that a small `forward()` adapter translates into `AppInput`.
//!
//! Phase 4 wires the auth backbone: cold-start token restore, device-
//! code OAuth login flow with polling, header label sync. Network calls
//! run on background threads; results return through the input sender,
//! which Relm4 marshals to the GTK main loop.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use libadwaita::prelude::*;
use libadwaita::{ApplicationWindow, ToolbarView};
use relm4::adw::Application;
use relm4::gtk::glib;
use relm4::gtk::{Box as GtkBox, Orientation, Paned, Separator};
use relm4::{
    Component, ComponentController, ComponentParts, ComponentSender, Controller,
    SimpleComponent,
};

use crate::components::content_stack::{ContentStackInput, ContentStackModel};
use crate::components::header::{HeaderInput, HeaderModel, HeaderOutput};
use crate::components::login_dialog::{
    LoginDialogInput, LoginDialogModel, LoginDialogOutput,
};
use crate::components::mini_player::{MiniPlayerModel, MiniPlayerOutput};
use crate::components::sidebar::{SidebarInput, SidebarModel, SidebarOutput};
use crate::messages::{AppInput, AuthPollOutcome};
use crate::model::AppModel;
use crate::services::tidal_session::{spawn_blocking, TidalSessionService};
use crate::settings::Settings;
use crate::state::auth::{AuthStatus, UserProfile};

pub struct AppController {
    model: AppModel,
    /// Owns the rust_tidal_core Session and persistence.
    session: TidalSessionService,
    /// Set to true when an in-flight device-poll loop should stop. The
    /// timer task checks this each tick and exits if set; user pressing
    /// Cancel or the auth flow finishing both flip it.
    poll_cancelled: Arc<AtomicBool>,

    #[allow(dead_code)]
    header: Controller<HeaderModel>,
    sidebar: Controller<SidebarModel>,
    content: Controller<ContentStackModel>,
    #[allow(dead_code)]
    mini: Controller<MiniPlayerModel>,
    login: Controller<LoginDialogModel>,
}

pub struct AppWidgets {
    window: ApplicationWindow,
}

impl SimpleComponent for AppController {
    type Init = AppModel;
    type Input = AppInput;
    type Output = ();
    type Root = ApplicationWindow;
    type Widgets = AppWidgets;

    fn init_root() -> Self::Root {
        ApplicationWindow::builder()
            .title("HiresTI")
            .default_width(1250)
            .default_height(800)
            .build()
    }

    fn init(
        init: Self::Init,
        root: Self::Root,
        sender: ComponentSender<Self>,
    ) -> ComponentParts<Self> {
        root.set_default_size(init.settings.window_width, init.settings.window_height);

        let session = TidalSessionService::new();

        // ---- Children -------------------------------------------------
        let header = HeaderModel::builder().launch(()).forward(
            sender.input_sender(),
            |out| match out {
                HeaderOutput::Search(q) => AppInput::Search(q),
                HeaderOutput::LoginRequested => AppInput::RequestLogin,
                HeaderOutput::OpenSettings => AppInput::OpenSettings,
                HeaderOutput::OpenAbout => AppInput::OpenAbout,
            },
        );

        let sidebar = SidebarModel::builder()
            .launch(init.current_nav)
            .forward(sender.input_sender(), |out| match out {
                SidebarOutput::Navigate(t) => AppInput::NavigateTo(t),
            });

        let content = ContentStackModel::builder()
            .launch(init.current_nav)
            .detach();

        let mini = MiniPlayerModel::builder().launch(()).forward(
            sender.input_sender(),
            |out| match out {
                MiniPlayerOutput::Play => AppInput::TransportPlay,
                MiniPlayerOutput::Pause => AppInput::TransportPause,
                MiniPlayerOutput::Next => AppInput::TransportNext,
                MiniPlayerOutput::Previous => AppInput::TransportPrev,
                MiniPlayerOutput::Seek(p) => AppInput::TransportSeek(p),
            },
        );

        let login = LoginDialogModel::builder().launch(()).forward(
            sender.input_sender(),
            |out| match out {
                LoginDialogOutput::Cancelled => AppInput::AuthDeviceCancelled,
                LoginDialogOutput::OpenInBrowser(url) => AppInput::OpenBrowser(url),
                LoginDialogOutput::CopyCode(code) => AppInput::CopyToClipboard(code),
            },
        );
        // Login dialog needs a parent for modality.
        login.widget().set_transient_for(Some(&root));

        // ---- Layout ---------------------------------------------------
        let toolbar = ToolbarView::new();
        toolbar.add_top_bar(header.widget());

        let body = GtkBox::builder().orientation(Orientation::Vertical).build();

        let paned = Paned::builder()
            .orientation(Orientation::Horizontal)
            .resize_start_child(false)
            .shrink_start_child(false)
            .position(220)
            .build();
        paned.set_start_child(Some(sidebar.widget()));
        paned.set_end_child(Some(content.widget()));
        body.append(&paned);

        body.append(&Separator::new(Orientation::Horizontal));
        body.append(mini.widget());

        toolbar.set_content(Some(&body));
        root.set_content(Some(&toolbar));

        // Window resize → settings.
        let s = sender.clone();
        root.connect_default_width_notify(move |w| {
            let _ = s.input_sender().send(AppInput::WindowResized {
                width: w.default_width(),
                height: w.default_height(),
            });
        });
        let s = sender.clone();
        root.connect_default_height_notify(move |w| {
            let _ = s.input_sender().send(AppInput::WindowResized {
                width: w.default_width(),
                height: w.default_height(),
            });
        });

        // ---- Cold-start auth restore ----------------------------------
        // If a token file exists, kick off load_token + check_login on a
        // worker thread. We don't block init() because /v1/sessions can
        // take ~hundreds of ms over a slow network and the GTK loop
        // shouldn't wait.
        kickoff_cold_start(session.clone(), sender.clone());

        let model = Self {
            model: init,
            session,
            poll_cancelled: Arc::new(AtomicBool::new(false)),
            header,
            sidebar,
            content,
            mini,
            login,
        };
        let widgets = AppWidgets { window: root };
        ComponentParts { model, widgets }
    }

    fn update(&mut self, msg: Self::Input, sender: ComponentSender<Self>) {
        match msg {
            AppInput::NavigateTo(target) => {
                self.model.current_nav = target;
                self.model.settings.last_nav = target.as_id().into();
                self.persist_settings();
                self.sidebar.sender().send(SidebarInput::SetActive(target)).ok();
                self.content.sender().send(ContentStackInput::Show(target)).ok();
            }
            AppInput::WindowResized { width, height } => {
                if self.model.settings.remember_window_size {
                    self.model.settings.window_width = width;
                    self.model.settings.window_height = height;
                }
            }
            AppInput::ApplySettings(new) => {
                self.model.settings = new;
                self.persist_settings();
            }
            AppInput::Search(q) => {
                tracing::info!(query = %q, "search submitted (Phase 7 will route this)");
            }
            AppInput::RequestLogin => {
                if self.model.auth.is_logged_in() {
                    sender.input_sender().send(AppInput::LogoutRequested).ok();
                } else {
                    self.start_device_login(sender.clone());
                }
            }
            AppInput::LogoutRequested => {
                tracing::info!("logout requested (Phase 8 will clear token + reload UI)");
            }
            AppInput::OpenSettings => {
                tracing::info!("settings dialog requested (Phase 8)");
            }
            AppInput::OpenAbout => {
                tracing::info!("about dialog requested (Phase 8)");
            }
            AppInput::TransportPlay
            | AppInput::TransportPause
            | AppInput::TransportNext
            | AppInput::TransportPrev => {
                tracing::info!(?msg, "transport (Phase 4 wires the audio engine)");
            }
            AppInput::TransportSeek(p) => {
                tracing::debug!(seek = p, "seek (Phase 4 wires the audio engine)");
            }

            AppInput::AuthRestoreResult(profile) => {
                self.apply_auth_result(profile);
            }
            AppInput::AuthDeviceStarted(info) => {
                self.poll_cancelled.store(false, Ordering::SeqCst);
                self.login.sender().send(LoginDialogInput::Show(info.clone())).ok();
                self.schedule_device_poll(
                    Duration::from_secs(info.interval.max(2) as u64),
                    sender.clone(),
                );
            }
            AppInput::AuthDeviceStartFailed(err) => {
                tracing::warn!(error = %err, "device-code start failed");
                self.model.auth.last_error = Some(err);
                self.model.auth.status = AuthStatus::LoggedOut;
            }
            AppInput::AuthDevicePollTick(outcome) => match outcome {
                AuthPollOutcome::StillPending => {
                    // Loop continues; nothing to do.
                }
                AuthPollOutcome::LoggedIn(profile) => {
                    self.poll_cancelled.store(true, Ordering::SeqCst);
                    self.login.sender().send(LoginDialogInput::Hide).ok();
                    if let Err(e) = self.session.save_persisted() {
                        tracing::warn!(error = %e, "failed to persist token after login");
                    }
                    self.apply_auth_result(Some(profile));
                }
                AuthPollOutcome::Failed(err) => {
                    self.poll_cancelled.store(true, Ordering::SeqCst);
                    self.login
                        .sender()
                        .send(LoginDialogInput::SetStatus(format!("Failed: {err}")))
                        .ok();
                    tracing::warn!(error = %err, "device-code poll terminated");
                }
            },
            AppInput::AuthDeviceCancelled => {
                self.poll_cancelled.store(true, Ordering::SeqCst);
                self.login.sender().send(LoginDialogInput::Hide).ok();
            }
            AppInput::OpenBrowser(url) => {
                if let Err(e) = open_in_browser(&url) {
                    tracing::warn!(error = %e, %url, "xdg-open failed");
                }
            }
            AppInput::CopyToClipboard(text) => {
                copy_to_clipboard(&text);
            }
        }
    }

    fn update_view(&self, widgets: &mut Self::Widgets, _sender: ComponentSender<Self>) {
        let _ = &widgets.window;
        let display = self
            .model
            .auth
            .profile
            .as_ref()
            .map(|p| p.display_name().to_string());
        self.header
            .sender()
            .send(HeaderInput::SetUserDisplay(display))
            .ok();
    }
}

impl AppController {
    fn persist_settings(&self) {
        if let Err(e) = self.model.settings.save() {
            tracing::warn!(error = %e, "settings save failed");
        }
    }

    fn apply_auth_result(&mut self, profile: Option<UserProfile>) {
        match profile {
            Some(p) => {
                self.model.auth.profile = Some(p);
                self.model.auth.status = AuthStatus::LoggedIn;
                self.model.auth.last_error = None;
                tracing::info!(
                    user_id = self.model.auth.profile.as_ref().map(|p| p.user_id),
                    "logged in"
                );
            }
            None => {
                self.model.auth.profile = None;
                self.model.auth.status = AuthStatus::LoggedOut;
            }
        }
    }

    fn start_device_login(&mut self, sender: ComponentSender<Self>) {
        self.model.auth.status = AuthStatus::Authenticating;
        self.model.auth.last_error = None;
        let svc = self.session.clone();
        let sender_in = sender.input_sender().clone();
        spawn_blocking(
            move || svc.oauth_device_start_blocking(),
            move |result| match result {
                Ok(info) => {
                    let _ = sender_in.send(AppInput::AuthDeviceStarted(info));
                }
                Err(e) => {
                    let _ = sender_in.send(AppInput::AuthDeviceStartFailed(e.to_string()));
                }
            },
        );
    }

    fn schedule_device_poll(&self, interval: Duration, sender: ComponentSender<Self>) {
        let svc = self.session.clone();
        let cancel = Arc::clone(&self.poll_cancelled);
        let secs = interval.as_secs().max(2);
        // glib timeout runs on the main thread; each tick fires off a
        // worker thread for the blocking poll call so the UI stays
        // responsive even if the network hiccups.
        glib::timeout_add_seconds_local(secs as u32, move || {
            if cancel.load(Ordering::SeqCst) {
                return glib::ControlFlow::Break;
            }
            // Two clones: one captured by the closure that runs
            // oauth_device_poll, the other captured by the result
            // callback which fetches the user profile on success.
            let svc_for_poll = svc.clone();
            let svc_for_cb = svc.clone();
            let sender_in = sender.input_sender().clone();
            let cancel_inner = Arc::clone(&cancel);
            spawn_blocking(
                move || svc_for_poll.oauth_device_poll_blocking(),
                move |result| {
                    if cancel_inner.load(Ordering::SeqCst) {
                        return;
                    }
                    let outcome = match result {
                        Ok(None) => AuthPollOutcome::StillPending,
                        Ok(Some(info)) => {
                            // Best-effort: enrich the UserProfile with
                            // /users/{id} fields. Failure is non-fatal —
                            // we still log in with the bare info.
                            let mut profile = UserProfile::from_user_info(&info);
                            if let Ok(json) = svc_for_cb.fetch_profile_blocking(info.user_id)
                            {
                                fill_profile_from_json(&mut profile, &json);
                            }
                            AuthPollOutcome::LoggedIn(profile)
                        }
                        Err(e) => AuthPollOutcome::Failed(e.to_string()),
                    };
                    let _ = sender_in.send(AppInput::AuthDevicePollTick(outcome));
                },
            );
            glib::ControlFlow::Continue
        });
    }
}

fn kickoff_cold_start(svc: TidalSessionService, sender: ComponentSender<AppController>) {
    let svc_for_save = svc.clone();
    spawn_blocking(
        move || -> Option<UserProfile> {
            let token = match TidalSessionService::load_persisted() {
                Ok(Some(t)) => t,
                Ok(None) => return None,
                Err(e) => {
                    tracing::warn!(error = %e, "token load failed");
                    return None;
                }
            };
            let info = match svc.load_token_with_refresh_blocking(token) {
                Ok(i) => i,
                Err(e) => {
                    tracing::info!(error = %e, "saved token rejected; staying logged out");
                    return None;
                }
            };
            // If refresh ran above, persist the new token immediately
            // so the next launch can skip /v1/sessions on the stale one.
            if let Err(e) = svc_for_save.save_persisted() {
                tracing::debug!(error = %e, "save after restore skipped");
            }
            if !svc.check_login_blocking() {
                tracing::info!("check_login returned false; staying logged out");
                return None;
            }
            let mut profile = UserProfile::from_user_info(&info);
            if let Ok(json) = svc.fetch_profile_blocking(info.user_id) {
                fill_profile_from_json(&mut profile, &json);
            }
            Some(profile)
        },
        move |profile| {
            let _ = sender.input_sender().send(AppInput::AuthRestoreResult(profile));
        },
    );
}

fn fill_profile_from_json(p: &mut UserProfile, json: &serde_json::Value) {
    if let Some(s) = json.get("firstName").and_then(|v| v.as_str()) {
        p.first_name = s.to_owned();
    }
    if let Some(s) = json.get("lastName").and_then(|v| v.as_str()) {
        p.last_name = s.to_owned();
    }
    if let Some(s) = json.get("username").and_then(|v| v.as_str()) {
        p.username = s.to_owned();
    }
    if let Some(s) = json.get("email").and_then(|v| v.as_str()) {
        p.email = s.to_owned();
    }
    // displayName / nickName is owned by the profileMetadata endpoint;
    // not fetched here yet (matches Python behavior of best-effort).
}

fn open_in_browser(url: &str) -> std::io::Result<()> {
    std::process::Command::new("xdg-open").arg(url).spawn()?;
    Ok(())
}

fn copy_to_clipboard(text: &str) {
    if let Some(display) = relm4::gtk::gdk::Display::default() {
        display.clipboard().set_text(text);
    } else {
        tracing::debug!("no GDK display, can't copy to clipboard");
    }
}

/// Build the `Adw.Application`, wire the root component, and run the
/// GTK main loop. Called from `main()` once logging + paths are set up.
pub fn run(initial: Settings) -> i32 {
    let app = Application::builder()
        .application_id("com.hiresti.player")
        .build();

    let runner = relm4::RelmApp::from_app(app);
    runner.run::<AppController>(AppModel::from_settings(initial));
    0
}
