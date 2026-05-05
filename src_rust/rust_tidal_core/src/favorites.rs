//! User favorites + library listing endpoints.
//!
//! TIDAL splits favorites across two API versions:
//!   v1: /users/<uid>/favorites/{albums,artists,tracks}
//!   v2: /favorites/mixes/{add,remove}, /my-collection/playlists/folders
//!
//! All requests authenticate with the active session; pagination follows
//! TIDAL's conventional `limit`/`offset` plus optional `order`/`orderDirection`.

use std::collections::BTreeMap;

use serde::Serialize;
use serde_json::Value;

use crate::endpoints::ok_or_status;
use crate::error::{RtcError, RtcResult};
use crate::models::{
    parse_album, parse_artist, parse_mix, parse_playlist, parse_track, Album, Artist, Folder,
    Mix, Playlist, Track,
};
use crate::request::{ParamValue, RequestArgs};
use crate::session::Session;

const V2_BASE: &str = "https://api.tidal.com/v2/";

/// Reference values for callers that want to construct `order` strings
/// without hard-coding TIDAL's enum tokens. Python passes `order` as a
/// pre-computed string today, so these helpers are unused but documented.
#[allow(dead_code)]
#[derive(Debug, Clone, Copy)]
pub enum Order {
    Name,
    DateAdded,
    Date,
    DateCreated,
    Artist,
    Album,
    ReleaseDate,
}

#[allow(dead_code)]
impl Order {
    pub fn as_str(self) -> &'static str {
        match self {
            Order::Name => "NAME",
            Order::DateAdded => "DATE",
            Order::Date => "DATE",
            Order::DateCreated => "DATE_CREATED",
            Order::Artist => "ARTIST",
            Order::Album => "ALBUM",
            Order::ReleaseDate => "RELEASE_DATE",
        }
    }
}

#[allow(dead_code)]
#[derive(Debug, Clone, Copy)]
pub enum OrderDirection {
    Ascending,
    Descending,
}

#[allow(dead_code)]
impl OrderDirection {
    pub fn as_str(self) -> &'static str {
        match self {
            OrderDirection::Ascending => "ASC",
            OrderDirection::Descending => "DESC",
        }
    }
}

#[derive(Debug, Clone, Default, serde::Deserialize)]
pub struct ListArgs {
    #[serde(default = "default_limit")]
    pub limit: i32,
    #[serde(default)]
    pub offset: i32,
    #[serde(default)]
    pub order: Option<String>,
    #[serde(default)]
    pub order_direction: Option<String>,
}

fn default_limit() -> i32 {
    50
}

#[derive(Debug, Clone, Serialize)]
pub struct PageResponse<T> {
    pub items: Vec<T>,
    pub total_number_of_items: i32,
    pub limit: i32,
    pub offset: i32,
}

fn build_list_params(args: &ListArgs) -> BTreeMap<String, ParamValue> {
    let mut params = BTreeMap::new();
    params.insert(
        "limit".to_string(),
        ParamValue::Int(args.limit.clamp(1, 1000) as i64),
    );
    params.insert("offset".to_string(), ParamValue::Int(args.offset.max(0) as i64));
    if let Some(order) = args.order.as_ref().filter(|s| !s.is_empty()) {
        params.insert("order".to_string(), ParamValue::Str(order.clone()));
    }
    if let Some(dir) = args.order_direction.as_ref().filter(|s| !s.is_empty()) {
        params.insert("orderDirection".to_string(), ParamValue::Str(dir.clone()));
    }
    params
}

fn fetch_paginated<T, F>(
    session: &Session,
    path: &str,
    base_url: Option<&str>,
    args: &ListArgs,
    extra_params: Option<BTreeMap<String, ParamValue>>,
    parse_one: F,
) -> RtcResult<PageResponse<T>>
where
    F: Fn(&Value) -> T,
{
    let mut params = build_list_params(args);
    if let Some(extra) = extra_params {
        for (k, v) in extra {
            params.insert(k, v);
        }
    }
    let mut req = RequestArgs {
        method: "GET".into(),
        path: path.into(),
        base_url: base_url.map(str::to_string),
        params: Some(params),
        headers: None,
        json_body: None,
        form_body: false,
    };
    if base_url.is_some() {
        req.base_url = base_url.map(str::to_string);
    }
    let resp = session.request(req)?;
    let body = ok_or_status(resp)?;
    let total = body
        .get("totalNumberOfItems")
        .and_then(|v| v.as_i64())
        .unwrap_or(-1);
    let items = body
        .get("items")
        .and_then(|v| v.as_array())
        .map(|arr| {
            arr.iter()
                .map(|raw| parse_one(extract_item(raw)))
                .collect()
        })
        .unwrap_or_default();
    let limit = args.limit;
    let offset = args.offset;
    Ok(PageResponse {
        items,
        total_number_of_items: i32::try_from(total).unwrap_or(-1),
        limit,
        offset,
    })
}

/// favorites endpoints wrap each item in `{"item": {...}, "type": "..."}`.
/// Album/track lists do, artists may not. Unwrap if present.
fn extract_item(value: &Value) -> &Value {
    value.get("item").unwrap_or(value)
}

// ---------------------------------------------------------------------------
// Add / remove
// ---------------------------------------------------------------------------

fn user_id_required(session: &Session) -> RtcResult<i64> {
    session
        .user_snapshot()
        .map(|u| u.user_id)
        .ok_or_else(|| RtcError::Auth("no user loaded — call load_token first".into()))
}

pub fn favorites_add(session: &Session, kind: FavoriteKind, id: &str) -> RtcResult<bool> {
    let uid = user_id_required(session)?;
    let (path, key) = kind.add_descriptor();
    let mut params = BTreeMap::new();
    params.insert(key.to_string(), ParamValue::Str(id.to_string()));
    let req = RequestArgs {
        method: "POST".into(),
        path: format!("users/{}/favorites/{}", uid, path),
        base_url: None,
        params: Some(params),
        headers: None,
        json_body: None,
        form_body: true,
    };
    let resp = session.request(req)?;
    Ok(resp.ok)
}

pub fn favorites_remove(session: &Session, kind: FavoriteKind, id: &str) -> RtcResult<bool> {
    let uid = user_id_required(session)?;
    let path = kind.remove_path();
    let req = RequestArgs {
        method: "DELETE".into(),
        path: format!("users/{}/favorites/{}/{}", uid, path, id),
        base_url: None,
        params: None,
        headers: None,
        json_body: None,
        form_body: false,
    };
    let resp = session.request(req)?;
    Ok(resp.ok)
}

pub fn favorites_mix_toggle(session: &Session, mix_id: &str, add: bool) -> RtcResult<bool> {
    let endpoint = if add {
        "favorites/mixes/add"
    } else {
        "favorites/mixes/remove"
    };
    let mut params = BTreeMap::new();
    params.insert("mixIds".into(), ParamValue::Str(mix_id.to_string()));
    params.insert("onArtifactNotFound".into(), ParamValue::Str("FAIL".into()));
    let req = RequestArgs {
        method: "PUT".into(),
        path: endpoint.into(),
        base_url: Some(V2_BASE.into()),
        params: Some(params),
        headers: None,
        json_body: None,
        form_body: false,
    };
    let resp = session.request(req)?;
    Ok(resp.ok)
}

#[derive(Debug, Clone, Copy)]
pub enum FavoriteKind {
    Album,
    Artist,
    Track,
    Video,
}

impl FavoriteKind {
    fn add_descriptor(self) -> (&'static str, &'static str) {
        match self {
            FavoriteKind::Album => ("albums", "albumId"),
            FavoriteKind::Artist => ("artists", "artistId"),
            FavoriteKind::Track => ("tracks", "trackId"),
            FavoriteKind::Video => ("videos", "videoIds"),
        }
    }

    fn remove_path(self) -> &'static str {
        match self {
            FavoriteKind::Album => "albums",
            FavoriteKind::Artist => "artists",
            FavoriteKind::Track => "tracks",
            FavoriteKind::Video => "videos",
        }
    }

    pub fn from_str(s: &str) -> RtcResult<Self> {
        match s.to_ascii_lowercase().as_str() {
            "album" | "albums" => Ok(FavoriteKind::Album),
            "artist" | "artists" => Ok(FavoriteKind::Artist),
            "track" | "tracks" => Ok(FavoriteKind::Track),
            "video" | "videos" => Ok(FavoriteKind::Video),
            other => Err(RtcError::InvalidInput(format!(
                "unknown favorite kind: {}",
                other
            ))),
        }
    }
}

// ---------------------------------------------------------------------------
// Listings
// ---------------------------------------------------------------------------

pub fn list_favorite_albums(session: &Session, args: &ListArgs) -> RtcResult<PageResponse<Album>> {
    let uid = user_id_required(session)?;
    fetch_paginated(
        session,
        &format!("users/{}/favorites/albums", uid),
        None,
        args,
        None,
        parse_album,
    )
}

pub fn list_favorite_artists(
    session: &Session,
    args: &ListArgs,
) -> RtcResult<PageResponse<Artist>> {
    let uid = user_id_required(session)?;
    fetch_paginated(
        session,
        &format!("users/{}/favorites/artists", uid),
        None,
        args,
        None,
        parse_artist,
    )
}

pub fn list_favorite_tracks(session: &Session, args: &ListArgs) -> RtcResult<PageResponse<Track>> {
    let uid = user_id_required(session)?;
    fetch_paginated(
        session,
        &format!("users/{}/favorites/tracks", uid),
        None,
        args,
        None,
        parse_track,
    )
}

pub fn list_favorite_mixes(session: &Session, args: &ListArgs) -> RtcResult<PageResponse<Mix>> {
    // v2 surface: https://api.tidal.com/v2/favorites/mixes (no userId path).
    // The v1 /users/{uid}/favorites/mixes path 404s.
    fetch_paginated(
        session,
        "favorites/mixes",
        Some(V2_BASE),
        args,
        None,
        parse_mix,
    )
}

pub fn list_user_playlists(
    session: &Session,
    folder_id: &str,
    args: &ListArgs,
) -> RtcResult<PageResponse<Playlist>> {
    let mut extra = BTreeMap::new();
    extra.insert("folderId".into(), ParamValue::Str(folder_id.to_string()));
    extra.insert("includeOnly".into(), ParamValue::Str("PLAYLIST".into()));
    fetch_paginated(
        session,
        "my-collection/playlists/folders",
        Some(V2_BASE),
        args,
        Some(extra),
        |raw| {
            // v2 returns {"itemType":"PLAYLIST", "data": {playlist...}}
            let inner = raw.get("data").unwrap_or(raw);
            parse_playlist(inner)
        },
    )
}

pub fn list_playlist_folders(
    session: &Session,
    folder_id: &str,
    args: &ListArgs,
) -> RtcResult<PageResponse<Folder>> {
    let mut extra = BTreeMap::new();
    extra.insert("folderId".into(), ParamValue::Str(folder_id.to_string()));
    extra.insert("includeOnly".into(), ParamValue::Str("FOLDER".into()));
    fetch_paginated(
        session,
        "my-collection/playlists/folders",
        Some(V2_BASE),
        args,
        Some(extra),
        |raw| {
            let inner = raw.get("data").unwrap_or(raw);
            crate::models::parse_folder(inner)
        },
    )
}

/// Total count helpers — TIDAL puts the total in `totalNumberOfItems` on
/// every paginated response, so we just hit limit=1 and read it back.
pub fn count_favorite_albums(session: &Session) -> RtcResult<i32> {
    list_favorite_albums(
        session,
        &ListArgs {
            limit: 1,
            offset: 0,
            order: None,
            order_direction: None,
        },
    )
    .map(|p| p.total_number_of_items.max(0))
}

pub fn count_favorite_artists(session: &Session) -> RtcResult<i32> {
    list_favorite_artists(
        session,
        &ListArgs {
            limit: 1,
            offset: 0,
            order: None,
            order_direction: None,
        },
    )
    .map(|p| p.total_number_of_items.max(0))
}

pub fn count_favorite_tracks(session: &Session) -> RtcResult<i32> {
    list_favorite_tracks(
        session,
        &ListArgs {
            limit: 1,
            offset: 0,
            order: None,
            order_direction: None,
        },
    )
    .map(|p| p.total_number_of_items.max(0))
}

// ---------------------------------------------------------------------------
// Playlist / folder CRUD (v2 my-collection endpoints)
// ---------------------------------------------------------------------------

pub fn create_playlist(
    session: &Session,
    title: &str,
    description: &str,
    parent_folder_id: &str,
) -> RtcResult<Playlist> {
    let mut params = BTreeMap::new();
    params.insert("name".into(), ParamValue::Str(title.into()));
    params.insert("description".into(), ParamValue::Str(description.into()));
    params.insert("folderId".into(), ParamValue::Str(parent_folder_id.into()));
    let req = RequestArgs {
        method: "PUT".into(),
        path: "my-collection/playlists/folders/create-playlist".into(),
        base_url: Some(V2_BASE.into()),
        params: Some(params),
        headers: None,
        json_body: None,
        form_body: false,
    };
    let resp = session.request(req)?;
    let body = ok_or_status(resp)?;
    let data = body
        .get("data")
        .ok_or_else(|| RtcError::Other("create_playlist: response missing 'data'".into()))?;
    Ok(parse_playlist(data))
}

pub fn create_folder(
    session: &Session,
    title: &str,
    parent_folder_id: &str,
) -> RtcResult<Folder> {
    let mut params = BTreeMap::new();
    params.insert("name".into(), ParamValue::Str(title.into()));
    params.insert("folderId".into(), ParamValue::Str(parent_folder_id.into()));
    let req = RequestArgs {
        method: "PUT".into(),
        path: "my-collection/playlists/folders/create-folder".into(),
        base_url: Some(V2_BASE.into()),
        params: Some(params),
        headers: None,
        json_body: None,
        form_body: false,
    };
    let resp = session.request(req)?;
    let body = ok_or_status(resp)?;
    let data = body
        .get("data")
        .ok_or_else(|| RtcError::Other("create_folder: response missing 'data'".into()))?;
    Ok(crate::models::parse_folder(data))
}

/// Remove playlists or folders by trn. `kind` must be "playlist" or "folder";
/// `ids` is a list of bare ids (without the trn: prefix) or full trns. The
/// caller-supplied prefix is preserved when the id already begins with "trn:".
pub fn remove_folders_playlists(
    session: &Session,
    kind: &str,
    ids: &[String],
) -> RtcResult<bool> {
    let kind_norm = match kind.to_ascii_lowercase().as_str() {
        "playlist" => "playlist",
        "folder" => "folder",
        other => return Err(RtcError::InvalidInput(format!(
            "remove_folders_playlists: unknown kind {:?}",
            other
        ))),
    };
    let trns: Vec<String> = ids
        .iter()
        .map(|id| {
            if id.contains("trn:") {
                id.clone()
            } else {
                format!("trn:{}:{}", kind_norm, id)
            }
        })
        .collect();
    if trns.is_empty() {
        return Err(RtcError::InvalidInput(
            "remove_folders_playlists: empty id list".into(),
        ));
    }
    let mut params = BTreeMap::new();
    params.insert("trns".into(), ParamValue::Str(trns.join(",")));
    let req = RequestArgs {
        method: "PUT".into(),
        path: "my-collection/playlists/folders/remove".into(),
        base_url: Some(V2_BASE.into()),
        params: Some(params),
        headers: None,
        json_body: None,
        form_body: false,
    };
    let resp = session.request(req)?;
    Ok(resp.ok)
}
