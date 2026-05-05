//! Listing endpoints for already-fetched models — replaces tidalapi's
//! `Album.tracks()`, `Playlist.tracks()`, `Playlist.items()`, `Mix.items()`.
//!
//! All four return paged content — TIDAL caps single requests at 50 or 100
//! items, so callers need offset-loop pagination. We expose a single-page
//! shape and let backend/tidal.py drive the loop, matching how tidalapi's
//! upstream pagination helper works.

use std::collections::BTreeMap;

use crate::endpoints::ok_or_status;
use crate::error::RtcResult;
use crate::favorites::{ListArgs, PageResponse};
use crate::models::{parse_track, parse_video, Track, Video};
use crate::request::{ParamValue, RequestArgs};
use crate::session::Session;

fn build_paged_get(path: &str, args: &ListArgs) -> RequestArgs {
    let mut params = BTreeMap::new();
    params.insert(
        "limit".into(),
        ParamValue::Int(args.limit.clamp(1, 1000) as i64),
    );
    params.insert(
        "offset".into(),
        ParamValue::Int(args.offset.max(0) as i64),
    );
    if let Some(order) = args.order.as_ref().filter(|s| !s.is_empty()) {
        params.insert("order".into(), ParamValue::Str(order.clone()));
    }
    if let Some(dir) = args.order_direction.as_ref().filter(|s| !s.is_empty()) {
        params.insert("orderDirection".into(), ParamValue::Str(dir.clone()));
    }
    RequestArgs {
        method: "GET".into(),
        path: path.into(),
        base_url: None,
        params: Some(params),
        headers: None,
        json_body: None,
        form_body: false,
    }
}

pub fn album_tracks(
    session: &Session,
    album_id: i64,
    args: &ListArgs,
) -> RtcResult<PageResponse<Track>> {
    let resp = session.request(build_paged_get(&format!("albums/{}/tracks", album_id), args))?;
    let body = ok_or_status(resp)?;
    let total = body
        .get("totalNumberOfItems")
        .and_then(|v| v.as_i64())
        .unwrap_or(-1);
    let items = body
        .get("items")
        .and_then(|v| v.as_array())
        .map(|arr| arr.iter().map(parse_track).collect())
        .unwrap_or_default();
    Ok(PageResponse {
        items,
        total_number_of_items: i32::try_from(total).unwrap_or(-1),
        limit: args.limit,
        offset: args.offset,
    })
}

#[derive(Debug, Clone, serde::Serialize)]
#[serde(tag = "kind", rename_all = "lowercase")]
pub enum PlaylistItem {
    Track(Track),
    Video(Video),
}

pub fn playlist_items(
    session: &Session,
    playlist_id: &str,
    args: &ListArgs,
) -> RtcResult<PageResponse<PlaylistItem>> {
    // /v1/playlists/<uuid>/items returns mixed Track/Video items as
    // {"item": {...}, "type": "track" | "video"}.
    let resp = session.request(build_paged_get(
        &format!("playlists/{}/items", playlist_id),
        args,
    ))?;
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
                .filter_map(|raw| {
                    let kind = raw.get("type").and_then(|v| v.as_str()).unwrap_or("track");
                    let inner = raw.get("item").unwrap_or(raw);
                    match kind.to_ascii_lowercase().as_str() {
                        "video" => Some(PlaylistItem::Video(parse_video(inner))),
                        _ => Some(PlaylistItem::Track(parse_track(inner))),
                    }
                })
                .collect()
        })
        .unwrap_or_default();
    Ok(PageResponse {
        items,
        total_number_of_items: i32::try_from(total).unwrap_or(-1),
        limit: args.limit,
        offset: args.offset,
    })
}

pub fn playlist_tracks(
    session: &Session,
    playlist_id: &str,
    args: &ListArgs,
) -> RtcResult<PageResponse<Track>> {
    // tracks-only endpoint (`tracks` instead of `items`) to avoid filtering
    // on the client side.
    let resp = session.request(build_paged_get(
        &format!("playlists/{}/tracks", playlist_id),
        args,
    ))?;
    let body = ok_or_status(resp)?;
    let total = body
        .get("totalNumberOfItems")
        .and_then(|v| v.as_i64())
        .unwrap_or(-1);
    let items = body
        .get("items")
        .and_then(|v| v.as_array())
        .map(|arr| arr.iter().map(parse_track).collect())
        .unwrap_or_default();
    Ok(PageResponse {
        items,
        total_number_of_items: i32::try_from(total).unwrap_or(-1),
        limit: args.limit,
        offset: args.offset,
    })
}

pub fn mix_items(
    session: &Session,
    mix_id: &str,
    args: &ListArgs,
) -> RtcResult<PageResponse<PlaylistItem>> {
    // /v1/mixes/<id>/items has the same wrapper shape as playlists.
    let resp = session.request(build_paged_get(&format!("mixes/{}/items", mix_id), args))?;
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
                .filter_map(|raw| {
                    let kind = raw.get("type").and_then(|v| v.as_str()).unwrap_or("track");
                    let inner = raw.get("item").unwrap_or(raw);
                    match kind.to_ascii_lowercase().as_str() {
                        "video" => Some(PlaylistItem::Video(parse_video(inner))),
                        _ => Some(PlaylistItem::Track(parse_track(inner))),
                    }
                })
                .collect()
        })
        .unwrap_or_default();
    Ok(PageResponse {
        items,
        total_number_of_items: i32::try_from(total).unwrap_or(-1),
        limit: args.limit,
        offset: args.offset,
    })
}
