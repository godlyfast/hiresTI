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

use crate::components::content_stack::{
    ContentStackInit, ContentStackInput, ContentStackModel,
};
use crate::components::header::{HeaderInput, HeaderModel, HeaderOutput};
use crate::components::login_dialog::{
    LoginDialogInput, LoginDialogModel, LoginDialogOutput,
};
use crate::components::mini_player::{MiniPlayerInput, MiniPlayerModel, MiniPlayerOutput};
use crate::components::sidebar::{SidebarInput, SidebarModel, SidebarOutput};
use crate::components::views::album_detail::{AlbumDetailInit, AlbumDetailViewModel};
use crate::components::views::albums::{AlbumsViewInput, AlbumsViewModel};
use crate::components::views::artist_detail::{ArtistDetailInit, ArtistDetailViewModel};
use crate::components::views::artists::{ArtistsViewInput, ArtistsViewModel};
use crate::components::views::common::LibraryViewOutput;
use crate::components::views::discovery::{
    DiscoverySource, DiscoveryViewInit, DiscoveryViewInput, DiscoveryViewModel,
};
use crate::components::views::history::{HistoryViewInput, HistoryViewModel};
use crate::components::views::mix_detail::{MixDetailInit, MixDetailViewModel};
use crate::components::views::mixes::{MixesViewInput, MixesViewModel};
use crate::components::views::playlist_detail::{PlaylistDetailInit, PlaylistDetailViewModel};
use crate::components::views::playlists::{PlaylistsViewInput, PlaylistsViewModel};
use crate::components::views::tabbed_discovery::{
    TabSource, TabbedDiscoveryInit, TabbedDiscoveryInput, TabbedDiscoveryViewModel,
};
use crate::components::views::tracks::{TracksViewInput, TracksViewModel};
use crate::messages::{AppInput, AuthPollOutcome, NavTarget};
use crate::model::AppModel;
use crate::services::tidal_session::{spawn_blocking, ResolvedPlayback, TidalSessionService};
use crate::state::playback::TransportState;
use rust_audio_core::Engine;
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

    // Library views (Phase 5). Each owns its own data fetch; the
    // parent dispatches Refresh on first navigation.
    albums_view: Controller<AlbumsViewModel>,
    tracks_view: Controller<TracksViewModel>,
    artists_view: Controller<ArtistsViewModel>,
    playlists_view: Controller<PlaylistsViewModel>,
    mixes_view: Controller<MixesViewModel>,
    #[allow(dead_code)]
    history_view: Controller<HistoryViewModel>,

    // Discovery views (Phase 6). Home / New / Top / Hi-Res share one
    // shared component; Genres / Decades / Moods are tab-style and
    // come in Phase 6.5.
    home_view: Controller<DiscoveryViewModel>,
    new_view: Controller<DiscoveryViewModel>,
    top_view: Controller<DiscoveryViewModel>,
    hires_view: Controller<DiscoveryViewModel>,
    genres_view: Controller<TabbedDiscoveryViewModel>,
    decades_view: Controller<TabbedDiscoveryViewModel>,
    moods_view: Controller<TabbedDiscoveryViewModel>,

    /// Currently displayed detail page, if any. Replacing the variant
    /// drops the previous Controller (and its widget — ContentStack
    /// removes it from the stack via SetDetail).
    detail: Option<DetailPage>,

    /// Monotonic counter for play requests — Phase 7-C uses it as a
    /// "stale-resolve drop" key so a slow stream-fetch can't clobber a
    /// fresher click. Wraps without consequence; collisions across 2^64
    /// clicks aren't a real concern.
    play_request_counter: u64,

    /// rust_audio_core engine. None on platforms where construction
    /// fails (no usable native transport) — playback flows degrade to
    /// log-only without crashing the UI.
    engine: Option<Engine>,
}

/// Variants of the global detail surface. Each holds the active
/// Controller so the widget tree stays alive while the page is open.
enum DetailPage {
    Album(Controller<AlbumDetailViewModel>),
    Playlist(Controller<PlaylistDetailViewModel>),
    Artist(Controller<ArtistDetailViewModel>),
    Mix(Controller<MixDetailViewModel>),
}

impl DetailPage {
    fn widget(&self) -> relm4::gtk::Widget {
        match self {
            DetailPage::Album(c) => c.widget().clone().into(),
            DetailPage::Playlist(c) => c.widget().clone().into(),
            DetailPage::Artist(c) => c.widget().clone().into(),
            DetailPage::Mix(c) => c.widget().clone().into(),
        }
    }
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
                HeaderOutput::BackPressed => AppInput::CloseDetail,
            },
        );

        let sidebar = SidebarModel::builder()
            .launch(init.current_nav)
            .forward(sender.input_sender(), |out| match out {
                SidebarOutput::Navigate(t) => AppInput::NavigateTo(t),
            });

        // Library view controllers. Each gets a clone of the session
        // service. They forward LibraryViewOutput up into AppInput so
        // the root can dispatch detail/play actions.
        let lib_forward = make_lib_forward();
        let albums_view = AlbumsViewModel::builder()
            .launch(session.clone())
            .forward(sender.input_sender(), lib_forward);
        let tracks_view = TracksViewModel::builder()
            .launch(session.clone())
            .forward(sender.input_sender(), lib_forward);
        let artists_view = ArtistsViewModel::builder()
            .launch(session.clone())
            .forward(sender.input_sender(), lib_forward);
        let playlists_view = PlaylistsViewModel::builder()
            .launch(session.clone())
            .forward(sender.input_sender(), lib_forward);
        let mixes_view = MixesViewModel::builder()
            .launch(session.clone())
            .forward(sender.input_sender(), lib_forward);
        let history_view = HistoryViewModel::builder()
            .launch(())
            .forward(sender.input_sender(), lib_forward);

        let home_view = DiscoveryViewModel::builder()
            .launch(DiscoveryViewInit {
                session: session.clone(),
                source: DiscoverySource::Home,
                empty_message:
                    "Your home feed is empty. Try refreshing or favorite some music to seed it.",
            })
            .forward(sender.input_sender(), lib_forward);
        let new_view = DiscoveryViewModel::builder()
            .launch(DiscoveryViewInit {
                session: session.clone(),
                source: DiscoverySource::Path("pages/explore_new_music".into()),
                empty_message: "TIDAL didn't return any New Music sections.",
            })
            .forward(sender.input_sender(), lib_forward);
        let top_view = DiscoveryViewModel::builder()
            .launch(DiscoveryViewInit {
                session: session.clone(),
                source: DiscoverySource::Path("pages/explore_top_music".into()),
                empty_message: "TIDAL didn't return any Top Music sections.",
            })
            .forward(sender.input_sender(), lib_forward);
        let hires_view = DiscoveryViewModel::builder()
            .launch(DiscoveryViewInit {
                session: session.clone(),
                source: DiscoverySource::Path("pages/hires".into()),
                empty_message: "No Hi-Res sections available right now.",
            })
            .forward(sender.input_sender(), lib_forward);

        let genres_view = TabbedDiscoveryViewModel::builder()
            .launch(TabbedDiscoveryInit {
                session: session.clone(),
                source: TabSource::Definitions("pages/genre_page".into()),
                empty_message: "TIDAL didn't return any genre tabs.",
            })
            .forward(sender.input_sender(), lib_forward);
        let decades_view = TabbedDiscoveryViewModel::builder()
            .launch(TabbedDiscoveryInit {
                session: session.clone(),
                source: TabSource::Static(vec![
                    ("1950s".into(), "pages/m_1950s".into()),
                    ("1960s".into(), "pages/m_1960s".into()),
                    ("1970s".into(), "pages/m_1970s".into()),
                    ("1980s".into(), "pages/m_1980s".into()),
                    ("1990s".into(), "pages/m_1990s".into()),
                    ("2000s".into(), "pages/m_2000s".into()),
                    ("2010s".into(), "pages/m_2010s".into()),
                ]),
                empty_message: "No decade pages available.",
            })
            .forward(sender.input_sender(), lib_forward);
        let moods_view = TabbedDiscoveryViewModel::builder()
            .launch(TabbedDiscoveryInit {
                session: session.clone(),
                source: TabSource::Definitions("pages/moods_page".into()),
                empty_message: "TIDAL didn't return any mood tabs.",
            })
            .forward(sender.input_sender(), lib_forward);

        let content = ContentStackModel::builder()
            .launch(ContentStackInit {
                current: init.current_nav,
                pages: vec![
                    (NavTarget::Home, home_view.widget().clone().into()),
                    (NavTarget::New, new_view.widget().clone().into()),
                    (NavTarget::Top, top_view.widget().clone().into()),
                    (NavTarget::HiRes, hires_view.widget().clone().into()),
                    (NavTarget::Genres, genres_view.widget().clone().into()),
                    (NavTarget::Decades, decades_view.widget().clone().into()),
                    (NavTarget::Moods, moods_view.widget().clone().into()),
                    (NavTarget::Albums, albums_view.widget().clone().into()),
                    (NavTarget::Tracks, tracks_view.widget().clone().into()),
                    (NavTarget::Artists, artists_view.widget().clone().into()),
                    (NavTarget::Playlists, playlists_view.widget().clone().into()),
                    (NavTarget::MixesAndRadio, mixes_view.widget().clone().into()),
                    (NavTarget::History, history_view.widget().clone().into()),
                ],
            })
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

        // ---- Audio engine ---------------------------------------------
        let mut engine = match Engine::new() {
            Ok(mut e) => {
                let driver = init.audio.driver.as_str();
                let device = if init.audio.device.is_empty() {
                    None
                } else {
                    Some(init.audio.device.as_str())
                };
                let rc = e.set_output(driver, device);
                if rc != 0 {
                    tracing::warn!(rc, driver, "engine.set_output rejected — falling back to defaults");
                }
                let vol = (init.audio.volume.min(150) as f32) / 100.0;
                e.set_volume(vol);
                Some(e)
            }
            Err(err) => {
                tracing::warn!(error = %err, "rust_audio_core engine init failed; running without audio");
                None
            }
        };
        let _ = &mut engine; // suppress warning if Engine::new always succeeds

        // ---- Playback tick --------------------------------------------
        // 1s cadence is plenty for the seek scale; finer-grained position
        // is read on demand if Phase 8 adds a Now Playing window with a
        // seek thumb that follows decode timing.
        let s = sender.clone();
        glib::timeout_add_seconds_local(1, move || {
            let _ = s.input_sender().send(AppInput::PlaybackTick);
            glib::ControlFlow::Continue
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
            albums_view,
            tracks_view,
            artists_view,
            playlists_view,
            mixes_view,
            history_view,
            home_view,
            new_view,
            top_view,
            hires_view,
            genres_view,
            decades_view,
            moods_view,
            detail: None,
            play_request_counter: 0,
            engine,
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
                // Sidebar nav drops any open detail surface. ContentStack
                // already clears its detail child on Show(); we have to
                // drop our owning Controller too.
                self.detail = None;
                self.sidebar.sender().send(SidebarInput::SetActive(target)).ok();
                self.content.sender().send(ContentStackInput::Show(target)).ok();
                // First navigation to a library view triggers a fetch.
                // Subsequent navigations don't re-fetch (the view's own
                // state machine guards against re-entrant Loading).
                self.dispatch_view_refresh(target);
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
                if let Some(engine) = self.engine.as_mut() {
                    let _ = engine.stop();
                }
                self.model.playback = Default::default();
                self.model.queue = Default::default();
                self.mini
                    .sender()
                    .send(MiniPlayerInput::SetNowPlaying {
                        title: "Nothing playing".into(),
                        artist: String::new(),
                    })
                    .ok();
                self.mini
                    .sender()
                    .send(MiniPlayerInput::SetProgress(0.0))
                    .ok();
                self.mini
                    .sender()
                    .send(MiniPlayerInput::SetCover(None))
                    .ok();
                self.detail = None;
                self.content
                    .sender()
                    .send(ContentStackInput::SetDetail(None))
                    .ok();
                match self.session.logout_blocking() {
                    Ok(path) => {
                        tracing::info!(?path, "logged out, token file removed");
                    }
                    Err(e) => {
                        tracing::warn!(error = %e, "logout failed");
                    }
                }
                self.apply_auth_result(None);
            }
            AppInput::OpenSettings => {
                tracing::info!("settings dialog requested (Phase 8)");
            }
            AppInput::OpenAbout => {
                tracing::info!("about dialog requested (Phase 8)");
            }
            AppInput::TransportPlay => {
                // Phase 7-D: if we already have a buffered URI, resume.
                // Otherwise the click is a no-op until the user picks a
                // track from a list.
                if let Some(engine) = self.engine.as_mut() {
                    if let Err(e) = engine.play() {
                        tracing::warn!(error = %e, "engine play failed");
                    } else {
                        self.model.playback.transport = TransportState::Playing;
                    }
                }
            }
            AppInput::TransportPause => {
                if let Some(engine) = self.engine.as_mut() {
                    if let Err(e) = engine.pause() {
                        tracing::warn!(error = %e, "engine pause failed");
                    } else {
                        self.model.playback.transport = TransportState::Paused;
                    }
                }
            }
            AppInput::TransportNext => {
                self.advance_queue(1, sender.clone());
            }
            AppInput::TransportPrev => {
                self.advance_queue(-1, sender.clone());
            }
            AppInput::TransportSeek(p) => {
                if let Some(engine) = self.engine.as_mut() {
                    let target = p.clamp(0.0, 1.0)
                        * self.model.playback.duration.as_secs_f64().max(0.0);
                    if let Err(e) = engine.seek_seconds(target) {
                        tracing::warn!(error = %e, "engine seek failed");
                    }
                }
            }

            AppInput::AuthRestoreResult(profile) => {
                self.apply_auth_result(profile);
                // If the user landed on a library page (typical case
                // when last_nav is e.g. "albums"), kick off its first
                // fetch now that auth is ready.
                if self.model.auth.is_logged_in() {
                    self.dispatch_view_refresh(self.model.current_nav);
                }
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

            AppInput::OpenAlbum { id, title } => {
                let parsed = id.parse::<i64>().ok();
                if let Some(album_id) = parsed {
                    self.open_album_detail(album_id, title, sender.clone());
                } else {
                    tracing::warn!(%id, "ignoring OpenAlbum: id is not a numeric album id");
                }
            }
            AppInput::OpenArtist { id, name } => {
                let parsed = id.parse::<i64>().ok();
                if let Some(artist_id) = parsed {
                    self.open_artist_detail(artist_id, name, sender.clone());
                } else {
                    tracing::warn!(%id, "ignoring OpenArtist: id is not a numeric artist id");
                }
            }
            AppInput::OpenPlaylist { uuid, title } => {
                self.open_playlist_detail(uuid, title, sender.clone());
            }
            AppInput::OpenMix { id, title } => {
                self.open_mix_detail(id, title, sender.clone());
            }
            AppInput::CloseDetail => {
                self.close_detail();
            }
            AppInput::PlayTrack { track_id } => {
                // Single-track play: clear the queue so Next/Prev can't
                // pull stale rows from a previous list.
                self.model.queue = Default::default();
                self.model.playback.source =
                    crate::state::playback::PlaybackSource::SingleTrack;
                self.start_play_track(track_id, sender.clone());
            }
            AppInput::PlayContext {
                tracks,
                start_index,
                source,
            } => {
                if let Some(track) = tracks.get(start_index) {
                    let track_id = track.id;
                    self.model.queue.tracks = tracks;
                    self.model.queue.current_index = start_index;
                    self.model.queue.shuffle_order = None;
                    self.model.playback.source = source;
                    self.start_play_track(track_id, sender.clone());
                }
            }
            AppInput::NowPlayingResolved {
                request_id,
                resolved,
            } => {
                self.handle_now_playing_resolved(request_id, resolved, sender.clone());
            }
            AppInput::NowPlayingFailed { request_id, error } => {
                if request_id != self.play_request_counter {
                    return;
                }
                tracing::warn!(request_id, error = %error, "play resolve failed");
                self.model.playback.transport = TransportState::Stopped;
            }
            AppInput::PlaybackTick => {
                self.tick_playback_position();
            }
            AppInput::NowPlayingCoverReady { request_id, path } => {
                if request_id != self.play_request_counter {
                    return;
                }
                self.mini
                    .sender()
                    .send(MiniPlayerInput::SetCover(Some(path)))
                    .ok();
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
        self.header
            .sender()
            .send(HeaderInput::SetDetailOpen(self.detail.is_some()))
            .ok();
        let is_playing = matches!(self.model.playback.transport, TransportState::Playing);
        self.mini
            .sender()
            .send(MiniPlayerInput::SetIsPlaying(is_playing))
            .ok();
    }
}

impl AppController {
    fn persist_settings(&self) {
        if let Err(e) = self.model.settings.save() {
            tracing::warn!(error = %e, "settings save failed");
        }
    }

    /// On navigation, ask the destination view to fetch its data. The
    /// view's own state machine ignores Refresh while Loading and reuses
    /// already-loaded results, so this is cheap to call on every nav.
    fn dispatch_view_refresh(&self, target: NavTarget) {
        if !self.model.auth.is_logged_in() {
            return;
        }
        match target {
            NavTarget::Albums => {
                self.albums_view.sender().send(AlbumsViewInput::Refresh).ok();
            }
            NavTarget::Tracks => {
                self.tracks_view.sender().send(TracksViewInput::Refresh).ok();
            }
            NavTarget::Artists => {
                self.artists_view.sender().send(ArtistsViewInput::Refresh).ok();
            }
            NavTarget::Playlists => {
                self.playlists_view.sender().send(PlaylistsViewInput::Refresh).ok();
            }
            NavTarget::MixesAndRadio => {
                self.mixes_view.sender().send(MixesViewInput::Refresh).ok();
            }
            NavTarget::History => {
                self.history_view.sender().send(HistoryViewInput::Refresh).ok();
            }
            NavTarget::Home => {
                self.home_view.sender().send(DiscoveryViewInput::Refresh).ok();
            }
            NavTarget::New => {
                self.new_view.sender().send(DiscoveryViewInput::Refresh).ok();
            }
            NavTarget::Top => {
                self.top_view.sender().send(DiscoveryViewInput::Refresh).ok();
            }
            NavTarget::HiRes => {
                self.hires_view.sender().send(DiscoveryViewInput::Refresh).ok();
            }
            NavTarget::Genres => {
                self.genres_view
                    .sender()
                    .send(TabbedDiscoveryInput::Refresh)
                    .ok();
            }
            NavTarget::Decades => {
                self.decades_view
                    .sender()
                    .send(TabbedDiscoveryInput::Refresh)
                    .ok();
            }
            NavTarget::Moods => {
                self.moods_view
                    .sender()
                    .send(TabbedDiscoveryInput::Refresh)
                    .ok();
            }
        }
    }

    /// Step the queue by `delta` (+1 = next, -1 = prev) and start
    /// playing the new track. PlayMode interactions (Loop / One /
    /// Shuffle / Smart) come in Phase 8 — for now this is straight
    /// linear walk with end-of-queue clamp + start-of-queue clamp.
    fn advance_queue(&mut self, delta: i32, sender: ComponentSender<Self>) {
        let queue = &self.model.queue;
        if queue.tracks.is_empty() {
            return;
        }
        let cur = queue.current_index as i32;
        let next = cur + delta;
        if next < 0 || next as usize >= queue.tracks.len() {
            tracing::debug!(delta, cur, "queue boundary — no advance");
            return;
        }
        let next_idx = next as usize;
        let Some(track) = queue.tracks.get(next_idx) else {
            return;
        };
        let track_id = track.id;
        self.model.queue.current_index = next_idx;
        self.start_play_track(track_id, sender);
    }

    fn tick_playback_position(&mut self) {
        let Some(engine) = self.engine.as_ref() else {
            return;
        };
        if !matches!(self.model.playback.transport, TransportState::Playing) {
            return;
        }
        let pos = engine.position_seconds().max(0.0);
        let dur = {
            let from_engine = engine.duration_seconds();
            if from_engine > 0.0 {
                from_engine
            } else {
                self.model.playback.duration.as_secs_f64().max(0.0)
            }
        };
        self.model.playback.position = std::time::Duration::from_secs_f64(pos);
        let fraction = if dur > 0.001 {
            (pos / dur).clamp(0.0, 1.0)
        } else {
            0.0
        };
        self.mini
            .sender()
            .send(MiniPlayerInput::SetProgress(fraction))
            .ok();
    }

    fn start_play_track(&mut self, track_id: i64, sender: ComponentSender<Self>) {
        self.play_request_counter = self.play_request_counter.wrapping_add(1);
        let request_id = self.play_request_counter;
        self.model.playback.transport = TransportState::Buffering;
        // Reset position so the seek bar doesn't show the previous
        // track's leftover progress while we wait for resolution.
        self.model.playback.position = std::time::Duration::ZERO;
        self.mini
            .sender()
            .send(MiniPlayerInput::SetNowPlaying {
                title: format!("Loading track {track_id}…"),
                artist: String::new(),
            })
            .ok();
        self.mini
            .sender()
            .send(MiniPlayerInput::SetProgress(0.0))
            .ok();
        // Drop the previous track's cover so the mini-player doesn't
        // show the wrong artwork during the resolve gap.
        self.mini
            .sender()
            .send(MiniPlayerInput::SetCover(None))
            .ok();

        let svc = self.session.clone();
        // Phase 7-C uses HI_RES_LOSSLESS unconditionally. Phase 8 wires
        // the quality setting + per-track downgrade fallback chain that
        // Python's _resolve_quality_chain handles.
        let quality = "HI_RES_LOSSLESS".to_string();
        let sender_in = sender.input_sender().clone();
        spawn_blocking(
            move || svc.resolve_playback_blocking(track_id, &quality),
            move |result| {
                let msg = match result {
                    Ok(resolved) => AppInput::NowPlayingResolved {
                        request_id,
                        resolved,
                    },
                    Err(e) => AppInput::NowPlayingFailed {
                        request_id,
                        error: e.to_string(),
                    },
                };
                let _ = sender_in.send(msg);
            },
        );
    }

    fn handle_now_playing_resolved(
        &mut self,
        request_id: u64,
        resolved: ResolvedPlayback,
        sender: ComponentSender<Self>,
    ) {
        if request_id != self.play_request_counter {
            tracing::debug!(request_id, "stale play resolve dropped");
            return;
        }
        let track = &resolved.track;
        let title = if track.name.is_empty() {
            format!("Track {}", track.id)
        } else {
            track.name.clone()
        };
        let artist = crate::components::views::common::track_artist_name(track);
        tracing::info!(
            track_id = track.id,
            quality = %resolved.quality,
            sample_rate = resolved.sample_rate,
            bit_depth = resolved.bit_depth,
            url_present = resolved.url.is_some(),
            mpd = resolved.is_mpd,
            "playback resolved"
        );
        self.mini
            .sender()
            .send(MiniPlayerInput::SetNowPlaying {
                title: title.clone(),
                artist: artist.clone(),
            })
            .ok();
        self.model.playback.current_track = Some(track.clone());
        self.model.playback.duration =
            std::time::Duration::from_secs(track.duration.max(0) as u64);

        // Kick off the mini-player cover fetch in parallel with the
        // engine handoff. Album cover lives on the album-ref so we can
        // grab it without an extra fetch_album call.
        if let Some(cover_id) = track
            .album
            .as_ref()
            .and_then(|a| a.cover.clone())
            .filter(|s| !s.is_empty())
        {
            let req = request_id;
            let app_in = sender.input_sender().clone();
            spawn_blocking(
                move || crate::services::covers::fetch_cover_blocking(&cover_id, 80),
                move |result| {
                    if let Ok(path) = result {
                        let _ = app_in.send(AppInput::NowPlayingCoverReady {
                            request_id: req,
                            path,
                        });
                    }
                },
            );
        }

        // Hand the URL to rust_audio_core. The resolver already falls
        // back to the legacy URL endpoint for MPD manifests so we
        // expect a single playable URL here for both BTS and MPD; if
        // the fallback was suppressed (PKCE token + hi-res quality)
        // there'll be no URL and we degrade to a stop.
        let Some(url) = resolved.url else {
            tracing::warn!(
                track_id = track.id,
                is_mpd = resolved.is_mpd,
                "stream resolved without a URL"
            );
            self.model.playback.transport = TransportState::Stopped;
            return;
        };
        let Some(engine) = self.engine.as_mut() else {
            tracing::info!(track_id = track.id, %url, "no audio engine — staying in Buffering");
            return;
        };
        engine.set_uri_str(&url);
        match engine.play() {
            Ok(()) => {
                self.model.playback.transport = TransportState::Playing;
            }
            Err(e) => {
                tracing::warn!(error = %e, "engine.play failed");
                self.model.playback.transport = TransportState::Stopped;
            }
        }
    }

    fn open_album_detail(
        &mut self,
        album_id: i64,
        title: String,
        sender: ComponentSender<Self>,
    ) {
        let lib_forward = make_lib_forward();
        let view = AlbumDetailViewModel::builder()
            .launch(AlbumDetailInit {
                session: self.session.clone(),
                album_id,
                initial_title: title,
            })
            .forward(sender.input_sender(), lib_forward);
        self.install_detail(DetailPage::Album(view));
    }

    fn open_playlist_detail(
        &mut self,
        playlist_id: String,
        title: String,
        sender: ComponentSender<Self>,
    ) {
        let lib_forward = make_lib_forward();
        let view = PlaylistDetailViewModel::builder()
            .launch(PlaylistDetailInit {
                session: self.session.clone(),
                playlist_id,
                initial_title: title,
            })
            .forward(sender.input_sender(), lib_forward);
        self.install_detail(DetailPage::Playlist(view));
    }

    fn open_mix_detail(
        &mut self,
        mix_id: String,
        title: String,
        sender: ComponentSender<Self>,
    ) {
        let lib_forward = make_lib_forward();
        let view = MixDetailViewModel::builder()
            .launch(MixDetailInit {
                session: self.session.clone(),
                mix_id,
                initial_title: title,
            })
            .forward(sender.input_sender(), lib_forward);
        self.install_detail(DetailPage::Mix(view));
    }

    fn open_artist_detail(
        &mut self,
        artist_id: i64,
        name: String,
        sender: ComponentSender<Self>,
    ) {
        let lib_forward = make_lib_forward();
        let view = ArtistDetailViewModel::builder()
            .launch(ArtistDetailInit {
                session: self.session.clone(),
                artist_id,
                initial_name: name,
            })
            .forward(sender.input_sender(), lib_forward);
        self.install_detail(DetailPage::Artist(view));
    }

    fn install_detail(&mut self, page: DetailPage) {
        let widget = page.widget();
        // Drop the previous controller (if any) before installing the new
        // one — its widget is still in the stack until SetDetail removes
        // it, but ContentStack handles the swap atomically.
        self.detail = Some(page);
        self.content
            .sender()
            .send(ContentStackInput::SetDetail(Some(widget)))
            .ok();
    }

    fn close_detail(&mut self) {
        if self.detail.is_none() {
            return;
        }
        self.content
            .sender()
            .send(ContentStackInput::SetDetail(None))
            .ok();
        self.detail = None;
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

/// LibraryViewOutput → AppInput translator. Every library and detail
/// view forwards through this so a click on an artist link inside an
/// album-detail page reaches the same root handler as a click on an
/// artist tile in the Artists library page.
fn make_lib_forward() -> impl Fn(LibraryViewOutput) -> AppInput + 'static + Copy {
    |out| match out {
        LibraryViewOutput::OpenAlbum { id, title } => AppInput::OpenAlbum { id, title },
        LibraryViewOutput::OpenArtist { id, name } => AppInput::OpenArtist { id, name },
        LibraryViewOutput::OpenPlaylist { uuid, title } => {
            AppInput::OpenPlaylist { uuid, title }
        }
        LibraryViewOutput::OpenMix { id, title } => AppInput::OpenMix { id, title },
        LibraryViewOutput::PlayTrack { track_id } => AppInput::PlayTrack { track_id },
        LibraryViewOutput::PlayContext {
            tracks,
            start_index,
            source,
        } => AppInput::PlayContext {
            tracks,
            start_index,
            source,
        },
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
