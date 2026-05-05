//! Phase 6: lyrics + the long tail of secondary surfaces.
//!
//! These are individually small endpoints that the UI sprinkles around
//! (artist page sections, lyrics panel, "make a radio" actions). Putting
//! them in their own module keeps endpoints.rs focused on the primary
//! fetchers and gives Phase 7 a clear list of what it can drop from the
//! tidalapi proxy fallback.

use std::collections::BTreeMap;

use serde::Serialize;
use serde_json::Value;

use crate::endpoints::ok_or_status;
use crate::error::RtcResult;
use crate::favorites::{ListArgs, PageResponse};
use crate::models::{
    parse_album, parse_artist, parse_track, Album, Artist, Track,
};
use crate::request::{ParamValue, RequestArgs};
use crate::session::Session;

#[derive(Debug, Clone, Default, Serialize)]
pub struct Lyrics {
    pub track_id: i64,
    pub lyrics_provider: Option<String>,
    pub provider_track_id: Option<String>,
    pub provider_lyrics_id: Option<String>,
    pub text: Option<String>,
    /// LRC-style timestamped lyrics if TIDAL has them.
    pub subtitles: Option<String>,
    pub right_to_left: bool,
}

#[derive(Debug, Clone, Default, Serialize)]
pub struct Bio {
    pub text: Option<String>,
    pub summary: Option<String>,
    pub source: Option<String>,
    pub last_updated: Option<String>,
}

// ---------------------------------------------------------------------------
// Tracks
// ---------------------------------------------------------------------------

pub fn track_lyrics(session: &Session, track_id: i64) -> RtcResult<Lyrics> {
    let resp = session.request(get_path(&format!("tracks/{}/lyrics", track_id), None))?;
    let body = ok_or_status(resp)?;
    Ok(parse_lyrics(track_id, &body))
}

pub fn track_radio(
    session: &Session,
    track_id: i64,
    args: &ListArgs,
) -> RtcResult<PageResponse<Track>> {
    paginated_collection(
        session,
        &format!("tracks/{}/radio", track_id),
        args,
        None,
        parse_track,
    )
}

// ---------------------------------------------------------------------------
// Artists
// ---------------------------------------------------------------------------

pub fn artist_top_tracks(
    session: &Session,
    artist_id: i64,
    args: &ListArgs,
) -> RtcResult<PageResponse<Track>> {
    paginated_collection(
        session,
        &format!("artists/{}/toptracks", artist_id),
        args,
        None,
        parse_track,
    )
}

pub fn artist_albums(
    session: &Session,
    artist_id: i64,
    kind: ArtistAlbumKind,
    args: &ListArgs,
) -> RtcResult<PageResponse<Album>> {
    let mut extra = BTreeMap::new();
    if let Some(filter) = kind.filter_value() {
        extra.insert("filter".to_string(), ParamValue::Str(filter.into()));
    }
    paginated_collection(
        session,
        &format!("artists/{}/albums", artist_id),
        args,
        Some(extra),
        parse_album,
    )
}

pub fn artist_similar(
    session: &Session,
    artist_id: i64,
    args: &ListArgs,
) -> RtcResult<PageResponse<Artist>> {
    paginated_collection(
        session,
        &format!("artists/{}/similar", artist_id),
        args,
        None,
        parse_artist,
    )
}

pub fn artist_bio(session: &Session, artist_id: i64) -> RtcResult<Bio> {
    let resp = session.request(get_path(&format!("artists/{}/bio", artist_id), None))?;
    let body = ok_or_status(resp)?;
    Ok(parse_bio(&body))
}

#[derive(Debug, Clone, Copy)]
pub enum ArtistAlbumKind {
    All,
    EpsAndSingles,
    Compilations,
}

impl ArtistAlbumKind {
    pub fn filter_value(self) -> Option<&'static str> {
        match self {
            ArtistAlbumKind::All => None,
            ArtistAlbumKind::EpsAndSingles => Some("EPSANDSINGLES"),
            ArtistAlbumKind::Compilations => Some("COMPILATIONS"),
        }
    }

    pub fn from_str(s: &str) -> Self {
        match s.to_ascii_lowercase().as_str() {
            "ep_singles" | "epsandsingles" | "ep" | "singles" => ArtistAlbumKind::EpsAndSingles,
            "compilations" | "compilation" => ArtistAlbumKind::Compilations,
            _ => ArtistAlbumKind::All,
        }
    }
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn get_path(path: &str, params: Option<BTreeMap<String, ParamValue>>) -> RequestArgs {
    RequestArgs {
        method: "GET".into(),
        path: path.into(),
        base_url: None,
        params,
        headers: None,
        json_body: None,
        form_body: false,
    }
}

fn paginated_collection<T, F>(
    session: &Session,
    path: &str,
    args: &ListArgs,
    extra: Option<BTreeMap<String, ParamValue>>,
    parse_one: F,
) -> RtcResult<PageResponse<T>>
where
    F: Fn(&Value) -> T,
{
    let mut params = BTreeMap::new();
    params.insert(
        "limit".into(),
        ParamValue::Int(args.limit.clamp(1, 1000) as i64),
    );
    params.insert("offset".into(), ParamValue::Int(args.offset.max(0) as i64));
    if let Some(order) = args.order.as_ref().filter(|s| !s.is_empty()) {
        params.insert("order".into(), ParamValue::Str(order.clone()));
    }
    if let Some(dir) = args.order_direction.as_ref().filter(|s| !s.is_empty()) {
        params.insert("orderDirection".into(), ParamValue::Str(dir.clone()));
    }
    if let Some(extras) = extra {
        for (k, v) in extras {
            params.insert(k, v);
        }
    }
    let resp = session.request(get_path(path, Some(params)))?;
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
    Ok(PageResponse {
        items,
        total_number_of_items: i32::try_from(total).unwrap_or(-1),
        limit: args.limit,
        offset: args.offset,
    })
}

fn extract_item(value: &Value) -> &Value {
    value.get("item").unwrap_or(value)
}

fn parse_lyrics(fallback_id: i64, value: &Value) -> Lyrics {
    let track_id = value
        .get("trackId")
        .and_then(|v| v.as_i64())
        .unwrap_or(fallback_id);
    Lyrics {
        track_id,
        lyrics_provider: value
            .get("lyricsProvider")
            .and_then(|v| v.as_str())
            .map(str::to_string),
        provider_track_id: value
            .get("providerCommontrackId")
            .map(|v| v.to_string().trim_matches('"').to_string())
            .filter(|s| !s.is_empty() && s != "null"),
        provider_lyrics_id: value
            .get("providerLyricsId")
            .map(|v| v.to_string().trim_matches('"').to_string())
            .filter(|s| !s.is_empty() && s != "null"),
        text: value
            .get("lyrics")
            .and_then(|v| v.as_str())
            .map(str::to_string)
            .filter(|s| !s.is_empty()),
        subtitles: value
            .get("subtitles")
            .and_then(|v| v.as_str())
            .map(str::to_string)
            .filter(|s| !s.is_empty()),
        right_to_left: value
            .get("isRightToLeft")
            .and_then(|v| v.as_bool())
            .unwrap_or(false),
    }
}

fn parse_bio(value: &Value) -> Bio {
    Bio {
        text: value
            .get("text")
            .and_then(|v| v.as_str())
            .map(str::to_string),
        summary: value
            .get("summary")
            .and_then(|v| v.as_str())
            .map(str::to_string),
        source: value
            .get("source")
            .and_then(|v| v.as_str())
            .map(str::to_string),
        last_updated: value
            .get("lastUpdated")
            .and_then(|v| v.as_str())
            .map(str::to_string),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn parse_lyrics_extracts_text_and_subtitles() {
        let body = json!({
            "trackId": 42,
            "lyricsProvider": "MUSIXMATCH",
            "providerCommontrackId": "abc",
            "providerLyricsId": "lid",
            "lyrics": "verse one\nverse two",
            "subtitles": "[00:00.00]verse one\n[00:10.00]verse two",
            "isRightToLeft": false
        });
        let l = parse_lyrics(42, &body);
        assert_eq!(l.track_id, 42);
        assert_eq!(l.lyrics_provider.as_deref(), Some("MUSIXMATCH"));
        assert!(l.text.unwrap().contains("verse one"));
        assert!(l.subtitles.unwrap().contains("[00:00.00]"));
        assert!(!l.right_to_left);
    }

    #[test]
    fn parse_lyrics_handles_empty_text() {
        let body = json!({
            "trackId": 5,
            "lyrics": "",
            "subtitles": "",
        });
        let l = parse_lyrics(5, &body);
        assert!(l.text.is_none());
        assert!(l.subtitles.is_none());
    }

    #[test]
    fn artist_album_kind_dispatch() {
        assert!(matches!(
            ArtistAlbumKind::from_str("ep_singles"),
            ArtistAlbumKind::EpsAndSingles
        ));
        assert!(matches!(
            ArtistAlbumKind::from_str("compilations"),
            ArtistAlbumKind::Compilations
        ));
        assert!(matches!(
            ArtistAlbumKind::from_str("everything"),
            ArtistAlbumKind::All
        ));
        assert_eq!(
            ArtistAlbumKind::EpsAndSingles.filter_value(),
            Some("EPSANDSINGLES")
        );
        assert_eq!(ArtistAlbumKind::All.filter_value(), None);
    }
}
