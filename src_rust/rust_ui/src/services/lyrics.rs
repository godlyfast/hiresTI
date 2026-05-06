//! Lyrics fetching + LRC parsing. Mirrors the Python
//! `services.lyrics.LyricsManager` parser so existing user-facing
//! behavior stays identical: synced lines via `[mm:ss.cc]`,
//! karaoke word timings via inline `<mm:ss.cc>` tags, and a
//! best-guess "current line" based on the last timestamp <= now.
//!
//! The service itself is just a parser + LRU cache; the actual
//! fetch goes through `TidalSessionService::track_lyrics_blocking`
//! on a worker thread (via `spawn_blocking`).

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use crate::services::tidal_session::{spawn_blocking, TidalSessionService};

/// One synchronized lyric line. `time` is in seconds.
#[derive(Debug, Clone)]
pub struct LyricLine {
    pub time: f64,
    pub text: String,
}

/// Parsed lyrics for a single track.
#[derive(Debug, Clone, Default)]
pub struct ParsedLyrics {
    /// Sorted by time, ascending. Empty for tracks with only static
    /// (unsynced) text.
    pub synced: Vec<LyricLine>,
    /// Plain unsynced text. Populated only when the source had no
    /// timestamps; preserved verbatim so the UI can show the full
    /// block.
    pub plain: Option<String>,
    pub right_to_left: bool,
}

impl ParsedLyrics {
    /// Returns the line whose timestamp is the largest one
    /// `<= position_secs`, or `None` when we're before the first
    /// timestamp (typical for the lead-in to the first verse).
    pub fn line_at(&self, position_secs: f64) -> Option<&LyricLine> {
        if self.synced.is_empty() {
            return None;
        }
        // Linear scan — track lyric counts top out around ~80 lines,
        // not worth a binary search and the cache hit path skips the
        // walk entirely (active line index is memoized in the UI).
        let mut last: Option<&LyricLine> = None;
        for ln in &self.synced {
            if ln.time <= position_secs {
                last = Some(ln);
            } else {
                break;
            }
        }
        last
    }
}

#[derive(Debug, Default)]
struct State {
    cache: HashMap<i64, Arc<ParsedLyrics>>,
}

#[derive(Debug, Clone, Default)]
pub struct LyricsService {
    state: Arc<Mutex<State>>,
}

impl LyricsService {
    pub fn new() -> Self {
        Self::default()
    }

    /// Look up cached lyrics for a track without triggering a fetch.
    pub fn cached(&self, track_id: i64) -> Option<Arc<ParsedLyrics>> {
        self.state
            .lock()
            .ok()
            .and_then(|s| s.cache.get(&track_id).cloned())
    }

    /// Fetch lyrics for `track_id` on a worker thread. The callback
    /// fires on the GTK main loop after the parsed result is cached.
    /// `request_id` is opaque — pass the `play_request_counter` so
    /// stale fetches can be dropped at the call site.
    pub fn fetch_async(
        &self,
        session: TidalSessionService,
        track_id: i64,
        request_id: u64,
        on_done: impl FnOnce(u64, Option<Arc<ParsedLyrics>>) + Send + 'static,
    ) {
        if let Some(hit) = self.cached(track_id) {
            on_done(request_id, Some(hit));
            return;
        }
        let store = Arc::clone(&self.state);
        spawn_blocking(
            move || session.track_lyrics_blocking(track_id),
            move |result| match result {
                Ok(raw) => {
                    let parsed = parse_tidal_lyrics(raw);
                    let arc = Arc::new(parsed);
                    if let Ok(mut s) = store.lock() {
                        s.cache.insert(track_id, Arc::clone(&arc));
                    }
                    on_done(request_id, Some(arc));
                }
                Err(e) => {
                    tracing::debug!(track_id, error = %e, "lyrics fetch failed");
                    on_done(request_id, None);
                }
            },
        );
    }
}

fn parse_tidal_lyrics(raw: rust_tidal_core::api::Lyrics) -> ParsedLyrics {
    let rtl = raw.right_to_left;
    if let Some(subs) = raw.subtitles.as_deref() {
        let mut p = parse_lrc(subs);
        p.right_to_left = rtl;
        if p.synced.is_empty() && p.plain.is_none() {
            // Subtitles present but unparseable — fall through to text.
        } else {
            return p;
        }
    }
    let plain = raw.text.filter(|s| !s.is_empty());
    ParsedLyrics {
        synced: Vec::new(),
        plain,
        right_to_left: rtl,
    }
}

/// Parse an LRC body. Supports the standard `[mm:ss.cc]` and
/// `[mm:ss.ccc]` time tags. Inline word-timestamps (`<mm:ss.cc>`)
/// are stripped so the displayed line text is plain — the karaoke
/// view in Python wasn't ever wired to the GTK UI either.
fn parse_lrc(body: &str) -> ParsedLyrics {
    let mut synced: Vec<LyricLine> = Vec::new();
    let mut leading_plain: Vec<String> = Vec::new();
    let mut had_any_tag = false;

    for line in body.split('\n') {
        let line = line.trim_end_matches('\r');
        if line.is_empty() {
            continue;
        }
        let mut rest = line;
        let mut times: Vec<f64> = Vec::new();
        // Multiple `[..]` tags can stack on one line — each represents
        // the same lyric appearing at multiple timestamps.
        loop {
            let stripped = rest.trim_start();
            if !stripped.starts_with('[') {
                rest = stripped;
                break;
            }
            let Some(close) = stripped.find(']') else {
                rest = stripped;
                break;
            };
            let inner = &stripped[1..close];
            if let Some(secs) = parse_lrc_time(inner) {
                times.push(secs);
                had_any_tag = true;
            } else {
                // Metadata tag like [ti:Title], [ar:Artist] — drop it
                // silently; spec lets us ignore unknown tags.
            }
            rest = stripped[close + 1..].trim_start();
        }
        let text = strip_word_timestamps(rest);
        if times.is_empty() {
            if !text.is_empty() {
                leading_plain.push(text);
            }
            continue;
        }
        for t in times {
            synced.push(LyricLine {
                time: t,
                text: text.clone(),
            });
        }
    }

    synced.sort_by(|a, b| a.time.partial_cmp(&b.time).unwrap_or(std::cmp::Ordering::Equal));

    let plain = if had_any_tag {
        None
    } else if leading_plain.is_empty() {
        None
    } else {
        Some(leading_plain.join("\n"))
    };

    ParsedLyrics {
        synced,
        plain,
        right_to_left: false,
    }
}

fn parse_lrc_time(s: &str) -> Option<f64> {
    // mm:ss.cc / mm:ss.ccc / mm:ss
    let (m, rest) = s.split_once(':')?;
    let minutes: f64 = m.trim().parse().ok()?;
    let seconds: f64 = rest.trim().parse().ok()?;
    if !minutes.is_finite() || !seconds.is_finite() {
        return None;
    }
    Some(minutes * 60.0 + seconds)
}

fn strip_word_timestamps(s: &str) -> String {
    // <mm:ss.cc> word — keep the word, drop the bracketed stamp.
    let mut out = String::with_capacity(s.len());
    let mut chars = s.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '<' {
            // Drop until '>' or end-of-string.
            for nc in chars.by_ref() {
                if nc == '>' {
                    break;
                }
            }
        } else {
            out.push(c);
        }
    }
    out.trim().to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_basic_lrc() {
        let body = "[00:01.50]Hello\n[00:03.00]World";
        let p = parse_lrc(body);
        assert_eq!(p.synced.len(), 2);
        assert!((p.synced[0].time - 1.5).abs() < 1e-6);
        assert_eq!(p.synced[0].text, "Hello");
        assert_eq!(p.synced[1].text, "World");
        assert!(p.plain.is_none());
    }

    #[test]
    fn strips_inline_word_timestamps() {
        let body = "[00:10.20]<00:10.20>Hello <00:10.60>world";
        let p = parse_lrc(body);
        assert_eq!(p.synced.len(), 1);
        assert_eq!(p.synced[0].text, "Hello world");
    }

    #[test]
    fn line_at_returns_last_le() {
        let body = "[00:01.00]a\n[00:05.00]b\n[00:10.00]c";
        let p = parse_lrc(body);
        assert_eq!(p.line_at(0.5).map(|l| l.text.as_str()), None);
        assert_eq!(p.line_at(1.0).map(|l| l.text.as_str()), Some("a"));
        assert_eq!(p.line_at(7.0).map(|l| l.text.as_str()), Some("b"));
        assert_eq!(p.line_at(20.0).map(|l| l.text.as_str()), Some("c"));
    }
}
