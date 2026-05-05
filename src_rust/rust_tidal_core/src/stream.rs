//! Stream URL + manifest decoding (Phase 5).
//!
//! TIDAL's modern playback flow:
//!   GET /v1/tracks/{id}/playbackinfopostpaywall
//!     ?playbackmode=STREAM&audioquality=<Q>&assetpresentation=FULL
//! returns a JSON envelope whose `manifest` field is base64-encoded. The
//! mime type tells us how to decode it:
//!   * `application/vnd.tidal.bts`   — base64 → JSON {urls, codecs, mimeType,
//!                                                    encryptionType, keyId?}
//!   * `application/dash+xml`        — base64 → MPD XML (DASH manifest)
//!
//! We hand both shapes back to Python in a single decoded struct so the
//! UI code doesn't need a second round-trip just to read `urls[0]`.
//!
//! There's also a legacy `urlpostpaywall` endpoint that returns a plain
//! `{ "urls": [...] }` and ignores manifests — we expose it as a fallback
//! so callers can switch when the modern endpoint downgrades to AAC.

use base64::{engine::general_purpose::STANDARD as B64, Engine};
use serde::Serialize;
use serde_json::Value;
use std::collections::BTreeMap;

use crate::endpoints::ok_or_status;
use crate::error::{RtcError, RtcResult};
use crate::request::{ParamValue, RequestArgs};
use crate::session::Session;

const BTS_MIME_PREFIX: &str = "application/vnd.tidal.bts";
const MPD_MIME: &str = "application/dash+xml";

#[derive(Debug, Clone, Serialize, Default)]
pub struct StreamInfo {
    pub track_id: i64,
    pub audio_mode: String,
    pub audio_quality: String,
    pub manifest_mime_type: String,
    pub manifest_hash: Option<String>,
    /// Raw base64 manifest; preserved so callers can re-decode if they want
    /// to keep parity with tidalapi's `Stream.manifest`.
    pub manifest: String,
    pub asset_presentation: String,
    pub bit_depth: i32,
    pub sample_rate: i32,
    pub album_replay_gain: f64,
    pub album_peak_amplitude: f64,
    pub track_replay_gain: f64,
    pub track_peak_amplitude: f64,
    pub is_bts: bool,
    pub is_mpd: bool,
    /// Decoded manifest as UTF-8. JSON for BTS, XML for MPD.
    pub manifest_data: String,
    /// Stream URLs. For BTS this comes from the inner JSON's `urls`;
    /// for MPD it stays empty here — callers feed `manifest_data` to a
    /// DASH-aware native transport which handles segment fetching.
    pub urls: Vec<String>,
    pub codecs: Option<String>,
    pub mime_type: Option<String>,
    pub encryption_type: Option<String>,
    pub encryption_key: Option<String>,
}

/// Fetch + decode the modern `playbackinfopostpaywall` envelope.
pub fn fetch_stream(
    session: &Session,
    track_id: i64,
    audio_quality: &str,
    playback_mode: Option<&str>,
    asset_presentation: Option<&str>,
) -> RtcResult<StreamInfo> {
    let mut params = BTreeMap::new();
    params.insert(
        "playbackmode".to_string(),
        ParamValue::Str(playback_mode.unwrap_or("STREAM").to_string()),
    );
    params.insert(
        "audioquality".to_string(),
        ParamValue::Str(audio_quality.to_string()),
    );
    params.insert(
        "assetpresentation".to_string(),
        ParamValue::Str(asset_presentation.unwrap_or("FULL").to_string()),
    );
    let resp = session.request(RequestArgs {
        method: "GET".into(),
        path: format!("tracks/{}/playbackinfopostpaywall", track_id),
        base_url: None,
        params: Some(params),
        headers: None,
        json_body: None,
        form_body: false,
    })?;
    let body = ok_or_status(resp)?;
    decode_stream_envelope(body)
}

/// Legacy URL fetch (`urlpostpaywall`). Returns the first URL or an error.
/// Tidalapi gates this behind a `is_pkce` check because PKCE-flow tokens
/// can't access this endpoint at hi-res quality; we mirror that constraint
/// so callers don't waste a 4xx.
pub fn fetch_legacy_url(
    session: &Session,
    track_id: i64,
    audio_quality: &str,
) -> RtcResult<String> {
    if session.is_pkce() {
        return Err(RtcError::Client {
            status: 0,
            body: format!(
                "legacy urlpostpaywall is not available with PKCE auth (quality={})",
                audio_quality
            ),
        });
    }
    let mut params = BTreeMap::new();
    params.insert(
        "urlusagemode".to_string(),
        ParamValue::Str("STREAM".into()),
    );
    params.insert(
        "audioquality".to_string(),
        ParamValue::Str(audio_quality.to_string()),
    );
    params.insert(
        "assetpresentation".to_string(),
        ParamValue::Str("FULL".into()),
    );
    let resp = session.request(RequestArgs {
        method: "GET".into(),
        path: format!("tracks/{}/urlpostpaywall", track_id),
        base_url: None,
        params: Some(params),
        headers: None,
        json_body: None,
        form_body: false,
    })?;
    let body = ok_or_status(resp)?;
    body.get("urls")
        .and_then(|v| v.as_array())
        .and_then(|arr| arr.first())
        .and_then(|v| v.as_str())
        .map(str::to_string)
        .ok_or_else(|| RtcError::Server {
            status: 200,
            body: "urlpostpaywall returned no urls".into(),
        })
}

fn decode_stream_envelope(body: Value) -> RtcResult<StreamInfo> {
    let track_id = body.get("trackId").and_then(|v| v.as_i64()).unwrap_or(0);
    let audio_mode = body
        .get("audioMode")
        .and_then(|v| v.as_str())
        .unwrap_or_default()
        .to_string();
    let audio_quality = body
        .get("audioQuality")
        .and_then(|v| v.as_str())
        .unwrap_or_default()
        .to_string();
    let manifest_mime_type = body
        .get("manifestMimeType")
        .and_then(|v| v.as_str())
        .unwrap_or_default()
        .to_string();
    let manifest_hash = body
        .get("manifestHash")
        .and_then(|v| v.as_str())
        .map(str::to_string);
    let manifest = body
        .get("manifest")
        .and_then(|v| v.as_str())
        .unwrap_or_default()
        .to_string();
    let asset_presentation = body
        .get("assetPresentation")
        .and_then(|v| v.as_str())
        .unwrap_or("FULL")
        .to_string();
    let bit_depth = body
        .get("bitDepth")
        .and_then(|v| v.as_i64())
        .map(|n| n as i32)
        .unwrap_or(16);
    let sample_rate = body
        .get("sampleRate")
        .and_then(|v| v.as_i64())
        .map(|n| n as i32)
        .unwrap_or(44100);
    let album_replay_gain = body
        .get("albumReplayGain")
        .and_then(|v| v.as_f64())
        .unwrap_or(1.0);
    let album_peak_amplitude = body
        .get("albumPeakAmplitude")
        .and_then(|v| v.as_f64())
        .unwrap_or(1.0);
    let track_replay_gain = body
        .get("trackReplayGain")
        .and_then(|v| v.as_f64())
        .unwrap_or(1.0);
    let track_peak_amplitude = body
        .get("trackPeakAmplitude")
        .and_then(|v| v.as_f64())
        .unwrap_or(1.0);

    let is_bts = manifest_mime_type.contains(BTS_MIME_PREFIX);
    let is_mpd = manifest_mime_type.contains(MPD_MIME);

    let manifest_data = if manifest.is_empty() {
        String::new()
    } else {
        let raw = B64.decode(manifest.as_bytes()).map_err(|e| RtcError::Server {
            status: 200,
            body: format!("manifest base64 decode failed: {}", e),
        })?;
        String::from_utf8(raw).map_err(|e| RtcError::Server {
            status: 200,
            body: format!("manifest is not valid utf-8: {}", e),
        })?
    };

    let mut info = StreamInfo {
        track_id,
        audio_mode,
        audio_quality,
        manifest_mime_type,
        manifest_hash,
        manifest,
        asset_presentation,
        bit_depth,
        sample_rate,
        album_replay_gain,
        album_peak_amplitude,
        track_replay_gain,
        track_peak_amplitude,
        is_bts,
        is_mpd,
        manifest_data,
        ..Default::default()
    };

    if is_bts && !info.manifest_data.is_empty() {
        // BTS payload: { mimeType, codecs, encryptionType, keyId?, urls: [...] }
        match serde_json::from_str::<Value>(&info.manifest_data) {
            Ok(inner) => {
                info.urls = inner
                    .get("urls")
                    .and_then(|v| v.as_array())
                    .map(|arr| {
                        arr.iter()
                            .filter_map(|v| v.as_str().map(str::to_string))
                            .collect()
                    })
                    .unwrap_or_default();
                info.codecs = inner
                    .get("codecs")
                    .and_then(|v| v.as_str())
                    .map(|s| s.to_ascii_uppercase().split('.').next().unwrap_or(s).to_string());
                info.mime_type = inner
                    .get("mimeType")
                    .and_then(|v| v.as_str())
                    .map(str::to_string);
                info.encryption_type = inner
                    .get("encryptionType")
                    .and_then(|v| v.as_str())
                    .map(str::to_string);
                info.encryption_key = inner
                    .get("keyId")
                    .and_then(|v| v.as_str())
                    .map(str::to_string);
            }
            Err(e) => {
                return Err(RtcError::Server {
                    status: 200,
                    body: format!("BTS manifest is not JSON: {}", e),
                })
            }
        }
    }
    // MPD parsing stays in the native transport / DASH layer; we only
    // expose the decoded XML (manifest_data) so Python can either write it
    // to a temp file or hand it directly to a DASH parser.

    Ok(info)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn decode_bts_envelope_extracts_url_and_codec() {
        let inner = json!({
            "mimeType": "audio/mpeg",
            "codecs": "mp4a.40.5",
            "encryptionType": "NONE",
            "urls": ["https://stream.example/track-1.m4a"],
        });
        let inner_b64 = B64.encode(inner.to_string());
        let envelope = json!({
            "trackId": 7,
            "audioMode": "STEREO",
            "audioQuality": "LOW",
            "manifestMimeType": "application/vnd.tidal.bts",
            "manifestHash": "abc",
            "manifest": inner_b64,
            "bitDepth": 16,
            "sampleRate": 44100,
        });
        let info = decode_stream_envelope(envelope).unwrap();
        assert!(info.is_bts);
        assert!(!info.is_mpd);
        assert_eq!(info.urls, vec!["https://stream.example/track-1.m4a"]);
        assert_eq!(info.codecs.as_deref(), Some("MP4A"));
        assert_eq!(info.encryption_type.as_deref(), Some("NONE"));
    }

    #[test]
    fn decode_mpd_envelope_returns_xml() {
        let xml = r#"<MPD xmlns="urn:mpeg:dash:schema:mpd:2011"></MPD>"#;
        let envelope = json!({
            "trackId": 9,
            "audioMode": "STEREO",
            "audioQuality": "HI_RES_LOSSLESS",
            "manifestMimeType": "application/dash+xml",
            "manifest": B64.encode(xml),
            "bitDepth": 24,
            "sampleRate": 96000,
        });
        let info = decode_stream_envelope(envelope).unwrap();
        assert!(info.is_mpd);
        assert!(!info.is_bts);
        assert_eq!(info.manifest_data, xml);
        assert!(info.urls.is_empty());
        assert_eq!(info.bit_depth, 24);
        assert_eq!(info.sample_rate, 96000);
    }

    #[test]
    fn missing_manifest_returns_empty_data_without_panic() {
        let envelope = json!({
            "trackId": 1,
            "audioQuality": "LOW",
            "manifestMimeType": "application/vnd.tidal.bts",
        });
        let info = decode_stream_envelope(envelope).unwrap();
        assert_eq!(info.manifest_data, "");
        assert!(info.urls.is_empty());
    }
}
