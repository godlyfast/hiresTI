// rust_tidal_core: native TIDAL client for hiresTI
//
// FFI shape: every entry point takes/returns UTF-8 JSON. Successful results
// come back as the value JSON; failures come back as
// `{"error": "...", "kind": "...", "status": ...}` so the Python side can
// dispatch on the same error categories that core.errors.classify_exception
// already understands.

mod auth;
mod endpoints;
mod error;
mod favorites;
mod http;
mod lists;
mod models;
mod request;
mod session;
mod stream;
mod tail;

// Public Rust API for in-workspace consumers (rust_ui). The C ABI further
// down stays the canonical surface for the legacy ctypes path; Rust callers
// avoid the JSON round-trip by going through these direct types instead.
pub mod api {
    pub use crate::auth::DeviceLogin;
    pub use crate::error::{RtcError, RtcResult};
    pub use crate::favorites::{ListArgs, PageResponse};
    pub use crate::models::{Album, Artist, Folder, Mix, Playlist, Track};
    pub use crate::request::{RequestArgs, ResponseJson};
    pub use crate::session::{
        read_persisted_token, write_persisted_token, PersistedToken, Session, UserInfo,
    };
}

use std::ffi::{CStr, CString};
use std::os::raw::{c_char, c_int};
use std::ptr;

use serde::Serialize;
use serde_json::json;

use crate::error::{ErrorPayload, RtcError, RtcResult};
use crate::session::{
    read_persisted_token, write_persisted_token, PersistedToken, Session, UserInfo,
};

const CRATE_VERSION: &str = env!("CARGO_PKG_VERSION");

#[derive(Serialize)]
struct VersionInfo {
    crate_name: &'static str,
    version: &'static str,
}

// ---------------------------------------------------------------------------
// JSON output helpers
// ---------------------------------------------------------------------------

fn json_to_cstring<T: Serialize>(value: &T) -> *mut c_char {
    match serde_json::to_string(value) {
        Ok(s) => match CString::new(s) {
            Ok(cs) => cs.into_raw(),
            Err(_) => ptr::null_mut(),
        },
        Err(_) => ptr::null_mut(),
    }
}

fn raw_string(s: String) -> *mut c_char {
    match CString::new(s) {
        Ok(cs) => cs.into_raw(),
        Err(_) => ptr::null_mut(),
    }
}

fn ok<T: Serialize>(value: &T) -> *mut c_char {
    json_to_cstring(value)
}

fn err(e: &RtcError) -> *mut c_char {
    let payload = ErrorPayload::from(e);
    json_to_cstring(&payload)
}

fn handle<T: Serialize>(result: RtcResult<T>) -> *mut c_char {
    match result {
        Ok(v) => ok(&v),
        Err(e) => err(&e),
    }
}

unsafe fn cstr_or_invalid<'a>(ptr: *const c_char, name: &str) -> RtcResult<&'a str> {
    if ptr.is_null() {
        return Err(RtcError::InvalidInput(format!("{} is null", name)));
    }
    CStr::from_ptr(ptr)
        .to_str()
        .map_err(|e| RtcError::InvalidInput(format!("{} not utf-8: {}", name, e)))
}

unsafe fn parse_json_input<T: serde::de::DeserializeOwned>(
    ptr: *const c_char,
    name: &str,
) -> RtcResult<T> {
    let s = cstr_or_invalid(ptr, name)?;
    serde_json::from_str(s).map_err(RtcError::Json)
}

// ---------------------------------------------------------------------------
// Lifecycle / version
// ---------------------------------------------------------------------------

#[no_mangle]
pub extern "C" fn rtc_version() -> *mut c_char {
    json_to_cstring(&VersionInfo {
        crate_name: "rust_tidal_core",
        version: CRATE_VERSION,
    })
}

#[no_mangle]
pub extern "C" fn rtc_echo_json(input: *const c_char) -> *mut c_char {
    if input.is_null() {
        return ptr::null_mut();
    }
    let bytes = unsafe { CStr::from_ptr(input) }.to_bytes();
    match CString::new(bytes) {
        Ok(cs) => cs.into_raw(),
        Err(_) => ptr::null_mut(),
    }
}

/// Creates a new session. `pool_size` is the HTTP connection-pool target;
/// pass `0` to use the default (currently 32 idle connections per host).
#[no_mangle]
pub extern "C" fn rtc_session_new(pool_size: c_int) -> *mut Session {
    let pool = if pool_size <= 0 { 32 } else { pool_size as usize };
    Box::into_raw(Box::new(Session::new(pool)))
}

/// # Safety
/// `handle` must have been returned from `rtc_session_new` and not freed.
#[no_mangle]
pub unsafe extern "C" fn rtc_session_free(handle: *mut Session) {
    if handle.is_null() {
        return;
    }
    drop(Box::from_raw(handle));
}

/// # Safety
/// `ptr` must be a JSON pointer returned by this crate.
#[no_mangle]
pub unsafe extern "C" fn rtc_free_string(ptr: *mut c_char) {
    if ptr.is_null() {
        return;
    }
    let _ = CString::from_raw(ptr);
}

// ---------------------------------------------------------------------------
// Token persistence
// ---------------------------------------------------------------------------

/// Reads `~/.config/hiresti/hiresti_token.json` (or any path) and returns
/// the parsed PersistedToken JSON. Returns an error payload if the file is
/// missing or malformed.
#[no_mangle]
pub unsafe extern "C" fn rtc_token_read_file(path: *const c_char) -> *mut c_char {
    let result = (|| -> RtcResult<PersistedToken> {
        let path = cstr_or_invalid(path, "path")?;
        read_persisted_token(path)
    })();
    handle(result)
}

#[no_mangle]
pub unsafe extern "C" fn rtc_token_write_file(
    path: *const c_char,
    persisted_json: *const c_char,
) -> *mut c_char {
    let result = (|| -> RtcResult<serde_json::Value> {
        let path = cstr_or_invalid(path, "path")?;
        let persisted: PersistedToken = parse_json_input(persisted_json, "persisted_json")?;
        write_persisted_token(path, &persisted)?;
        Ok(json!({"ok": true}))
    })();
    handle(result)
}

// ---------------------------------------------------------------------------
// Session ops
// ---------------------------------------------------------------------------

/// Loads a saved token (PersistedToken JSON) into the session. Calls
/// `/v1/sessions` to populate user_id / session_id / country_code. Returns
/// the resulting UserInfo on success.
#[no_mangle]
pub unsafe extern "C" fn rtc_session_load_token(
    handle_ptr: *mut Session,
    persisted_json: *const c_char,
) -> *mut c_char {
    let result = (|| -> RtcResult<UserInfo> {
        let session = session_ref(handle_ptr)?;
        let persisted: PersistedToken = parse_json_input(persisted_json, "persisted_json")?;
        let token = persisted.to_token()?;
        session.load_token(token)
    })();
    handle(result)
}

#[no_mangle]
pub unsafe extern "C" fn rtc_session_token_snapshot(handle_ptr: *mut Session) -> *mut c_char {
    let result = (|| -> RtcResult<serde_json::Value> {
        let session = session_ref(handle_ptr)?;
        let snap = session.token_snapshot();
        Ok(serde_json::to_value(snap)?)
    })();
    handle(result)
}

#[no_mangle]
pub unsafe extern "C" fn rtc_session_user_snapshot(handle_ptr: *mut Session) -> *mut c_char {
    let result = (|| -> RtcResult<serde_json::Value> {
        let session = session_ref(handle_ptr)?;
        Ok(serde_json::to_value(session.user_snapshot())?)
    })();
    handle(result)
}

#[no_mangle]
pub unsafe extern "C" fn rtc_session_persisted_snapshot(handle_ptr: *mut Session) -> *mut c_char {
    let result = (|| -> RtcResult<serde_json::Value> {
        let session = session_ref(handle_ptr)?;
        let snap = session
            .token_snapshot()
            .map(|t| PersistedToken::from_token(&t));
        Ok(serde_json::to_value(snap)?)
    })();
    handle(result)
}

#[no_mangle]
pub unsafe extern "C" fn rtc_session_check_login(handle_ptr: *mut Session) -> *mut c_char {
    let result = (|| -> RtcResult<serde_json::Value> {
        let session = session_ref(handle_ptr)?;
        Ok(json!({"ok": session.check_login()}))
    })();
    handle(result)
}

#[no_mangle]
pub unsafe extern "C" fn rtc_session_pkce_login_url(handle_ptr: *mut Session) -> *mut c_char {
    let result = (|| -> RtcResult<serde_json::Value> {
        let session = session_ref(handle_ptr)?;
        Ok(json!({ "url": session.pkce_login_url()? }))
    })();
    handle(result)
}

#[no_mangle]
pub unsafe extern "C" fn rtc_session_pkce_finish(
    handle_ptr: *mut Session,
    redirect_url: *const c_char,
) -> *mut c_char {
    let result = (|| -> RtcResult<UserInfo> {
        let session = session_ref(handle_ptr)?;
        let url = cstr_or_invalid(redirect_url, "redirect_url")?;
        session.pkce_finish(url)
    })();
    handle(result)
}

#[no_mangle]
pub unsafe extern "C" fn rtc_session_oauth_device_start(
    handle_ptr: *mut Session,
) -> *mut c_char {
    let result = (|| -> RtcResult<serde_json::Value> {
        let session = session_ref(handle_ptr)?;
        let login = session.oauth_device_start()?;
        Ok(serde_json::to_value(login)?)
    })();
    handle(result)
}

#[no_mangle]
pub unsafe extern "C" fn rtc_session_oauth_device_poll(handle_ptr: *mut Session) -> *mut c_char {
    let result = (|| -> RtcResult<serde_json::Value> {
        let session = session_ref(handle_ptr)?;
        match session.oauth_device_poll()? {
            Some(user) => Ok(json!({"status": "ok", "user": user})),
            None => Ok(json!({"status": "pending"})),
        }
    })();
    handle(result)
}

// ---------------------------------------------------------------------------
// Model fetchers (Phase 3)
// ---------------------------------------------------------------------------

#[no_mangle]
pub unsafe extern "C" fn rtc_session_fetch_track(
    handle_ptr: *mut Session,
    track_id: i64,
) -> *mut c_char {
    let result = (|| -> RtcResult<serde_json::Value> {
        let session = session_ref(handle_ptr)?;
        Ok(serde_json::to_value(session.fetch_track(track_id)?)?)
    })();
    handle(result)
}

#[no_mangle]
pub unsafe extern "C" fn rtc_session_fetch_album(
    handle_ptr: *mut Session,
    album_id: i64,
) -> *mut c_char {
    let result = (|| -> RtcResult<serde_json::Value> {
        let session = session_ref(handle_ptr)?;
        Ok(serde_json::to_value(session.fetch_album(album_id)?)?)
    })();
    handle(result)
}

#[no_mangle]
pub unsafe extern "C" fn rtc_session_fetch_artist(
    handle_ptr: *mut Session,
    artist_id: i64,
) -> *mut c_char {
    let result = (|| -> RtcResult<serde_json::Value> {
        let session = session_ref(handle_ptr)?;
        Ok(serde_json::to_value(session.fetch_artist(artist_id)?)?)
    })();
    handle(result)
}

#[no_mangle]
pub unsafe extern "C" fn rtc_session_fetch_playlist(
    handle_ptr: *mut Session,
    playlist_id: *const c_char,
) -> *mut c_char {
    let result = (|| -> RtcResult<serde_json::Value> {
        let session = session_ref(handle_ptr)?;
        let id = cstr_or_invalid(playlist_id, "playlist_id")?;
        Ok(serde_json::to_value(session.fetch_playlist(id)?)?)
    })();
    handle(result)
}

#[no_mangle]
pub unsafe extern "C" fn rtc_session_fetch_mix(
    handle_ptr: *mut Session,
    mix_id: *const c_char,
) -> *mut c_char {
    let result = (|| -> RtcResult<serde_json::Value> {
        let session = session_ref(handle_ptr)?;
        let id = cstr_or_invalid(mix_id, "mix_id")?;
        Ok(serde_json::to_value(session.fetch_mix(id)?)?)
    })();
    handle(result)
}

#[no_mangle]
pub unsafe extern "C" fn rtc_session_fetch_folder(
    handle_ptr: *mut Session,
    folder_id: *const c_char,
) -> *mut c_char {
    let result = (|| -> RtcResult<serde_json::Value> {
        let session = session_ref(handle_ptr)?;
        let id = cstr_or_invalid(folder_id, "folder_id")?;
        Ok(serde_json::to_value(session.fetch_folder(id)?)?)
    })();
    handle(result)
}

#[no_mangle]
pub unsafe extern "C" fn rtc_session_search(
    handle_ptr: *mut Session,
    query: *const c_char,
    limit: c_int,
) -> *mut c_char {
    let result = (|| -> RtcResult<serde_json::Value> {
        let session = session_ref(handle_ptr)?;
        let q = cstr_or_invalid(query, "query")?;
        Ok(serde_json::to_value(session.search(q, limit as i32)?)?)
    })();
    handle(result)
}

/// `args_json`: `{"path": "pages/genre_page", "params": {"deviceType": "BROWSER"}}`
#[no_mangle]
pub unsafe extern "C" fn rtc_session_page_get_raw(
    handle_ptr: *mut Session,
    args_json: *const c_char,
) -> *mut c_char {
    #[derive(serde::Deserialize)]
    struct PageArgs {
        path: String,
        #[serde(default)]
        params: Option<std::collections::BTreeMap<String, crate::request::ParamValue>>,
    }
    let result = (|| -> RtcResult<serde_json::Value> {
        let session = session_ref(handle_ptr)?;
        let args: PageArgs = parse_json_input(args_json, "args_json")?;
        session.page_get_raw(&args.path, args.params)
    })();
    handle(result)
}

/// Parse a model dict using one of the named parsers. `args_json`:
/// `{"kind": "track" | "album" | ..., "value": {...}}`. Returns the parsed
/// model JSON.
#[no_mangle]
pub unsafe extern "C" fn rtc_parse_model(args_json: *const c_char) -> *mut c_char {
    #[derive(serde::Deserialize)]
    struct ParseArgs {
        kind: String,
        value: serde_json::Value,
    }
    let result = (|| -> RtcResult<serde_json::Value> {
        let args: ParseArgs = parse_json_input(args_json, "args_json")?;
        crate::endpoints::parse_typed(&args.kind, &args.value)
    })();
    handle(result)
}

// ---------------------------------------------------------------------------
// Favorites + list endpoints (Phase 4)
// ---------------------------------------------------------------------------

#[no_mangle]
pub unsafe extern "C" fn rtc_session_favorites_add(
    handle_ptr: *mut Session,
    kind: *const c_char,
    id: *const c_char,
) -> *mut c_char {
    let result = (|| -> RtcResult<serde_json::Value> {
        let session = session_ref(handle_ptr)?;
        let kind = crate::favorites::FavoriteKind::from_str(cstr_or_invalid(kind, "kind")?)?;
        let id = cstr_or_invalid(id, "id")?;
        Ok(serde_json::json!({"ok": session.favorites_add(kind, id)?}))
    })();
    handle(result)
}

#[no_mangle]
pub unsafe extern "C" fn rtc_session_favorites_remove(
    handle_ptr: *mut Session,
    kind: *const c_char,
    id: *const c_char,
) -> *mut c_char {
    let result = (|| -> RtcResult<serde_json::Value> {
        let session = session_ref(handle_ptr)?;
        let kind = crate::favorites::FavoriteKind::from_str(cstr_or_invalid(kind, "kind")?)?;
        let id = cstr_or_invalid(id, "id")?;
        Ok(serde_json::json!({"ok": session.favorites_remove(kind, id)?}))
    })();
    handle(result)
}

#[no_mangle]
pub unsafe extern "C" fn rtc_session_favorites_mix_toggle(
    handle_ptr: *mut Session,
    mix_id: *const c_char,
    add: c_int,
) -> *mut c_char {
    let result = (|| -> RtcResult<serde_json::Value> {
        let session = session_ref(handle_ptr)?;
        let id = cstr_or_invalid(mix_id, "mix_id")?;
        Ok(serde_json::json!({
            "ok": session.favorites_mix_toggle(id, add != 0)?
        }))
    })();
    handle(result)
}

/// Playlist / folder mutation dispatcher. `args_json`:
///   {"op": "create_playlist", "title": "...", "description": "...",
///    "parent_folder_id": "root"}
///   {"op": "create_folder", "title": "...", "parent_folder_id": "root"}
///   {"op": "remove", "kind": "playlist|folder", "ids": ["uuid", ...]}
/// Returns the parsed model dict for create ops, `{"ok": true|false}` for remove.
#[no_mangle]
pub unsafe extern "C" fn rtc_session_collection_mutate(
    handle_ptr: *mut Session,
    args_json: *const c_char,
) -> *mut c_char {
    #[derive(serde::Deserialize)]
    #[serde(tag = "op", rename_all = "snake_case")]
    enum Op {
        CreatePlaylist {
            title: String,
            #[serde(default)]
            description: String,
            #[serde(default = "default_root")]
            parent_folder_id: String,
        },
        CreateFolder {
            title: String,
            #[serde(default = "default_root")]
            parent_folder_id: String,
        },
        Remove {
            kind: String,
            ids: Vec<String>,
        },
    }
    fn default_root() -> String {
        "root".into()
    }
    let result = (|| -> RtcResult<serde_json::Value> {
        let session = session_ref(handle_ptr)?;
        let args: Op = parse_json_input(args_json, "args_json")?;
        match args {
            Op::CreatePlaylist {
                title,
                description,
                parent_folder_id,
            } => {
                let pl = session.create_playlist(&title, &description, &parent_folder_id)?;
                Ok(serde_json::to_value(pl)?)
            }
            Op::CreateFolder {
                title,
                parent_folder_id,
            } => {
                let f = session.create_folder(&title, &parent_folder_id)?;
                Ok(serde_json::to_value(f)?)
            }
            Op::Remove { kind, ids } => {
                Ok(serde_json::json!({
                    "ok": session.remove_folders_playlists(&kind, &ids)?
                }))
            }
        }
    })();
    handle(result)
}

/// Listing endpoint dispatcher. `args_json` schema:
/// `{"kind": "albums|artists|tracks|mixes|playlists|playlist_folders|
///           album_tracks|playlist_items|playlist_tracks|mix_items",
///   "limit": 50, "offset": 0, "order": "DATE", "order_direction": "DESC",
///   "id": "..." | <int>, "folder_id": "root"}`
/// Returns a PageResponse: `{items, total_number_of_items, limit, offset}`.
#[no_mangle]
pub unsafe extern "C" fn rtc_session_list(
    handle_ptr: *mut Session,
    args_json: *const c_char,
) -> *mut c_char {
    #[derive(serde::Deserialize)]
    struct Args {
        kind: String,
        #[serde(default = "default_limit")]
        limit: i32,
        #[serde(default)]
        offset: i32,
        #[serde(default)]
        order: Option<String>,
        #[serde(default)]
        order_direction: Option<String>,
        #[serde(default)]
        id: Option<serde_json::Value>,
        #[serde(default)]
        folder_id: Option<String>,
    }
    fn default_limit() -> i32 {
        50
    }
    let result = (|| -> RtcResult<serde_json::Value> {
        let session = session_ref(handle_ptr)?;
        let args: Args = parse_json_input(args_json, "args_json")?;
        let list_args = crate::favorites::ListArgs {
            limit: args.limit,
            offset: args.offset,
            order: args.order.clone(),
            order_direction: args.order_direction.clone(),
        };
        let id_str = args.id.as_ref().and_then(|v| v.as_str()).map(str::to_string);
        let id_int = args.id.as_ref().and_then(|v| v.as_i64());
        let kind = args.kind.to_ascii_lowercase();
        let folder_id = args.folder_id.unwrap_or_else(|| "root".to_string());
        let value: serde_json::Value = match kind.as_str() {
            "favorite_albums" | "albums" => {
                serde_json::to_value(session.list_favorite_albums(&list_args)?)?
            }
            "favorite_artists" | "artists" => {
                serde_json::to_value(session.list_favorite_artists(&list_args)?)?
            }
            "favorite_tracks" | "tracks" => {
                serde_json::to_value(session.list_favorite_tracks(&list_args)?)?
            }
            "favorite_mixes" | "mixes" => {
                serde_json::to_value(session.list_favorite_mixes(&list_args)?)?
            }
            "user_playlists" | "playlists" => {
                serde_json::to_value(session.list_user_playlists(&folder_id, &list_args)?)?
            }
            "playlist_folders" => {
                serde_json::to_value(session.list_playlist_folders(&folder_id, &list_args)?)?
            }
            "album_tracks" => {
                let id = id_int
                    .or_else(|| id_str.as_ref().and_then(|s| s.parse().ok()))
                    .ok_or_else(|| RtcError::InvalidInput("album_tracks needs numeric id".into()))?;
                serde_json::to_value(session.album_tracks(id, &list_args)?)?
            }
            "playlist_items" => {
                let id = id_str
                    .clone()
                    .ok_or_else(|| RtcError::InvalidInput("playlist_items needs id".into()))?;
                serde_json::to_value(session.playlist_items(&id, &list_args)?)?
            }
            "playlist_tracks" => {
                let id = id_str
                    .clone()
                    .ok_or_else(|| RtcError::InvalidInput("playlist_tracks needs id".into()))?;
                serde_json::to_value(session.playlist_tracks(&id, &list_args)?)?
            }
            "mix_items" => {
                let id = id_str
                    .clone()
                    .ok_or_else(|| RtcError::InvalidInput("mix_items needs id".into()))?;
                serde_json::to_value(session.mix_items_list(&id, &list_args)?)?
            }
            "track_radio" => {
                let id = id_int
                    .or_else(|| id_str.as_ref().and_then(|s| s.parse().ok()))
                    .ok_or_else(|| RtcError::InvalidInput("track_radio needs numeric id".into()))?;
                serde_json::to_value(session.track_radio(id, &list_args)?)?
            }
            "artist_top_tracks" => {
                let id = id_int
                    .or_else(|| id_str.as_ref().and_then(|s| s.parse().ok()))
                    .ok_or_else(|| RtcError::InvalidInput("artist_top_tracks needs numeric id".into()))?;
                serde_json::to_value(session.artist_top_tracks(id, &list_args)?)?
            }
            "artist_similar" => {
                let id = id_int
                    .or_else(|| id_str.as_ref().and_then(|s| s.parse().ok()))
                    .ok_or_else(|| RtcError::InvalidInput("artist_similar needs numeric id".into()))?;
                serde_json::to_value(session.artist_similar(id, &list_args)?)?
            }
            "artist_albums" | "artist_ep_singles" | "artist_compilations" => {
                let id = id_int
                    .or_else(|| id_str.as_ref().and_then(|s| s.parse().ok()))
                    .ok_or_else(|| RtcError::InvalidInput("artist_albums needs numeric id".into()))?;
                let kind_str = match kind.as_str() {
                    "artist_ep_singles" => "ep_singles",
                    "artist_compilations" => "compilations",
                    _ => "all",
                };
                serde_json::to_value(session.artist_albums(id, kind_str, &list_args)?)?
            }
            other => return Err(RtcError::InvalidInput(format!("unknown list kind: {}", other))),
        };
        Ok(value)
    })();
    handle(result)
}

#[no_mangle]
pub unsafe extern "C" fn rtc_session_count(
    handle_ptr: *mut Session,
    kind: *const c_char,
) -> *mut c_char {
    let result = (|| -> RtcResult<serde_json::Value> {
        let session = session_ref(handle_ptr)?;
        let kind = cstr_or_invalid(kind, "kind")?.to_ascii_lowercase();
        let count = match kind.as_str() {
            "favorite_albums" | "albums" => session.count_favorite_albums()?,
            "favorite_artists" | "artists" => session.count_favorite_artists()?,
            "favorite_tracks" | "tracks" => session.count_favorite_tracks()?,
            other => return Err(RtcError::InvalidInput(format!("unknown count kind: {}", other))),
        };
        Ok(serde_json::json!({"count": count}))
    })();
    handle(result)
}

/// Generic authenticated HTTP. Input JSON shape:
/// `{ "method": "GET", "path": "search", "base_url": null, "params": {...},
///    "headers": {...}, "json_body": null, "form_body": false }`
/// Output: `{ "ok": bool, "status": int, "body": <parsed JSON or string> }`
/// or an error payload.
#[no_mangle]
pub unsafe extern "C" fn rtc_session_request(
    handle_ptr: *mut Session,
    args_json: *const c_char,
) -> *mut c_char {
    let result = (|| -> RtcResult<serde_json::Value> {
        let session = session_ref(handle_ptr)?;
        let args: crate::request::RequestArgs = parse_json_input(args_json, "args_json")?;
        let resp = session.request(args)?;
        Ok(serde_json::to_value(resp)?)
    })();
    handle(result)
}

// ---------------------------------------------------------------------------
// Tail surfaces (Phase 6) — lyrics, bio
// ---------------------------------------------------------------------------

#[no_mangle]
pub unsafe extern "C" fn rtc_session_track_lyrics(
    handle_ptr: *mut Session,
    track_id: i64,
) -> *mut c_char {
    let result = (|| -> RtcResult<serde_json::Value> {
        let session = session_ref(handle_ptr)?;
        Ok(serde_json::to_value(session.track_lyrics(track_id)?)?)
    })();
    handle(result)
}

#[no_mangle]
pub unsafe extern "C" fn rtc_session_artist_bio(
    handle_ptr: *mut Session,
    artist_id: i64,
) -> *mut c_char {
    let result = (|| -> RtcResult<serde_json::Value> {
        let session = session_ref(handle_ptr)?;
        Ok(serde_json::to_value(session.artist_bio(artist_id)?)?)
    })();
    handle(result)
}

// ---------------------------------------------------------------------------
// Stream + manifest (Phase 5)
// ---------------------------------------------------------------------------

/// Fetch + decode the modern playbackinfopostpaywall envelope.
/// `args_json`: `{"track_id": 12345, "audio_quality": "HI_RES_LOSSLESS",
///                "playback_mode": "STREAM", "asset_presentation": "FULL"}`.
/// Returns a fully-decoded StreamInfo (manifest already base64-decoded;
/// BTS payload's inner JSON expanded into urls/codecs/etc).
#[no_mangle]
pub unsafe extern "C" fn rtc_session_fetch_stream(
    handle_ptr: *mut Session,
    args_json: *const c_char,
) -> *mut c_char {
    #[derive(serde::Deserialize)]
    struct StreamArgs {
        track_id: i64,
        audio_quality: String,
        #[serde(default)]
        playback_mode: Option<String>,
        #[serde(default)]
        asset_presentation: Option<String>,
    }
    let result = (|| -> RtcResult<serde_json::Value> {
        let session = session_ref(handle_ptr)?;
        let args: StreamArgs = parse_json_input(args_json, "args_json")?;
        let info = session.fetch_stream(
            args.track_id,
            &args.audio_quality,
            args.playback_mode.as_deref(),
            args.asset_presentation.as_deref(),
        )?;
        Ok(serde_json::to_value(info)?)
    })();
    handle(result)
}

/// Legacy `urlpostpaywall` fallback. `args_json`:
/// `{"track_id": 12345, "audio_quality": "LOSSLESS"}`. Returns
/// `{"url": "https://..."}`.
#[no_mangle]
pub unsafe extern "C" fn rtc_session_fetch_legacy_url(
    handle_ptr: *mut Session,
    args_json: *const c_char,
) -> *mut c_char {
    #[derive(serde::Deserialize)]
    struct LegacyArgs {
        track_id: i64,
        audio_quality: String,
    }
    let result = (|| -> RtcResult<serde_json::Value> {
        let session = session_ref(handle_ptr)?;
        let args: LegacyArgs = parse_json_input(args_json, "args_json")?;
        let url = session.fetch_legacy_url(args.track_id, &args.audio_quality)?;
        Ok(json!({ "url": url }))
    })();
    handle(result)
}

#[no_mangle]
pub unsafe extern "C" fn rtc_session_refresh_token(handle_ptr: *mut Session) -> *mut c_char {
    let result = (|| -> RtcResult<serde_json::Value> {
        let session = session_ref(handle_ptr)?;
        let token = session.refresh_token()?;
        let persisted = PersistedToken::from_token(&token);
        Ok(serde_json::to_value(persisted)?)
    })();
    handle(result)
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn session_ref<'a>(ptr: *mut Session) -> RtcResult<&'a Session> {
    if ptr.is_null() {
        return Err(RtcError::InvalidInput("session handle is null".into()));
    }
    unsafe { Ok(&*ptr) }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn version_returns_valid_json() {
        let raw = rtc_version();
        assert!(!raw.is_null());
        let s = unsafe { CStr::from_ptr(raw) }.to_str().unwrap().to_owned();
        unsafe { rtc_free_string(raw) };
        let v: serde_json::Value = serde_json::from_str(&s).unwrap();
        assert_eq!(v["crate_name"], "rust_tidal_core");
        assert!(v["version"].is_string());
    }

    #[test]
    fn echo_roundtrips_utf8() {
        let input = CString::new(r#"{"hello":"世界"}"#).unwrap();
        let raw = rtc_echo_json(input.as_ptr());
        assert!(!raw.is_null());
        let s = unsafe { CStr::from_ptr(raw) }.to_str().unwrap().to_owned();
        unsafe { rtc_free_string(raw) };
        let v: serde_json::Value = serde_json::from_str(&s).unwrap();
        assert_eq!(v["hello"], "世界");
    }

    #[test]
    fn session_lifecycle_no_token_check_login_false() {
        let s = rtc_session_new(0);
        assert!(!s.is_null());
        let raw = unsafe { rtc_session_check_login(s) };
        let text = unsafe { CStr::from_ptr(raw) }.to_str().unwrap().to_owned();
        unsafe { rtc_free_string(raw) };
        unsafe { rtc_session_free(s) };
        let v: serde_json::Value = serde_json::from_str(&text).unwrap();
        assert_eq!(v["ok"], false);
    }
}

// Suppress unused — `raw_string` is reserved for future entry points that
// return a plain string (e.g. login URL) bypassing JSON serialization. Kept
// in the codebase so future phases don't redefine the helper.
#[allow(dead_code)]
fn _keep_helpers_alive() {
    let _ = raw_string("".into());
}
