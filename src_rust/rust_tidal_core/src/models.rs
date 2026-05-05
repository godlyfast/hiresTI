//! TIDAL response models, normalized to the field shape hiresTI's Python
//! layer expects (the subset of tidalapi attributes it actually reads —
//! see Phase 0 inventory section 5).
//!
//! TIDAL's JSON uses camelCase keys and is inconsistent about which fields
//! are present, so every parser is field-by-field with `.get().and_then(...)`
//! rather than `serde::Deserialize` on the raw shape — that lets us survive
//! upstream schema drift without panicking.

use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ArtistRef {
    pub id: i64,
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub picture: Option<String>,
    /// "MAIN" / "FEATURED" — TIDAL exposes this on track-level artists.
    #[serde(skip_serializing_if = "Option::is_none")]
    #[serde(rename = "type")]
    pub kind: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct AlbumRef {
    pub id: i64,
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cover: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub vibrant_color: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub release_date: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct Track {
    pub id: i64,
    pub name: String,
    pub duration: i32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub track_num: Option<i32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub volume_num: Option<i32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub isrc: Option<String>,
    #[serde(default)]
    pub explicit: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub audio_quality: Option<String>,
    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    pub audio_modes: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub artist: Option<ArtistRef>,
    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    pub artists: Vec<ArtistRef>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub album: Option<AlbumRef>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub copyright: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub version: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct Album {
    pub id: i64,
    pub name: String,
    pub duration: Option<i32>,
    pub num_tracks: Option<i32>,
    pub release_date: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cover: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub vibrant_color: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub explicit: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub artist: Option<ArtistRef>,
    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    pub artists: Vec<ArtistRef>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub audio_quality: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub copyright: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub upc: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct Artist {
    pub id: i64,
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub picture: Option<String>,
    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    pub artist_types: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct Playlist {
    pub id: String,
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub trn: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub kind: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub creator_id: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub creator_name: Option<String>,
    pub num_tracks: Option<i32>,
    pub duration: Option<i32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_updated: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub created: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub image: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub square_image: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct Mix {
    pub id: String,
    pub title: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sub_title: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub mix_type: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub image: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub detail_image: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct Folder {
    pub id: String,
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub parent_folder_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub trn: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub created: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_modified_at: Option<String>,
    pub total_number_of_items: Option<i32>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct Video {
    pub id: i64,
    pub name: String,
    pub duration: Option<i32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub image_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub artist: Option<ArtistRef>,
    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    pub artists: Vec<ArtistRef>,
}

#[derive(Debug, Clone, Serialize, Default)]
pub struct SearchResults {
    pub artists: Vec<Artist>,
    pub albums: Vec<Album>,
    pub tracks: Vec<Track>,
    pub playlists: Vec<Playlist>,
    pub videos: Vec<Video>,
}

// ---------------------------------------------------------------------------
// Parsers
// ---------------------------------------------------------------------------

fn s_get<'a>(value: &'a Value, key: &str) -> Option<&'a Value> {
    value.get(key).filter(|v| !v.is_null())
}

fn parse_str(value: &Value, key: &str) -> Option<String> {
    s_get(value, key).and_then(|v| v.as_str()).map(str::to_string)
}

fn parse_i64(value: &Value, key: &str) -> Option<i64> {
    s_get(value, key).and_then(|v| v.as_i64())
}

fn parse_i32(value: &Value, key: &str) -> Option<i32> {
    s_get(value, key)
        .and_then(|v| v.as_i64())
        .and_then(|n| i32::try_from(n).ok())
}

fn parse_bool(value: &Value, key: &str) -> Option<bool> {
    s_get(value, key).and_then(|v| v.as_bool())
}

fn parse_str_array(value: &Value, key: &str) -> Vec<String> {
    s_get(value, key)
        .and_then(|v| v.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|x| x.as_str().map(str::to_string))
                .collect()
        })
        .unwrap_or_default()
}

pub fn parse_artist_ref(value: &Value) -> ArtistRef {
    ArtistRef {
        id: parse_i64(value, "id").unwrap_or(0),
        name: parse_str(value, "name").unwrap_or_default(),
        picture: parse_str(value, "picture"),
        kind: parse_str(value, "type"),
    }
}

pub fn parse_album_ref(value: &Value) -> AlbumRef {
    AlbumRef {
        id: parse_i64(value, "id").unwrap_or(0),
        name: parse_str(value, "title").unwrap_or_default(),
        cover: parse_str(value, "cover"),
        vibrant_color: parse_str(value, "vibrantColor"),
        release_date: parse_str(value, "releaseDate"),
    }
}

pub fn parse_track(value: &Value) -> Track {
    let artist = s_get(value, "artist").map(parse_artist_ref);
    let artists = s_get(value, "artists")
        .and_then(|v| v.as_array())
        .map(|a| a.iter().map(parse_artist_ref).collect())
        .unwrap_or_default();
    let album = s_get(value, "album").map(parse_album_ref);
    Track {
        id: parse_i64(value, "id").unwrap_or(0),
        name: parse_str(value, "title").unwrap_or_default(),
        duration: parse_i32(value, "duration").unwrap_or(0),
        track_num: parse_i32(value, "trackNumber"),
        volume_num: parse_i32(value, "volumeNumber"),
        isrc: parse_str(value, "isrc"),
        explicit: parse_bool(value, "explicit").unwrap_or(false),
        audio_quality: parse_str(value, "audioQuality"),
        audio_modes: parse_str_array(value, "audioModes"),
        artist,
        artists,
        album,
        copyright: parse_str(value, "copyright"),
        url: parse_str(value, "url"),
        version: parse_str(value, "version"),
    }
}

pub fn parse_album(value: &Value) -> Album {
    let artist = s_get(value, "artist").map(parse_artist_ref);
    let artists = s_get(value, "artists")
        .and_then(|v| v.as_array())
        .map(|a| a.iter().map(parse_artist_ref).collect())
        .unwrap_or_default();
    Album {
        id: parse_i64(value, "id").unwrap_or(0),
        name: parse_str(value, "title").unwrap_or_default(),
        duration: parse_i32(value, "duration"),
        num_tracks: parse_i32(value, "numberOfTracks"),
        release_date: parse_str(value, "releaseDate"),
        cover: parse_str(value, "cover"),
        vibrant_color: parse_str(value, "vibrantColor"),
        explicit: parse_bool(value, "explicit"),
        artist,
        artists,
        audio_quality: parse_str(value, "audioQuality"),
        copyright: parse_str(value, "copyright"),
        upc: parse_str(value, "upc"),
    }
}

pub fn parse_artist(value: &Value) -> Artist {
    Artist {
        id: parse_i64(value, "id").unwrap_or(0),
        name: parse_str(value, "name").unwrap_or_default(),
        picture: parse_str(value, "picture"),
        artist_types: parse_str_array(value, "artistTypes"),
        url: parse_str(value, "url"),
    }
}

pub fn parse_playlist(value: &Value) -> Playlist {
    let id = parse_str(value, "uuid").unwrap_or_else(|| parse_str(value, "id").unwrap_or_default());
    let creator_id = s_get(value, "creator")
        .and_then(|c| c.get("id"))
        .and_then(|v| v.as_i64());
    let creator_name = s_get(value, "creator")
        .and_then(|c| c.get("name"))
        .and_then(|v| v.as_str())
        .map(str::to_string);
    Playlist {
        id: id.clone(),
        name: parse_str(value, "title").unwrap_or_default(),
        trn: Some(format!("trn:playlist:{}", id)),
        description: parse_str(value, "description"),
        kind: parse_str(value, "type"),
        creator_id,
        creator_name,
        num_tracks: parse_i32(value, "numberOfTracks"),
        duration: parse_i32(value, "duration"),
        last_updated: parse_str(value, "lastUpdated"),
        created: parse_str(value, "created"),
        image: parse_str(value, "image"),
        square_image: parse_str(value, "squareImage"),
    }
}

pub fn parse_mix(value: &Value) -> Mix {
    let id = parse_str(value, "id").unwrap_or_default();
    let title = parse_str(value, "title").unwrap_or_default();
    let sub_title = parse_str(value, "subTitle").or_else(|| parse_str(value, "sub_title"));
    Mix {
        id,
        title,
        sub_title,
        mix_type: parse_str(value, "mixType"),
        image: parse_str(value, "image"),
        detail_image: parse_str(value, "detailImage"),
    }
}

pub fn parse_folder(value: &Value) -> Folder {
    Folder {
        id: parse_str(value, "id").unwrap_or_default(),
        name: parse_str(value, "name").unwrap_or_default(),
        parent_folder_id: parse_str(value, "parentFolderId"),
        trn: parse_str(value, "trn"),
        created: parse_str(value, "created"),
        last_modified_at: parse_str(value, "lastModifiedAt"),
        total_number_of_items: parse_i32(value, "totalNumberOfItems"),
    }
}

pub fn parse_video(value: &Value) -> Video {
    let artist = s_get(value, "artist").map(parse_artist_ref);
    let artists = s_get(value, "artists")
        .and_then(|v| v.as_array())
        .map(|a| a.iter().map(parse_artist_ref).collect())
        .unwrap_or_default();
    Video {
        id: parse_i64(value, "id").unwrap_or(0),
        name: parse_str(value, "title").unwrap_or_default(),
        duration: parse_i32(value, "duration"),
        image_id: parse_str(value, "imageId").or_else(|| parse_str(value, "image")),
        artist,
        artists,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn track_fixture() -> Value {
        serde_json::json!({
            "id": 12345,
            "title": "Get Lucky",
            "duration": 248,
            "trackNumber": 8,
            "volumeNumber": 1,
            "isrc": "QM-XXX-...",
            "explicit": false,
            "audioQuality": "LOSSLESS",
            "audioModes": ["STEREO"],
            "artist": {"id": 5, "name": "Daft Punk", "type": "MAIN"},
            "artists": [
                {"id": 5, "name": "Daft Punk", "type": "MAIN"},
                {"id": 6, "name": "Pharrell", "type": "FEATURED"}
            ],
            "album": {"id": 99, "title": "Random Access Memories", "cover": "uuid"}
        })
    }

    #[test]
    fn track_parse_extracts_normalized_fields() {
        let t = parse_track(&track_fixture());
        assert_eq!(t.id, 12345);
        assert_eq!(t.name, "Get Lucky");
        assert_eq!(t.duration, 248);
        assert_eq!(t.track_num, Some(8));
        assert_eq!(t.audio_quality.as_deref(), Some("LOSSLESS"));
        assert_eq!(t.artists.len(), 2);
        assert_eq!(t.artist.as_ref().unwrap().name, "Daft Punk");
        assert_eq!(t.album.as_ref().unwrap().name, "Random Access Memories");
    }

    #[test]
    fn track_parse_handles_missing_fields() {
        let v = serde_json::json!({"id": 5, "title": "X"});
        let t = parse_track(&v);
        assert_eq!(t.id, 5);
        assert_eq!(t.name, "X");
        assert_eq!(t.duration, 0);
        assert!(t.album.is_none());
        assert!(t.artist.is_none());
    }

    #[test]
    fn playlist_parse_synthesizes_trn_from_uuid() {
        let v = serde_json::json!({
            "uuid": "abcd-1234",
            "title": "My Playlist",
            "numberOfTracks": 7,
            "duration": 1800
        });
        let p = parse_playlist(&v);
        assert_eq!(p.id, "abcd-1234");
        assert_eq!(p.trn.as_deref(), Some("trn:playlist:abcd-1234"));
        assert_eq!(p.num_tracks, Some(7));
    }

    #[test]
    fn album_parse_keeps_artists_and_meta() {
        let v = serde_json::json!({
            "id": 42,
            "title": "Some Album",
            "duration": 3600,
            "numberOfTracks": 12,
            "releaseDate": "2024-01-01",
            "cover": "uuid-cover",
            "audioQuality": "HI_RES_LOSSLESS",
            "artist": {"id": 5, "name": "Some Artist"},
            "artists": [{"id": 5, "name": "Some Artist"}]
        });
        let a = parse_album(&v);
        assert_eq!(a.id, 42);
        assert_eq!(a.num_tracks, Some(12));
        assert_eq!(a.audio_quality.as_deref(), Some("HI_RES_LOSSLESS"));
        assert_eq!(a.artist.unwrap().id, 5);
    }
}
