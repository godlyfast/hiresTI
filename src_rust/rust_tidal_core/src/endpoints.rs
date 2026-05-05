//! Endpoint helpers built on top of `Session::request()`.
//!
//! Each `fetch_*` method maps to a single TIDAL endpoint, parses the
//! response with the corresponding `models::parse_*` helper, and returns
//! a strongly-typed Rust struct that serializes cleanly back to JSON for
//! the Python wrapper to consume.

use std::collections::BTreeMap;

use serde_json::{json, Value};

use crate::error::{RtcError, RtcResult};
use crate::models::{
    parse_album, parse_artist, parse_folder, parse_mix, parse_playlist, parse_track, parse_video,
    Album, Artist, Folder, Mix, Playlist, SearchResults, Track,
};
use crate::request::{ParamValue, RequestArgs};
use crate::session::Session;

pub fn ok_or_status(resp: crate::request::ResponseJson) -> RtcResult<Value> {
    if resp.ok {
        Ok(resp.body)
    } else if resp.status == 404 {
        Err(RtcError::NotFound(format!("HTTP 404: {}", resp.body)))
    } else if matches!(resp.status, 401 | 403) {
        Err(RtcError::Auth(format!("HTTP {}: {}", resp.status, resp.body)))
    } else if (500..600).contains(&resp.status) {
        Err(RtcError::Server {
            status: resp.status,
            body: resp.body.to_string(),
        })
    } else {
        Err(RtcError::Client {
            status: resp.status,
            body: resp.body.to_string(),
        })
    }
}

fn build_get(path: &str) -> RequestArgs {
    RequestArgs {
        method: "GET".into(),
        path: path.into(),
        base_url: None,
        params: None,
        headers: None,
        json_body: None,
        form_body: false,
    }
}

fn build_get_with_params(path: &str, params: BTreeMap<String, ParamValue>) -> RequestArgs {
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

pub fn fetch_track(session: &Session, id: i64) -> RtcResult<Track> {
    let resp = session.request(build_get(&format!("tracks/{}", id)))?;
    let body = ok_or_status(resp)?;
    Ok(parse_track(&body))
}

pub fn fetch_album(session: &Session, id: i64) -> RtcResult<Album> {
    let resp = session.request(build_get(&format!("albums/{}", id)))?;
    let body = ok_or_status(resp)?;
    Ok(parse_album(&body))
}

pub fn fetch_artist(session: &Session, id: i64) -> RtcResult<Artist> {
    let resp = session.request(build_get(&format!("artists/{}", id)))?;
    let body = ok_or_status(resp)?;
    Ok(parse_artist(&body))
}

pub fn fetch_playlist(session: &Session, id: &str) -> RtcResult<Playlist> {
    let resp = session.request(build_get(&format!("playlists/{}", id)))?;
    let body = ok_or_status(resp)?;
    Ok(parse_playlist(&body))
}

pub fn fetch_mix(session: &Session, id: &str) -> RtcResult<Mix> {
    // tidalapi.Mix.get hits /mixes/<id>?mixId=<id> in some flows. Testing
    // shows /pages/mix returns the mix metadata, but the simple GET on
    // /mixes/<id> works for our hot paths. If TIDAL changes that we'll
    // adjust here without touching the rest of the stack.
    let mut params = BTreeMap::new();
    params.insert("mixId".to_string(), ParamValue::Str(id.to_string()));
    params.insert("deviceType".to_string(), ParamValue::Str("BROWSER".into()));
    let resp = session.request(build_get_with_params("pages/mix", params));
    if let Ok(r) = resp {
        if r.ok {
            // pages/mix returns {rows: [{modules: [{mixHeader: {...}}]}]}
            if let Some(header) = extract_pages_mix_header(&r.body) {
                let mut mix = parse_mix(&header);
                if mix.id.is_empty() {
                    mix.id = id.to_string();
                }
                return Ok(mix);
            }
        }
    }
    // Fallback to constructing a bare Mix with just the ID — Phase 4 mix
    // items API will populate the rest.
    Ok(Mix {
        id: id.to_string(),
        ..Default::default()
    })
}

pub fn fetch_folder(session: &Session, id: &str) -> RtcResult<Folder> {
    // Folder is a v2 surface. tidalapi calls /v2/my-collection/folders/<id>
    // but the canonical metadata lookup is /v2/my-collection/playlists?folderId=...
    // — there's no plain /folders/<id> endpoint. For Phase 3 we synthesize
    // a Folder from just the ID + name we already know; Phase 4 fleshes
    // out the listing endpoints.
    let mut params = BTreeMap::new();
    params.insert("folderId".to_string(), ParamValue::Str(id.to_string()));
    let mut args = build_get_with_params("my-collection/playlists/folders", params);
    args.base_url = Some("https://api.tidal.com/v2/".into());
    if let Ok(resp) = session.request(args) {
        if resp.ok {
            if let Some(items) = resp.body.get("items").and_then(|v| v.as_array()) {
                for item in items {
                    if item.get("id").and_then(|v| v.as_str()) == Some(id) {
                        return Ok(parse_folder(item));
                    }
                }
            }
        }
    }
    Ok(Folder {
        id: id.to_string(),
        name: String::new(),
        ..Default::default()
    })
}

pub fn search(session: &Session, query: &str, limit: i32) -> RtcResult<SearchResults> {
    let mut params = BTreeMap::new();
    params.insert("query".to_string(), ParamValue::Str(query.to_string()));
    params.insert(
        "limit".to_string(),
        ParamValue::Int(limit.clamp(1, 300) as i64),
    );
    params.insert("offset".to_string(), ParamValue::Int(0));
    params.insert(
        "types".to_string(),
        ParamValue::Str("ARTISTS,ALBUMS,TRACKS,VIDEOS,PLAYLISTS".into()),
    );
    let resp = session.request(build_get_with_params("search", params))?;
    let body = ok_or_status(resp)?;
    let map_array = |key: &str| -> Vec<Value> {
        body.get(key)
            .and_then(|v| v.get("items"))
            .and_then(|v| v.as_array())
            .cloned()
            .unwrap_or_default()
    };
    Ok(SearchResults {
        artists: map_array("artists").iter().map(parse_artist).collect(),
        albums: map_array("albums").iter().map(parse_album).collect(),
        tracks: map_array("tracks").iter().map(parse_track).collect(),
        playlists: map_array("playlists").iter().map(parse_playlist).collect(),
        videos: map_array("videos").iter().map(parse_video).collect(),
    })
}

/// Surface the raw response of `pages/X?...` since tidalapi.Page is too
/// shape-specific to port wholesale at this stage. Phase 3 callers consume
/// the JSON dict directly; Phase 3.5 (later) will normalize categories.
pub fn page_get_raw(
    session: &Session,
    path: &str,
    extra_params: Option<BTreeMap<String, ParamValue>>,
) -> RtcResult<Value> {
    let mut params = extra_params.unwrap_or_default();
    if !params.contains_key("deviceType") {
        params.insert("deviceType".into(), ParamValue::Str("BROWSER".into()));
    }
    let resp = session.request(build_get_with_params(path, params))?;
    ok_or_status(resp)
}

fn extract_pages_mix_header(body: &Value) -> Option<Value> {
    let rows = body.get("rows")?.as_array()?;
    for row in rows {
        let modules = row.get("modules").and_then(|v| v.as_array())?;
        for module in modules {
            if let Some(header) = module.get("mix") {
                return Some(header.clone());
            }
            if let Some(header) = module.get("mixHeader") {
                return Some(header.clone());
            }
        }
    }
    None
}

/// Convenience: fetch an arbitrary URL path and parse it through one of the
/// model parsers. Used when callers already have a model ID type-erased
/// behind a `kind` string (e.g. home-page items).
pub fn parse_typed(kind: &str, value: &Value) -> RtcResult<Value> {
    let parsed = match kind.to_ascii_lowercase().as_str() {
        "track" => json!(parse_track(value)),
        "album" => json!(parse_album(value)),
        "artist" => json!(parse_artist(value)),
        "playlist" => json!(parse_playlist(value)),
        "mix" => json!(parse_mix(value)),
        "video" => json!(parse_video(value)),
        "folder" => json!(parse_folder(value)),
        other => return Err(RtcError::InvalidInput(format!("unknown kind: {}", other))),
    };
    Ok(parsed)
}
