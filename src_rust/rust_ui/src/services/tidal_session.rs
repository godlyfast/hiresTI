//! Wraps `rust_tidal_core::api::Session` with the threading + persistence
//! shims the UI needs. Session methods block on HTTP so every operation
//! that touches the network spawns a dedicated thread; results are
//! delivered back to the UI via the Relm4 sender supplied by the caller.
//!
//! The Session itself is wrapped in `Arc` so multiple in-flight worker
//! threads (token refresh + active fetch + login poll) can hold it
//! simultaneously. `Session` already has internal locking for its
//! mutable state, so an Arc is enough.

use std::path::PathBuf;
use std::sync::Arc;
use std::thread;

use rust_tidal_core::api::{
    read_persisted_token, write_persisted_token, Album, Artist, Bio, DeviceLogin, ListArgs, Mix,
    Page, PersistedToken, Playlist, RequestArgs, RtcError, Session, Track, UserInfo,
};

/// Resolved playback bundle: track metadata + first usable stream URL.
/// `quality` echoes back what the manifest actually delivered (TIDAL
/// downgrades quality silently if the requested level isn't available
/// for the track), and `is_mpd` flags MPEG-DASH manifests so callers
/// know to feed the engine the manifest XML instead of a single URL.
#[derive(Debug, Clone)]
pub struct ResolvedPlayback {
    pub track: Track,
    pub url: Option<String>,
    /// MPD/DASH manifest XML when `is_mpd` is true. Phase 7-D feeds this
    /// to a DASH-aware native transport instead of the single URL above.
    #[allow(dead_code)]
    pub mpd_manifest: Option<String>,
    pub quality: String,
    pub sample_rate: i32,
    pub bit_depth: i32,
    /// True when the manifest is the modern BTS shape (single URL list).
    /// Phase 7-D consults this to pick the playback strategy.
    #[allow(dead_code)]
    pub is_bts: bool,
    pub is_mpd: bool,
}

use crate::error::{AppError, AppResult};
use crate::paths;

const TOKEN_FILE: &str = "hiresti_token.json";
/// HTTP connection pool size. Matches the Python default (`HIRESTI_HTTP_POOL_SIZE`).
const POOL_SIZE: usize = 64;

#[derive(Clone)]
pub struct TidalSessionService {
    session: Arc<Session>,
}

impl TidalSessionService {
    pub fn new() -> Self {
        Self {
            session: Arc::new(Session::new(POOL_SIZE)),
        }
    }

    /// Reserved for Phase 5+ when data-fetch workers need a direct
    /// Session handle. Allow dead until then.
    #[allow(dead_code)]
    pub fn handle(&self) -> Arc<Session> {
        Arc::clone(&self.session)
    }

    pub fn token_path() -> AppResult<PathBuf> {
        Ok(paths::config_dir()?.join(TOKEN_FILE))
    }

    /// Read the persisted token from disk if present. Returns `Ok(None)`
    /// when the file simply doesn't exist (first run / logged out).
    pub fn load_persisted() -> AppResult<Option<PersistedToken>> {
        let path = Self::token_path()?;
        if !path.exists() {
            return Ok(None);
        }
        match read_persisted_token(&path) {
            Ok(t) => Ok(Some(t)),
            Err(e) => {
                tracing::warn!(?path, error = %e, "token file present but unreadable");
                Ok(None)
            }
        }
    }

    /// Save the current Session's token to disk atomically. Match the
    /// Python `save_session()` behavior: we round-trip via
    /// `token_snapshot` → `PersistedToken::from_token` so the file
    /// format stays identical across both implementations.
    pub fn save_persisted(&self) -> AppResult<()> {
        let token = self
            .session
            .token_snapshot()
            .ok_or_else(|| AppError::ConfigDir("no token loaded".into()))?;
        let snapshot = PersistedToken::from_token(&token);
        let path = Self::token_path()?;
        write_persisted_token(&path, &snapshot)
            .map_err(|e| AppError::ConfigDir(format!("write token: {e}")))?;
        Ok(())
    }

    /// Restore from a previously-loaded token. The session re-validates
    /// the token against /v1/sessions; returns the resulting `UserInfo`
    /// on success. Cold-start path goes through
    /// `load_token_with_refresh_blocking` instead, which auto-refreshes
    /// on auth-error; this raw variant is here for callers that want to
    /// surface the auth error directly (e.g. a Phase 8 settings dialog).
    #[allow(dead_code)]
    pub fn load_token_blocking(
        &self,
        token: PersistedToken,
    ) -> Result<UserInfo, RtcError> {
        let info = self.session.load_token(token.to_token()?)?;
        Ok(info)
    }

    /// Cold-start path: stage the saved token in the session, validate
    /// against /v1/sessions, and refresh in-place if the access token
    /// has expired. Mirrors what the Python `_restore_session_from_saved_data`
    /// did via the recovery loop, but as a single atomic call inside
    /// rust_tidal_core.
    pub fn load_token_with_refresh_blocking(
        &self,
        token: PersistedToken,
    ) -> Result<UserInfo, RtcError> {
        self.session.restore_with_refresh(token.to_token()?)
    }

    /// Cheap "is this access token still valid" probe — single GET to
    /// the sessions endpoint. Blocks; spawn off-thread before calling.
    pub fn check_login_blocking(&self) -> bool {
        self.session.check_login()
    }

    /// Kick off a device-code OAuth flow. Returns the `DeviceLogin`
    /// payload (verification URL + user code + polling interval) which
    /// the UI shows to the user.
    pub fn oauth_device_start_blocking(&self) -> Result<DeviceLogin, RtcError> {
        self.session.oauth_device_start()
    }

    /// Poll the device-code endpoint once. Returns `Ok(Some(user))` once
    /// the user authorizes, `Ok(None)` while still pending. Errors are
    /// terminal (token expired, denied, network).
    pub fn oauth_device_poll_blocking(&self) -> Result<Option<UserInfo>, RtcError> {
        self.session.oauth_device_poll()
    }

    // ---- Paginated favorites listings -----------------------------
    //
    // Each helper drains pages until the server reports the full count,
    // matching the Python fetch loop. The page size is intentionally
    // larger than the 50 default — for large collections the round-trip
    // cost dominates so fewer-bigger pages are cheaper.

    pub fn list_favorite_albums_blocking(&self) -> Result<Vec<Album>, RtcError> {
        drain_pages(|args| self.session.list_favorite_albums(args))
    }
    pub fn list_favorite_artists_blocking(&self) -> Result<Vec<Artist>, RtcError> {
        drain_pages(|args| self.session.list_favorite_artists(args))
    }
    pub fn list_favorite_tracks_blocking(&self) -> Result<Vec<Track>, RtcError> {
        drain_pages(|args| self.session.list_favorite_tracks(args))
    }
    pub fn list_favorite_mixes_blocking(&self) -> Result<Vec<Mix>, RtcError> {
        drain_pages(|args| self.session.list_favorite_mixes(args))
    }
    pub fn list_user_playlists_blocking(&self) -> Result<Vec<Playlist>, RtcError> {
        drain_pages(|args| self.session.list_user_playlists("root", args))
    }

    // ---- Detail surfaces (Phase 7) ---------------------------------

    pub fn fetch_album_blocking(&self, id: i64) -> Result<Album, RtcError> {
        self.session.fetch_album(id)
    }

    pub fn list_album_tracks_blocking(&self, id: i64) -> Result<Vec<Track>, RtcError> {
        drain_pages(|args| self.session.album_tracks(id, args))
    }

    pub fn fetch_artist_blocking(&self, id: i64) -> Result<Artist, RtcError> {
        self.session.fetch_artist(id)
    }

    pub fn fetch_playlist_blocking(&self, id: &str) -> Result<Playlist, RtcError> {
        self.session.fetch_playlist(id)
    }

    pub fn list_playlist_tracks_blocking(&self, id: &str) -> Result<Vec<Track>, RtcError> {
        drain_pages(|args| self.session.playlist_tracks(id, args))
    }

    pub fn artist_top_tracks_blocking(&self, id: i64) -> Result<Vec<Track>, RtcError> {
        // Just the first page (50) — these surfaces never want more.
        let args = ListArgs {
            limit: 50,
            offset: 0,
            order: None,
            order_direction: None,
        };
        Ok(self.session.artist_top_tracks(id, &args)?.items)
    }

    pub fn artist_albums_blocking(&self, id: i64) -> Result<Vec<Album>, RtcError> {
        drain_pages(|args| self.session.artist_albums(id, "all", args))
    }

    pub fn artist_bio_blocking(&self, id: i64) -> Result<Bio, RtcError> {
        self.session.artist_bio(id)
    }

    pub fn fetch_mix_blocking(&self, id: &str) -> Result<Mix, RtcError> {
        self.session.fetch_mix(id)
    }

    /// Standalone track-fetch (no stream-info pairing). Phase 7-D will
    /// use it to refresh the track info on transport state changes when
    /// the resolve path was skipped (e.g. mini-player Next button).
    #[allow(dead_code)]
    pub fn fetch_track_blocking(&self, id: i64) -> Result<Track, RtcError> {
        self.session.fetch_track(id)
    }

    /// Resolve a track to a playable stream. Fetches the metadata + the
    /// playback envelope on the same thread (two HTTP calls in sequence
    /// — both are required to render the now-playing surface and queue
    /// the engine, and parallelizing them only saves a few hundred ms).
    /// `audio_quality` follows TIDAL's enum: LOW / HIGH / LOSSLESS /
    /// HI_RES_LOSSLESS.
    pub fn resolve_playback_blocking(
        &self,
        track_id: i64,
        audio_quality: &str,
    ) -> Result<ResolvedPlayback, RtcError> {
        let track = self.session.fetch_track(track_id)?;
        let info = self.session.fetch_stream(track_id, audio_quality, None, None)?;
        let url = info.urls.first().cloned();
        let mpd_manifest = if info.is_mpd {
            Some(info.manifest_data.clone())
        } else {
            None
        };
        Ok(ResolvedPlayback {
            track,
            url,
            mpd_manifest,
            quality: info.audio_quality,
            sample_rate: info.sample_rate,
            bit_depth: info.bit_depth,
            is_bts: info.is_bts,
            is_mpd: info.is_mpd,
        })
    }

    /// Drain a Mix's items into a Track-only Vec. Mixed-content videos
    /// are dropped — Phase 8 will surface them in the UI.
    pub fn list_mix_tracks_blocking(&self, id: &str) -> Result<Vec<Track>, RtcError> {
        let mut out: Vec<Track> = Vec::new();
        let mut offset: i32 = 0;
        let limit: i32 = 200;
        loop {
            let args = ListArgs {
                limit,
                offset,
                order: None,
                order_direction: None,
            };
            let page = self.session.mix_items_list(id, &args)?;
            let n = page.items.len() as i32;
            for it in page.items {
                if let rust_tidal_core::api::PlaylistItem::Track(t) = it {
                    out.push(t);
                }
            }
            if n < limit
                || (page.total_number_of_items > 0
                    && offset + n >= page.total_number_of_items)
            {
                break;
            }
            offset += n;
            if offset > 100_000 {
                break;
            }
        }
        Ok(out)
    }

    // ---- Discovery pages (Phase 6) ---------------------------------
    //
    // `pages/<path>` lookups all share the same parser; the only thing
    // that varies is the path. Home is the odd one out — it lives on
    // /v2/home/feed/static — so it gets its own helper.

    pub fn fetch_home_page_blocking(&self) -> Result<Page, RtcError> {
        self.session.fetch_home_feed()
    }

    pub fn fetch_discovery_page_blocking(&self, path: &str) -> Result<Page, RtcError> {
        self.session.fetch_page(path, None)
    }

    /// Read the user-profile fields from `/v1/users/{id}` and merge with
    /// the `UserInfo` we already have. Mirrors the Python
    /// `_build_user_view` helper introduced in commit a0ef6b71.
    pub fn fetch_profile_blocking(
        &self,
        user_id: i64,
    ) -> Result<serde_json::Value, RtcError> {
        let resp = self.session.request(RequestArgs {
            method: "GET".into(),
            path: format!("users/{user_id}"),
            base_url: None,
            params: None,
            headers: None,
            json_body: None,
            form_body: false,
        })?;
        if resp.ok {
            Ok(resp.body)
        } else {
            Err(RtcError::Other(format!(
                "users/{user_id} returned status {}: {}",
                resp.status, resp.body
            )))
        }
    }
}

/// Drain a paged TIDAL listing endpoint into a single Vec. We ask for
/// 200 items per page; TIDAL clamps to its own internal max (1000 for
/// most endpoints) so even huge collections come down in 5-10 calls.
const PAGE_SIZE: i32 = 200;

fn drain_pages<T, F>(mut fetch: F) -> Result<Vec<T>, RtcError>
where
    F: FnMut(&ListArgs) -> Result<rust_tidal_core::api::PageResponse<T>, RtcError>,
{
    let mut out: Vec<T> = Vec::new();
    let mut offset: i32 = 0;
    loop {
        let args = ListArgs {
            limit: PAGE_SIZE,
            offset,
            order: None,
            order_direction: None,
        };
        let page = fetch(&args)?;
        let n = page.items.len() as i32;
        out.extend(page.items);
        if n < PAGE_SIZE || (page.total_number_of_items > 0 && out.len() as i32 >= page.total_number_of_items) {
            break;
        }
        offset += n;
        // Defensive cap so a bug in `total_number_of_items` can't loop us forever.
        if offset > 100_000 {
            tracing::warn!(offset, "drain_pages safety cap hit");
            break;
        }
    }
    Ok(out)
}

/// Helper: spawn `f` on a fresh thread and call `cb` with its result.
/// The callback is invoked on the worker thread; senders that target
/// the GTK main thread (Relm4 `ComponentSender::input_sender()`) marshal
/// across threads safely.
pub fn spawn_blocking<F, T>(f: F, cb: impl FnOnce(T) + Send + 'static)
where
    F: FnOnce() -> T + Send + 'static,
    T: Send + 'static,
{
    thread::spawn(move || {
        let out = f();
        cb(out);
    });
}
