//! System tray icon. Linux StatusNotifierItem via [`ksni`] running on
//! its own thread; menu activations forward back to the GTK main loop
//! through a relm4 `Sender<AppInput>` (Sender is Send + Clone, so the
//! ksni callbacks can capture clones freely).
//!
//! On systems without a StatusNotifierWatcher (no tray host running),
//! `start` still succeeds — ksni keeps polling for a watcher to come
//! online. `start` only returns an error if the D-Bus session itself
//! is unavailable, in which case the UI runs without tray support.

use ksni::blocking::{Handle, TrayMethods};
use ksni::menu::{MenuItem, StandardItem};
use ksni::{Icon, Tray};
use relm4::Sender;

use crate::messages::AppInput;

/// Tray icon model. Holds the AppInput sender so menu callbacks can
/// post user actions back to the GTK main loop.
pub struct HirestiTray {
    sender: Sender<AppInput>,
    is_playing: bool,
}

impl Tray for HirestiTray {
    const MENU_ON_ACTIVATE: bool = false;

    fn id(&self) -> String {
        "com.hiresti.player".into()
    }

    fn title(&self) -> String {
        "HiresTI".into()
    }

    fn icon_name(&self) -> String {
        "audio-x-generic-symbolic".into()
    }

    fn icon_pixmap(&self) -> Vec<Icon> {
        // Defer to icon_name; pixmap fallback only used when icon-name
        // can't be resolved by the host (rare on modern desktops).
        Vec::new()
    }

    fn activate(&mut self, _x: i32, _y: i32) {
        // Left-click toggles window visibility.
        let _ = self.sender.send(AppInput::TrayShow);
    }

    fn menu(&self) -> Vec<MenuItem<Self>> {
        let play_label = if self.is_playing {
            "Pause"
        } else {
            "Play"
        };
        vec![
            StandardItem {
                label: "Show window".into(),
                icon_name: "view-restore-symbolic".into(),
                activate: Box::new(|t: &mut Self| {
                    let _ = t.sender.send(AppInput::TrayShow);
                }),
                ..Default::default()
            }
            .into(),
            MenuItem::Separator,
            StandardItem {
                label: play_label.into(),
                icon_name: "media-playback-start-symbolic".into(),
                activate: Box::new(|t: &mut Self| {
                    let _ = t.sender.send(AppInput::TogglePlayPause);
                }),
                ..Default::default()
            }
            .into(),
            StandardItem {
                label: "Next".into(),
                icon_name: "media-skip-forward-symbolic".into(),
                activate: Box::new(|t: &mut Self| {
                    let _ = t.sender.send(AppInput::TransportNext);
                }),
                ..Default::default()
            }
            .into(),
            StandardItem {
                label: "Previous".into(),
                icon_name: "media-skip-backward-symbolic".into(),
                activate: Box::new(|t: &mut Self| {
                    let _ = t.sender.send(AppInput::TransportPrev);
                }),
                ..Default::default()
            }
            .into(),
            MenuItem::Separator,
            StandardItem {
                label: "Quit".into(),
                icon_name: "application-exit-symbolic".into(),
                activate: Box::new(|t: &mut Self| {
                    let _ = t.sender.send(AppInput::TrayQuit);
                }),
                ..Default::default()
            }
            .into(),
        ]
    }
}

/// Owned handle returned by [`start`]. Drop it to tear the tray down.
pub struct TrayHandle {
    inner: Option<Handle<HirestiTray>>,
}

impl TrayHandle {
    /// Push a transport-state update into the tray so the menu's
    /// Play/Pause label flips. Cheap — the actual D-Bus update happens
    /// inside the ksni thread.
    pub fn set_is_playing(&self, playing: bool) {
        if let Some(h) = self.inner.as_ref() {
            h.update(|t| {
                t.is_playing = playing;
            });
        }
    }
}

impl Drop for TrayHandle {
    fn drop(&mut self) {
        if let Some(h) = self.inner.take() {
            h.shutdown().wait();
        }
    }
}

/// Spawn the tray on its dedicated thread. Returns Err if the D-Bus
/// session isn't reachable; missing StatusNotifierWatcher (no tray
/// host) is *not* an error — ksni keeps trying in the background.
pub fn start(sender: Sender<AppInput>) -> Result<TrayHandle, String> {
    let tray = HirestiTray {
        sender,
        is_playing: false,
    };
    match tray.spawn() {
        Ok(handle) => Ok(TrayHandle {
            inner: Some(handle),
        }),
        Err(e) => Err(format!("ksni spawn: {e}")),
    }
}
