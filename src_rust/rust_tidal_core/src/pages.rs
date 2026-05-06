//! Discovery `/pages/*` parser. TIDAL returns two shapes:
//!   V1: `{"title": ..., "rows": [{"modules": [<module>, ...]}, ...]}`
//!   V2: `{"title": ..., "items": [<category>, ...]}`
//! Each module is a `PageCategory` with a list of `items`. The shape of each
//! item depends on the category's `type` (TRACK_LIST/ALBUM_LIST/PAGE_LINKS/
//! FEATURED_PROMOTIONS/...). Mirrors the Python `_PageCategory._build_items`
//! logic in `src/_rust/tidal.py`.
//!
//! V2 home/feed/static is shaped a little differently — its category items
//! are `{type, data}` tuples — and we handle that via the V2 path too.
//!
//! `pages/m_1950s` and friends use the same V1 shape as every other
//! `/pages/*`, so the same parser covers Decades.
//!
//! This module returns owned types — Phase 6 callers don't need to keep the
//! raw `Value` around.
//!
//! Item parsing is intentionally typed-only for the items we want to show in
//! the discovery views — TRACK / ALBUM / ARTIST / PLAYLIST / MIX go through
//! the model parsers; PAGE_LINKS keep title + apiPath + imageId so genre/mood
//! tabs can render. Promotional cards (FEATURED_PROMOTIONS,
//! MULTIPLE_TOP_PROMOTIONS) are exposed via the same `Card` variant.

use serde::Serialize;
use serde_json::Value;

use crate::endpoints::ok_or_status;
use crate::error::RtcResult;
use crate::models::{
    parse_album, parse_artist, parse_mix, parse_playlist, parse_track, parse_video, Album, Artist,
    Mix, Playlist, Track, Video,
};
use crate::request::ParamValue;
use crate::session::Session;

#[derive(Debug, Clone, Serialize)]
pub struct Page {
    pub title: Option<String>,
    pub categories: Vec<PageCategory>,
}

#[derive(Debug, Clone, Serialize)]
pub struct PageCategory {
    pub title: Option<String>,
    pub subtitle: Option<String>,
    pub description: Option<String>,
    /// Raw category type (TRACK_LIST / PAGE_LINKS / FEATURED_PROMOTIONS / ...)
    pub category_type: Option<String>,
    /// `{title, apiPath}` of the "view all" link, when the category surfaces one.
    pub more: Option<More>,
    pub items: Vec<PageItem>,
}

#[derive(Debug, Clone, Serialize)]
pub struct More {
    pub api_path: String,
    pub title: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(tag = "kind", rename_all = "lowercase")]
pub enum PageItem {
    Track(Track),
    Album(Album),
    Artist(Artist),
    Playlist(Playlist),
    Mix(Mix),
    Video(Video),
    /// PageItem / PageLink card (header / shortHeader / imageId / type /
    /// apiPath). Used by FEATURED_PROMOTIONS / PAGE_LINKS / fallback paths.
    Card(Card),
}

#[derive(Debug, Clone, Default, Serialize)]
pub struct Card {
    pub title: Option<String>,
    pub header: Option<String>,
    pub short_header: Option<String>,
    pub sub_title: Option<String>,
    pub short_sub_header: Option<String>,
    pub image_id: Option<String>,
    /// Raw `type` — TRACK / ALBUM / EXTURL / ... when the card came from a
    /// promotional module; None when it's a PageLink.
    pub kind: Option<String>,
    pub api_path: Option<String>,
    /// e.g. apiPath -> the artifact id when present (mostly promotional cards).
    pub artifact_id: Option<String>,
}

pub fn fetch_page(
    session: &Session,
    path: &str,
    params: Option<std::collections::BTreeMap<String, ParamValue>>,
) -> RtcResult<Page> {
    let raw = session.page_get_raw(path, params)?;
    Ok(parse_page(&raw))
}

/// `home/feed/static` lives on /v2/ and uses the V2 categories shape but
/// without a wrapping `pages/...` URL.
pub fn fetch_home_feed(session: &Session) -> RtcResult<Page> {
    let mut params = std::collections::BTreeMap::new();
    params.insert("deviceType".into(), ParamValue::Str("BROWSER".into()));
    params.insert("locale".into(), ParamValue::Str("en_US".into()));
    params.insert("platform".into(), ParamValue::Str("WEB".into()));
    let resp = session.request(crate::request::RequestArgs {
        method: "GET".into(),
        path: "home/feed/static".into(),
        base_url: Some("https://api.tidal.com/v2/".into()),
        params: Some(params),
        headers: None,
        json_body: None,
        form_body: false,
    })?;
    let body = ok_or_status(resp)?;
    Ok(parse_page(&body))
}

pub fn parse_page(raw: &Value) -> Page {
    let title = raw
        .get("title")
        .and_then(|v| v.as_str())
        .map(str::to_string);
    let categories = if let Some(rows) = raw.get("rows").and_then(|v| v.as_array()) {
        rows.iter()
            .filter_map(|row| {
                let modules = row.get("modules")?.as_array()?;
                modules.iter().next().map(parse_category)
            })
            .collect()
    } else if let Some(items) = raw.get("items").and_then(|v| v.as_array()) {
        items.iter().map(parse_category).collect()
    } else {
        Vec::new()
    };
    Page { title, categories }
}

fn parse_category(raw: &Value) -> PageCategory {
    let title = raw
        .get("title")
        .and_then(|v| v.as_str())
        .map(str::to_string);
    let subtitle = raw
        .get("subtitle")
        .and_then(|v| v.as_str())
        .map(str::to_string);
    let description = raw
        .get("description")
        .and_then(|v| v.as_str())
        .map(str::to_string)
        .or_else(|| title.clone());
    let category_type = raw
        .get("type")
        .and_then(|v| v.as_str())
        .map(str::to_string);
    let more = parse_more(raw);
    let items = build_items(raw, category_type.as_deref().unwrap_or(""));
    PageCategory {
        title,
        subtitle,
        description,
        category_type,
        more,
        items,
    }
}

fn parse_more(raw: &Value) -> Option<More> {
    if let Some(show_more) = raw.get("showMore") {
        if let Some(api_path) = show_more.get("apiPath").and_then(|v| v.as_str()) {
            return Some(More {
                api_path: api_path.to_string(),
                title: show_more
                    .get("title")
                    .and_then(|v| v.as_str())
                    .map(str::to_string),
            });
        }
    }
    if let Some(view_all) = raw.get("viewAll").and_then(|v| v.as_str()) {
        if !view_all.is_empty() {
            return Some(More {
                api_path: view_all.to_string(),
                title: raw
                    .get("title")
                    .and_then(|v| v.as_str())
                    .map(str::to_string),
            });
        }
    }
    None
}

fn build_items(raw: &Value, cat_type: &str) -> Vec<PageItem> {
    let upper = cat_type.to_ascii_uppercase();

    // *_HEADER: single-item header.
    match upper.as_str() {
        "MIX_HEADER" => {
            if let Some(mix) = raw.get("mix") {
                return vec![PageItem::Mix(parse_mix(mix))];
            }
        }
        "ARTIST_HEADER" => {
            if let Some(a) = raw.get("artist") {
                return vec![PageItem::Artist(parse_artist(a))];
            }
        }
        "ALBUM_HEADER" => {
            if let Some(a) = raw.get("album") {
                return vec![PageItem::Album(parse_album(a))];
            }
        }
        _ => {}
    }

    // V2 typed items (top-level "items" with each carrying { type, data }).
    if let Some(items) = raw.get("items").and_then(|v| v.as_array()) {
        if items.iter().any(|it| it.get("data").is_some()) {
            return items.iter().filter_map(|it| parse_v2_item(it)).collect();
        }
    }

    let paged_items: Vec<&Value> = raw
        .get("pagedList")
        .and_then(|v| v.get("items"))
        .and_then(|v| v.as_array())
        .map(|arr| arr.iter().collect())
        .unwrap_or_default();

    match upper.as_str() {
        "FEATURED_PROMOTIONS" | "MULTIPLE_TOP_PROMOTIONS" => raw
            .get("items")
            .and_then(|v| v.as_array())
            .map(|arr| arr.iter().map(parse_card).map(PageItem::Card).collect())
            .unwrap_or_default(),
        "PAGE_LINKS" | "PAGE_LINKS_CLOUD" | "ARTICLE_LIST" => paged_items
            .iter()
            .map(|it| PageItem::Card(parse_card(it)))
            .collect(),
        "SOCIAL" => raw
            .get("socialProfiles")
            .and_then(|v| v.as_array())
            .map(|arr| arr.iter().map(parse_card).map(PageItem::Card).collect())
            .unwrap_or_default(),
        "ITEM_LIST_WITH_ROLES" => paged_items
            .iter()
            .filter_map(|entry| {
                entry
                    .get("item")
                    .map(parse_track)
                    .map(PageItem::Track)
            })
            .collect(),
        "HIGHLIGHT_MODULE" => raw
            .get("highlights")
            .and_then(|v| v.as_array())
            .map(|arr| {
                arr.iter()
                    .filter_map(|h| {
                        let item = h.get("item")?;
                        let kind = item.get("type").and_then(|v| v.as_str()).unwrap_or("");
                        wrap_v2(kind, item)
                    })
                    .collect()
            })
            .unwrap_or_default(),
        "MIXED_TYPES_LIST" | "ALBUM_ITEMS" => paged_items
            .iter()
            .filter_map(|it| parse_v2_item(it))
            .collect(),
        "TRACK_LIST" => paged_items
            .iter()
            .map(|it| PageItem::Track(parse_track(it)))
            .collect(),
        "ALBUM_LIST" => paged_items
            .iter()
            .map(|it| PageItem::Album(parse_album(it)))
            .collect(),
        "ARTIST_LIST" => paged_items
            .iter()
            .map(|it| PageItem::Artist(parse_artist(it)))
            .collect(),
        "PLAYLIST_LIST" => paged_items
            .iter()
            .map(|it| PageItem::Playlist(parse_playlist(it)))
            .collect(),
        "VIDEO_LIST" => paged_items
            .iter()
            .map(|it| PageItem::Video(parse_video(it)))
            .collect(),
        "MIX_LIST" => paged_items
            .iter()
            .map(|it| PageItem::Mix(parse_mix(it)))
            .collect(),
        _ => paged_items
            .iter()
            .map(|it| PageItem::Card(parse_card(it)))
            .collect(),
    }
}

fn parse_v2_item(entry: &Value) -> Option<PageItem> {
    let kind = entry.get("type").and_then(|v| v.as_str()).unwrap_or("");
    let data = entry.get("data").unwrap_or(entry);
    wrap_v2(kind, data)
}

fn wrap_v2(kind: &str, data: &Value) -> Option<PageItem> {
    if !data.is_object() {
        return None;
    }
    Some(match kind.to_ascii_uppercase().as_str() {
        "TRACK" => PageItem::Track(parse_track(data)),
        "ALBUM" => PageItem::Album(parse_album(data)),
        "ARTIST" => PageItem::Artist(parse_artist(data)),
        "PLAYLIST" => PageItem::Playlist(parse_playlist(data)),
        "VIDEO" => PageItem::Video(parse_video(data)),
        "MIX" => PageItem::Mix(parse_mix(data)),
        _ => PageItem::Card(parse_card(data)),
    })
}

fn parse_card(raw: &Value) -> Card {
    let s = |key: &str| -> Option<String> {
        raw.get(key)
            .and_then(|v| v.as_str())
            .map(str::to_string)
            .filter(|s| !s.is_empty())
    };
    Card {
        title: s("title"),
        header: s("header"),
        short_header: s("shortHeader"),
        sub_title: s("subTitle").or_else(|| s("subtitle")),
        short_sub_header: s("shortSubHeader"),
        image_id: s("imageId"),
        kind: s("type"),
        api_path: s("apiPath"),
        artifact_id: s("artifactId"),
    }
}
