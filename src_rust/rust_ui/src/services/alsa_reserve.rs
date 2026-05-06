//! `org.freedesktop.ReserveDevice1` D-Bus implementation. Lets the
//! app politely take exclusive ownership of an ALSA card so PipeWire /
//! WirePlumber back off and free `hw:N,0` for direct access.
//!
//! Mirrors the Python `services.alsa_reserve.AlsaDeviceReservation`
//! behavior: priority 20 (above WirePlumber's default 0), bus-name
//! takeover with `REPLACE_EXISTING`, owner-released yields when a
//! higher-priority requester asks.
//!
//! Architecture follows the other long-lived D-Bus services (tray,
//! mpris): a dedicated `async-io` thread owns the zbus connection;
//! `acquire` blocks the caller's thread up to the timeout via a
//! one-shot reply channel.

use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

use async_channel::{unbounded, Receiver as AsyncRx, Sender as AsyncTx};
use zbus::{
    fdo,
    interface,
    Connection,
};

const PRIORITY: i32 = 20;
const APP_NAME: &str = "hiresTI";

/// Inbound commands the reserver thread executes against zbus.
#[derive(Debug)]
enum Cmd {
    Acquire {
        card: u32,
        timeout: Duration,
        reply: std::sync::mpsc::Sender<bool>,
    },
    Release {
        reply: std::sync::mpsc::Sender<()>,
    },
    Shutdown,
}

/// Object exposed on `/org/freedesktop/ReserveDevice1/AudioN`.
struct ReserveObject {
    card: u32,
    /// Set by the worker when the bus name is yielded — the worker
    /// loop polls this between commands so a peer's `RequestRelease`
    /// causes us to drop the reservation cleanly.
    yielded: Arc<Mutex<bool>>,
}

#[interface(name = "org.freedesktop.ReserveDevice1")]
impl ReserveObject {
    /// Higher-priority requesters win — we mark ourselves yielded and
    /// the worker drops the bus name on the next tick. Equal/lower
    /// priority requests are refused.
    async fn request_release(&self, priority: i32) -> bool {
        if priority > PRIORITY {
            tracing::info!(
                card = self.card,
                priority,
                "ALSA reserve: yielding to higher-priority requester"
            );
            if let Ok(mut y) = self.yielded.lock() {
                *y = true;
            }
            true
        } else {
            false
        }
    }

    #[zbus(property)]
    fn priority(&self) -> i32 {
        PRIORITY
    }

    #[zbus(property)]
    fn application_name(&self) -> String {
        APP_NAME.into()
    }

    #[zbus(property)]
    fn application_device_name(&self) -> String {
        format!("Audio{}", self.card)
    }
}

/// Shared handle. Cheap to clone; backed by an async_channel sender.
#[derive(Clone)]
pub struct AlsaReserveService {
    tx: AsyncTx<Cmd>,
    /// Last successfully-acquired card number, if any. Tracked so
    /// `release` knows whether there's anything to give back without
    /// forcing the caller to remember.
    held: Arc<Mutex<Option<u32>>>,
}

impl AlsaReserveService {
    /// Best-effort acquire. Returns `false` when D-Bus isn't usable
    /// or the takeover times out — the caller should fall through to
    /// non-exclusive playback in that case.
    pub fn acquire(&self, card: u32, timeout: Duration) -> bool {
        let (reply_tx, reply_rx) = std::sync::mpsc::channel();
        if self
            .tx
            .try_send(Cmd::Acquire {
                card,
                timeout,
                reply: reply_tx,
            })
            .is_err()
        {
            return false;
        }
        match reply_rx.recv_timeout(timeout + Duration::from_millis(500)) {
            Ok(true) => {
                if let Ok(mut h) = self.held.lock() {
                    *h = Some(card);
                }
                true
            }
            _ => false,
        }
    }

    pub fn release(&self) {
        if !matches!(self.held.lock(), Ok(ref h) if h.is_some()) {
            return;
        }
        let (reply_tx, reply_rx) = std::sync::mpsc::channel();
        if self.tx.try_send(Cmd::Release { reply: reply_tx }).is_err() {
            return;
        }
        let _ = reply_rx.recv_timeout(Duration::from_secs(2));
        if let Ok(mut h) = self.held.lock() {
            *h = None;
        }
    }

    pub fn is_held(&self) -> bool {
        matches!(self.held.lock(), Ok(ref h) if h.is_some())
    }
}

impl Drop for AlsaReserveService {
    fn drop(&mut self) {
        // Best-effort: ask the worker to release any active hold and
        // then shut down. If the channel is already gone the worker
        // is dead and the OS will clean up.
        let _ = self.tx.try_send(Cmd::Shutdown);
    }
}

/// Spawn the worker thread. Returns `Err` only if the dedicated
/// thread fails to start; D-Bus connection failures are handled
/// per-`acquire` so the UI can still operate without ALSA reserve.
pub fn start() -> Result<AlsaReserveService, String> {
    let (tx, rx) = unbounded::<Cmd>();
    let held = Arc::new(Mutex::new(None));

    thread::Builder::new()
        .name("alsa-reserve".into())
        .spawn(move || {
            async_io::block_on(async move {
                run_loop(rx).await;
            });
        })
        .map_err(|e| format!("alsa-reserve thread spawn: {e}"))?;

    Ok(AlsaReserveService { tx, held })
}

async fn run_loop(rx: AsyncRx<Cmd>) {
    let mut active: Option<ActiveReservation> = None;
    while let Ok(cmd) = rx.recv().await {
        match cmd {
            Cmd::Shutdown => break,
            Cmd::Acquire {
                card,
                timeout,
                reply,
            } => {
                // If we already hold this card, succeed immediately
                // (idempotent). For a different card the previous
                // hold is dropped first.
                if let Some(a) = active.as_ref() {
                    if a.card == card {
                        let _ = reply.send(true);
                        continue;
                    }
                }
                if let Some(a) = active.take() {
                    a.release().await;
                }
                let result = acquire(card, timeout).await;
                let ok = result.is_some();
                active = result;
                let _ = reply.send(ok);
            }
            Cmd::Release { reply } => {
                if let Some(a) = active.take() {
                    a.release().await;
                }
                let _ = reply.send(());
            }
        }
        // Check for a yield from a higher-priority RequestRelease
        // arriving between commands. If yielded we drop the hold so
        // the peer's wait finishes promptly.
        if let Some(a) = active.as_ref() {
            if a.is_yielded() {
                if let Some(a) = active.take() {
                    a.release().await;
                }
            }
        }
    }
    if let Some(a) = active.take() {
        a.release().await;
    }
}

struct ActiveReservation {
    card: u32,
    conn: Connection,
    yielded: Arc<Mutex<bool>>,
}

impl ActiveReservation {
    fn is_yielded(&self) -> bool {
        matches!(self.yielded.lock(), Ok(ref y) if **y)
    }

    async fn release(self) {
        let bus_name = format!("org.freedesktop.ReserveDevice1.Audio{}", self.card);
        let proxy = match fdo::DBusProxy::new(&self.conn).await {
            Ok(p) => p,
            Err(e) => {
                tracing::debug!(card = self.card, error = %e, "alsa-reserve: DBusProxy unavailable on release");
                return;
            }
        };
        match proxy.release_name(bus_name.as_str().try_into().unwrap()).await {
            Ok(_) => tracing::info!(card = self.card, "alsa-reserve: released"),
            Err(e) => tracing::debug!(card = self.card, error = %e, "alsa-reserve: release failed"),
        }
    }
}

async fn acquire(card: u32, timeout: Duration) -> Option<ActiveReservation> {
    let bus_name = format!("org.freedesktop.ReserveDevice1.Audio{card}");
    let object_path = format!("/org/freedesktop/ReserveDevice1/Audio{card}");

    let conn = match Connection::session().await {
        Ok(c) => c,
        Err(e) => {
            tracing::warn!(error = %e, "alsa-reserve: session bus unavailable");
            return None;
        }
    };

    // Step 1: politely ask the current owner to release. Failures
    // are non-fatal — RequestName below will replace the owner if
    // the daemon allows it.
    let release_timeout = (timeout / 4).max(Duration::from_millis(150));
    let _ = request_release_from_owner(&conn, &bus_name, &object_path, release_timeout).await;

    // Step 2: register our object before the takeover so any peer
    // calling RequestRelease on us hits the implementation.
    let yielded = Arc::new(Mutex::new(false));
    let obj = ReserveObject {
        card,
        yielded: Arc::clone(&yielded),
    };
    if let Err(e) = conn.object_server().at(object_path.as_str(), obj).await {
        tracing::warn!(error = %e, card, "alsa-reserve: register object failed");
        return None;
    }

    // Step 3: claim the bus name with REPLACE_EXISTING |
    // ALLOW_REPLACEMENT — symmetric with how PipeWire holds it.
    let flags = fdo::RequestNameFlags::ReplaceExisting | fdo::RequestNameFlags::AllowReplacement;
    let proxy = match fdo::DBusProxy::new(&conn).await {
        Ok(p) => p,
        Err(e) => {
            tracing::warn!(error = %e, "alsa-reserve: DBusProxy unavailable");
            let _ = conn.object_server().remove::<ReserveObject, _>(object_path.as_str()).await;
            return None;
        }
    };
    let bus_well_known = match bus_name.as_str().try_into() {
        Ok(n) => n,
        Err(e) => {
            tracing::warn!(error = %e, %bus_name, "alsa-reserve: invalid bus name");
            let _ = conn.object_server().remove::<ReserveObject, _>(object_path.as_str()).await;
            return None;
        }
    };
    match proxy.request_name(bus_well_known, flags.into()).await {
        Ok(fdo::RequestNameReply::PrimaryOwner) | Ok(fdo::RequestNameReply::AlreadyOwner) => {}
        Ok(other) => {
            tracing::warn!(?other, card, "alsa-reserve: RequestName unexpected reply");
            let _ = conn.object_server().remove::<ReserveObject, _>(object_path.as_str()).await;
            return None;
        }
        Err(e) => {
            tracing::warn!(error = %e, card, "alsa-reserve: RequestName failed");
            let _ = conn.object_server().remove::<ReserveObject, _>(object_path.as_str()).await;
            return None;
        }
    }
    // PipeWire needs a moment to fully close the ALSA device.
    async_io::Timer::after(Duration::from_millis(180)).await;
    tracing::info!(card, %bus_name, "alsa-reserve: acquired");
    Some(ActiveReservation {
        card,
        conn,
        yielded,
    })
}

async fn request_release_from_owner(
    conn: &Connection,
    bus_name: &str,
    object_path: &str,
    timeout: Duration,
) -> Result<(), zbus::Error> {
    let dbus = fdo::DBusProxy::new(conn).await?;
    let owner = match dbus
        .get_name_owner(bus_name.try_into().map_err(zbus::Error::from)?)
        .await
    {
        Ok(o) => o,
        Err(_) => return Ok(()), // Nobody owns it — nothing to do.
    };
    // The peer-specific ReserveDevice1 proxy is a one-shot call; we
    // construct it on the fly with the owner's unique name as
    // destination.
    let proxy = zbus::proxy::Builder::<zbus::Proxy<'_>>::new(conn)
        .destination(owner.as_str())?
        .path(object_path)?
        .interface("org.freedesktop.ReserveDevice1")?
        .build()
        .await?;
    let _ = async_io::Timer::after(timeout / 8).await;
    let _: bool = proxy
        .call("RequestRelease", &(PRIORITY,))
        .await
        .unwrap_or(false);
    Ok(())
}
