//! MPRIS2 D-Bus integration. Lets KDE/GNOME media keys, Plasma's
//! "play queue" widget, GNOME's status-bar player, and any other
//! MPRIS-aware client drive transport + read what we're playing.
//!
//! Architecture mirrors `tray`: a dedicated thread owns the
//! `mpris_server::Player` (which is built on `LocalServer` and is
//! therefore `!Send`) and runs an async-io event loop. State updates
//! from the GTK main thread go through an `async_channel` of
//! `MprisCommand`; incoming D-Bus method calls come back via the
//! `relm4::Sender<AppInput>` we hand in at start.
//!
//! Bus name is `org.mpris.MediaPlayer2.hiresti` — same suffix the
//! Python build registered, so existing user shortcuts and KWin
//! rules stay valid.

use std::path::PathBuf;
use std::thread;
use std::time::Duration;

use async_channel::{unbounded, Sender as AsyncSender};
use mpris_server::{Metadata, PlaybackStatus, Player, Time, TrackId};
use relm4::Sender;

use crate::messages::AppInput;

const BUS_SUFFIX: &str = "hiresti";

/// State pushed from the GTK side into the MPRIS thread. The thread
/// applies these against the `Player` instance; mpris-server emits
/// the `PropertiesChanged` D-Bus signal automatically when `set_*`
/// is called with a new value.
#[derive(Debug, Clone)]
pub enum MprisCommand {
    SetTransport(MprisTransport),
    SetMetadata(MprisMetadata),
    SetPosition {
        seconds: f64,
        emit_seeked: bool,
    },
    SetVolume(f64),
    SetCan {
        play: bool,
        pause: bool,
        next: bool,
        previous: bool,
        seek: bool,
    },
    Stop,
    Shutdown,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MprisTransport {
    Stopped,
    Playing,
    Paused,
}

impl From<MprisTransport> for PlaybackStatus {
    fn from(t: MprisTransport) -> Self {
        match t {
            MprisTransport::Stopped => PlaybackStatus::Stopped,
            MprisTransport::Playing => PlaybackStatus::Playing,
            MprisTransport::Paused => PlaybackStatus::Paused,
        }
    }
}

#[derive(Debug, Clone, Default)]
pub struct MprisMetadata {
    pub track_id: Option<i64>,
    pub title: String,
    pub artist: String,
    pub album: String,
    pub album_artist: String,
    pub duration_seconds: f64,
    /// Local file path to the album cover (if downloaded). Converted to
    /// a `file://` URI for the `mpris:artUrl` field.
    pub art_path: Option<PathBuf>,
}

/// Owned handle returned by [`start`]. Drop it to shut the MPRIS
/// thread down (a `Shutdown` command is queued; the thread exits
/// its loop and `join`s).
pub struct MprisHandle {
    cmd_tx: AsyncSender<MprisCommand>,
    join: Option<thread::JoinHandle<()>>,
}

impl MprisHandle {
    fn send(&self, cmd: MprisCommand) {
        // try_send is non-blocking; the channel is unbounded so it
        // only fails when the receiver is gone — at which point the
        // MPRIS thread is shutting down and there's nothing to do.
        let _ = self.cmd_tx.try_send(cmd);
    }

    pub fn set_transport(&self, t: MprisTransport) {
        self.send(MprisCommand::SetTransport(t));
    }

    pub fn set_metadata(&self, m: MprisMetadata) {
        self.send(MprisCommand::SetMetadata(m));
    }

    pub fn set_position(&self, seconds: f64, emit_seeked: bool) {
        self.send(MprisCommand::SetPosition {
            seconds,
            emit_seeked,
        });
    }

    pub fn set_volume(&self, v: f64) {
        self.send(MprisCommand::SetVolume(v));
    }

    pub fn set_can(&self, play: bool, pause: bool, next: bool, previous: bool, seek: bool) {
        self.send(MprisCommand::SetCan {
            play,
            pause,
            next,
            previous,
            seek,
        });
    }

    pub fn stop(&self) {
        self.send(MprisCommand::Stop);
    }
}

impl Drop for MprisHandle {
    fn drop(&mut self) {
        let _ = self.cmd_tx.try_send(MprisCommand::Shutdown);
        if let Some(j) = self.join.take() {
            // Don't block forever — the async-io tasks should bail
            // promptly once Shutdown lands, but a stuck thread should
            // not freeze quit; the OS will clean up on process exit.
            let _ = j.join();
        }
    }
}

/// Spawn the MPRIS thread. Returns `Err` only when zbus session
/// connection fails (no D-Bus, sandboxed env). The caller treats
/// that as "no MPRIS" and continues without it.
pub fn start(sender: Sender<AppInput>) -> Result<MprisHandle, String> {
    let (cmd_tx, cmd_rx) = unbounded::<MprisCommand>();
    let cmd_tx_for_handle = cmd_tx.clone();

    // Use a one-shot channel to surface Player::build errors
    // synchronously so the caller can decide whether to log + skip.
    let (init_tx, init_rx) = std::sync::mpsc::channel::<Result<(), String>>();

    let join = thread::Builder::new()
        .name("mpris".into())
        .spawn(move || {
            async_io::block_on(async move {
                let player = match build_player(sender.clone()).await {
                    Ok(p) => p,
                    Err(e) => {
                        let _ = init_tx.send(Err(e));
                        return;
                    }
                };
                let _ = init_tx.send(Ok(()));

                run_loop(&player, cmd_rx).await;
            });
        })
        .map_err(|e| format!("mpris thread spawn: {e}"))?;

    match init_rx.recv_timeout(Duration::from_secs(3)) {
        Ok(Ok(())) => Ok(MprisHandle {
            cmd_tx: cmd_tx_for_handle,
            join: Some(join),
        }),
        Ok(Err(e)) => Err(e),
        Err(_) => Err("mpris init timeout".into()),
    }
}

async fn build_player(sender: Sender<AppInput>) -> Result<Player, String> {
    let player = Player::builder(BUS_SUFFIX)
        .identity("HiresTI")
        .desktop_entry("hiresti")
        .can_quit(true)
        .can_raise(true)
        .can_control(true)
        .can_play(false)
        .can_pause(false)
        .can_go_next(false)
        .can_go_previous(false)
        .can_seek(false)
        .build()
        .await
        .map_err(|e| format!("mpris build: {e}"))?;

    // Wire incoming D-Bus method calls. mpris-server fires the
    // closures on the same async-io thread; we just forward the
    // intent into the GTK main loop through the relm4 Sender
    // (Send + Clone — captured once, cloned per closure).
    let s = sender.clone();
    player.connect_play_pause(move |_| {
        let _ = s.send(AppInput::TogglePlayPause);
    });
    let s = sender.clone();
    player.connect_play(move |_| {
        let _ = s.send(AppInput::TransportPlay);
    });
    let s = sender.clone();
    player.connect_pause(move |_| {
        let _ = s.send(AppInput::TransportPause);
    });
    let s = sender.clone();
    player.connect_stop(move |_| {
        // No dedicated AppInput::Stop yet — pause is the closest
        // friendly translation. Phase 10-B can refine if the
        // scrobbler wants explicit Stop semantics.
        let _ = s.send(AppInput::TransportPause);
    });
    let s = sender.clone();
    player.connect_next(move |_| {
        let _ = s.send(AppInput::TransportNext);
    });
    let s = sender.clone();
    player.connect_previous(move |_| {
        let _ = s.send(AppInput::TransportPrev);
    });
    let s = sender.clone();
    player.connect_raise(move |_| {
        let _ = s.send(AppInput::TrayShow);
    });
    let s = sender.clone();
    player.connect_quit(move |_| {
        let _ = s.send(AppInput::TrayQuit);
    });

    Ok(player)
}

async fn run_loop(player: &Player, cmd_rx: async_channel::Receiver<MprisCommand>) {
    // mpris-server's Player::run is the long-lived task that pumps
    // the zbus connection. Race it against our command receiver so
    // a Shutdown unblocks immediately. `futures-lite` is already in
    // the dep tree via async-io, no extra dep needed.
    let cmd_loop = async {
        loop {
            let Ok(cmd) = cmd_rx.recv().await else {
                return;
            };
            match cmd {
                MprisCommand::Shutdown => return,
                MprisCommand::SetTransport(t) => {
                    let _ = player.set_playback_status(PlaybackStatus::from(t)).await;
                }
                MprisCommand::SetMetadata(m) => {
                    let mut md = Metadata::builder();
                    if let Some(id) = m.track_id {
                        if let Ok(tid) = TrackId::try_from(format!(
                            "/com/hiresti/track/{}",
                            id.unsigned_abs()
                        )) {
                            md = md.trackid(tid);
                        }
                    }
                    if !m.title.is_empty() {
                        md = md.title(&m.title);
                    }
                    if !m.artist.is_empty() {
                        md = md.artist([m.artist.clone()]);
                    }
                    if !m.album.is_empty() {
                        md = md.album(&m.album);
                    }
                    if !m.album_artist.is_empty() {
                        md = md.album_artist([m.album_artist.clone()]);
                    }
                    if m.duration_seconds > 0.0 {
                        md = md.length(seconds_to_time(m.duration_seconds));
                    }
                    if let Some(p) = m.art_path.as_ref() {
                        if let Some(uri) = path_to_file_uri(p) {
                            md = md.art_url(uri);
                        }
                    }
                    let _ = player.set_metadata(md.build()).await;
                }
                MprisCommand::SetPosition {
                    seconds,
                    emit_seeked,
                } => {
                    let t = seconds_to_time(seconds);
                    player.set_position(t);
                    if emit_seeked {
                        let _ = player.seeked(t).await;
                    }
                }
                MprisCommand::SetVolume(v) => {
                    let _ = player.set_volume(v).await;
                }
                MprisCommand::SetCan {
                    play,
                    pause,
                    next,
                    previous,
                    seek,
                } => {
                    let _ = player.set_can_play(play).await;
                    let _ = player.set_can_pause(pause).await;
                    let _ = player.set_can_go_next(next).await;
                    let _ = player.set_can_go_previous(previous).await;
                    let _ = player.set_can_seek(seek).await;
                }
                MprisCommand::Stop => {
                    let _ = player.set_playback_status(PlaybackStatus::Stopped).await;
                }
            }
        }
    };

    // Player::run() returns a LocalServerRunTask future that drives
    // the zbus dispatch. Whichever future finishes first ends the
    // thread — typically cmd_loop on Shutdown.
    let run = async {
        player.run().await;
    };
    futures_lite::future::race(run, cmd_loop).await;
}

fn seconds_to_time(secs: f64) -> Time {
    let micros = (secs.max(0.0) * 1_000_000.0).round() as i64;
    Time::from_micros(micros)
}

fn path_to_file_uri(p: &std::path::Path) -> Option<String> {
    let s = p.to_str()?;
    // Minimal RFC 8089 escaping: just the bare path. Cover-cache
    // filenames are hex-derived (services::covers writes them) so
    // there are no spaces or non-ASCII chars to escape.
    Some(format!("file://{s}"))
}
