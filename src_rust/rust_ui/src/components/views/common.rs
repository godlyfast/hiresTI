//! Shared building blocks used by every library view.
//!
//! - `ViewLoadState`: idle / loading / loaded / failed lifecycle that
//!   each view's update() drives.
//! - `LibraryViewOutput`: the unified navigation/playback output enum
//!   library views emit. The root forwards into AppInput.
//! - Helpers for the standard placeholder widgets (loading spinner,
//!   error label, empty-state label) so views don't reinvent the
//!   styling.

use std::rc::Rc;

use relm4::gtk::{
    self, prelude::*, Box as GtkBox, FlowBox, FlowBoxChild, Image, Label, ListBoxRow,
    Orientation, Spinner,
};

use rust_tidal_core::api::{Album, ArtistRef, PageCategory, PageItem, Track};

use crate::state::playback::PlaybackSource;

/// Type alias for the click-dispatch callback library/discovery views
/// pass to the page-rendering helpers. Owned + reference-counted so a
/// single callback can be cloned across every card without forcing the
/// caller to construct a fresh closure per card.
pub type CategoryOpener = Rc<dyn Fn(LibraryViewOutput) + 'static>;

#[derive(Debug, Clone, Default)]
pub enum ViewLoadState {
    #[default]
    Idle,
    Loading,
    Loaded,
    Failed(String),
}

impl ViewLoadState {
    /// Convenience predicates kept here for Phase 6 callers (Discovery
    /// pages reuse this enum and want symmetric is-state checks).
    #[allow(dead_code)]
    pub fn is_loaded(&self) -> bool {
        matches!(self, Self::Loaded)
    }
    #[allow(dead_code)]
    pub fn is_loading(&self) -> bool {
        matches!(self, Self::Loading)
    }
}

/// Output every library view emits. Variants name the kind of detail
/// page the user wants to open; the root component is responsible for
/// dispatching to the appropriate detail-view component (Phase 7).
#[derive(Debug, Clone)]
pub enum LibraryViewOutput {
    OpenAlbum { id: String, title: String },
    OpenArtist { id: String, name: String },
    OpenPlaylist { uuid: String, title: String },
    OpenMix { id: String, title: String },
    /// Single-track play with no list context. Used by discovery promo
    /// cards and other one-off Play sources. Clears the queue.
    PlayTrack { track_id: i64 },
    /// Play `tracks[start_index]` and load the rest as the queue so
    /// Next/Prev can navigate. `source` describes where the queue came
    /// from for the Now Playing breadcrumb.
    PlayContext {
        tracks: Vec<Track>,
        start_index: usize,
        source: PlaybackSource,
    },
}

/// Centered spinner for the Loading state. Same shape across every view.
pub fn build_loading_widget() -> GtkBox {
    let wrap = GtkBox::builder()
        .orientation(Orientation::Vertical)
        .vexpand(true)
        .hexpand(true)
        .halign(gtk::Align::Center)
        .valign(gtk::Align::Center)
        .spacing(8)
        .build();
    let spinner = Spinner::builder()
        .spinning(true)
        .width_request(32)
        .height_request(32)
        .build();
    let label = Label::builder()
        .label("Loading…")
        .css_classes(["dim-label"])
        .build();
    wrap.append(&spinner);
    wrap.append(&label);
    wrap
}

/// Centered "no items" placeholder — used when a successful fetch
/// returned an empty list (e.g. user has no favorite albums).
pub fn build_empty_widget(message: &str) -> GtkBox {
    let wrap = GtkBox::builder()
        .orientation(Orientation::Vertical)
        .vexpand(true)
        .hexpand(true)
        .halign(gtk::Align::Center)
        .valign(gtk::Align::Center)
        .spacing(8)
        .build();
    let label = Label::builder()
        .label(message)
        .css_classes(["dim-label"])
        .build();
    wrap.append(&label);
    wrap
}

/// `mm:ss` for the small-screen track-list rows. Negative values clamp to 0.
pub fn format_duration(seconds: i32) -> String {
    let s = seconds.max(0);
    let m = s / 60;
    let r = s % 60;
    format!("{m}:{r:02}")
}

/// Best-effort "primary artist" name picker — uses `artist` if populated,
/// otherwise joins the `artists` list. Returns "" when no artist info is
/// present so callers can render a blank slot rather than "Unknown".
pub fn primary_artist_name(primary: Option<&ArtistRef>, fallback: &[ArtistRef]) -> String {
    if let Some(a) = primary {
        if !a.name.is_empty() {
            return a.name.clone();
        }
    }
    let names: Vec<&str> = fallback
        .iter()
        .map(|a| a.name.as_str())
        .filter(|s| !s.is_empty())
        .collect();
    if names.is_empty() {
        String::new()
    } else {
        names.join(", ")
    }
}

pub fn track_artist_name(track: &Track) -> String {
    primary_artist_name(track.artist.as_ref(), &track.artists)
}

pub fn album_artist_name(album: &Album) -> String {
    primary_artist_name(album.artist.as_ref(), &album.artists)
}

/// Standard track-list row used by Tracks / AlbumDetail / PlaylistDetail
/// / MixDetail / ArtistDetail. Layout is `<index>  <title> <artist>
/// <duration>`. The `on_play` closure runs on row activation; each
/// caller's closure already knows its position in the list, so the
/// callback takes no arguments.
pub fn build_track_row<F>(idx: usize, track: &Track, on_play: F) -> ListBoxRow
where
    F: Fn() + 'static,
{
    let row = ListBoxRow::builder()
        .css_classes(["track-row"])
        .activatable(true)
        .build();
    let body = GtkBox::builder()
        .orientation(Orientation::Horizontal)
        .spacing(12)
        .margin_top(6)
        .margin_bottom(6)
        .margin_start(12)
        .margin_end(12)
        .build();

    let index_label = Label::builder()
        .label(format!("{idx}"))
        .width_chars(4)
        .xalign(1.0)
        .css_classes(["dim-label", "monospace"])
        .build();
    body.append(&index_label);

    let title = if track.name.is_empty() {
        "Unknown"
    } else {
        track.name.as_str()
    };
    let title_label = Label::builder()
        .label(title)
        .ellipsize(gtk::pango::EllipsizeMode::End)
        .hexpand(true)
        .xalign(0.0)
        .build();
    body.append(&title_label);

    let artist_label = Label::builder()
        .label(track_artist_name(track))
        .ellipsize(gtk::pango::EllipsizeMode::End)
        .width_chars(20)
        .xalign(0.0)
        .css_classes(["dim-label"])
        .build();
    body.append(&artist_label);

    let duration_label = Label::builder()
        .label(format_duration(track.duration))
        .width_chars(6)
        .xalign(1.0)
        .css_classes(["dim-label", "monospace"])
        .build();
    body.append(&duration_label);

    row.set_child(Some(&body));

    row.connect_activate(move |_| on_play());
    row
}

/// Render a single `/pages/*` category as a section: title + subtitle +
/// horizontal flow of item cards. Each card click dispatches via the
/// shared `on_open` callback. Empty categories render a single
/// "(no items)" label so the layout doesn't collapse.
pub fn build_page_category_section(
    category: &PageCategory,
    on_open: CategoryOpener,
) -> GtkBox {
    let section = GtkBox::builder()
        .orientation(Orientation::Vertical)
        .spacing(8)
        .build();

    if let Some(title) = category.title.as_deref().filter(|s| !s.is_empty()) {
        let title_label = Label::builder()
            .label(title)
            .xalign(0.0)
            .css_classes(["title-3"])
            .build();
        section.append(&title_label);
    }
    if let Some(subtitle) = category.subtitle.as_deref().filter(|s| !s.is_empty()) {
        let sub = Label::builder()
            .label(subtitle)
            .xalign(0.0)
            .css_classes(["dim-label", "caption"])
            .build();
        section.append(&sub);
    }

    if category.items.is_empty() {
        let empty = Label::builder()
            .label("(no items)")
            .xalign(0.0)
            .css_classes(["dim-label"])
            .build();
        section.append(&empty);
        return section;
    }

    let flow = FlowBox::builder()
        .selection_mode(gtk::SelectionMode::None)
        .max_children_per_line(8)
        .min_children_per_line(2)
        .row_spacing(12)
        .column_spacing(12)
        .homogeneous(true)
        .build();
    for item in &category.items {
        if let Some(card) = build_page_item_card(item, on_open.clone()) {
            let child = FlowBoxChild::builder().child(&card).build();
            flow.append(&child);
        }
    }
    section.append(&flow);
    section
}

/// Render a single `PageItem` as a clickable card. Returns `None` for
/// item types we intentionally skip in discovery surfaces (videos in
/// Phase 7). The cover slot is a placeholder icon — Phase 7-I swaps it
/// for the real resources.tidal.com fetch.
pub fn build_page_item_card(item: &PageItem, on_open: CategoryOpener) -> Option<GtkBox> {
    let card = GtkBox::builder()
        .orientation(Orientation::Vertical)
        .spacing(4)
        .css_classes(["album-card"])
        .build();

    let cover = Image::builder()
        .icon_name("audio-x-generic-symbolic")
        .pixel_size(160)
        .css_classes(["album-cover-img"])
        .build();
    card.append(&cover);

    let (primary, secondary, click_output): (String, String, Option<LibraryViewOutput>) = match item {
        PageItem::Track(t) => {
            let title = if t.name.is_empty() {
                "Unknown".to_string()
            } else {
                t.name.clone()
            };
            let artist = primary_artist_name(t.artist.as_ref(), &t.artists);
            (
                title,
                artist,
                Some(LibraryViewOutput::PlayTrack { track_id: t.id }),
            )
        }
        PageItem::Album(a) => {
            let title = if a.name.is_empty() {
                "Unknown".to_string()
            } else {
                a.name.clone()
            };
            let artist = album_artist_name(a);
            (
                title.clone(),
                artist,
                Some(LibraryViewOutput::OpenAlbum {
                    id: a.id.to_string(),
                    title,
                }),
            )
        }
        PageItem::Artist(a) => {
            let name = if a.name.is_empty() {
                "Unknown".to_string()
            } else {
                a.name.clone()
            };
            (
                name.clone(),
                String::new(),
                Some(LibraryViewOutput::OpenArtist {
                    id: a.id.to_string(),
                    name,
                }),
            )
        }
        PageItem::Playlist(p) => {
            let title = if p.name.is_empty() {
                "Untitled playlist".to_string()
            } else {
                p.name.clone()
            };
            let count = p
                .num_tracks
                .filter(|n| *n > 0)
                .map(|n| format!("{n} tracks"))
                .unwrap_or_default();
            (
                title.clone(),
                count,
                Some(LibraryViewOutput::OpenPlaylist {
                    uuid: p.id.clone(),
                    title,
                }),
            )
        }
        PageItem::Mix(m) => {
            let title = if m.title.is_empty() {
                "Mix".to_string()
            } else {
                m.title.clone()
            };
            let sub = m.sub_title.clone().unwrap_or_default();
            (
                title.clone(),
                sub,
                Some(LibraryViewOutput::OpenMix {
                    id: m.id.clone(),
                    title,
                }),
            )
        }
        PageItem::Video(_) => return None,
        PageItem::Card(c) => {
            let title = c
                .header
                .clone()
                .or_else(|| c.short_header.clone())
                .or_else(|| c.title.clone())
                .unwrap_or_else(|| "—".to_string());
            let sub = c
                .short_sub_header
                .clone()
                .or_else(|| c.sub_title.clone())
                .unwrap_or_default();
            // PageLink/promo cards have no typed action yet — Phase 7-G
            // adds a link router.
            (title, sub, None)
        }
    };

    let title_label = Label::builder()
        .label(&primary)
        .ellipsize(gtk::pango::EllipsizeMode::End)
        .max_width_chars(18)
        .xalign(0.0)
        .css_classes(["heading"])
        .build();
    card.append(&title_label);

    if !secondary.is_empty() {
        let sub_label = Label::builder()
            .label(&secondary)
            .ellipsize(gtk::pango::EllipsizeMode::End)
            .max_width_chars(18)
            .xalign(0.0)
            .css_classes(["dim-label", "caption"])
            .build();
        card.append(&sub_label);
    }

    if let Some(out) = click_output {
        let click = gtk::GestureClick::new();
        let cb = on_open.clone();
        click.connect_pressed(move |_, n_press, _, _| {
            if n_press == 1 {
                cb(out.clone());
            }
        });
        card.add_controller(click);
    }

    Some(card)
}

/// Centered error message + (later) retry button.
pub fn build_error_widget(message: &str) -> GtkBox {
    let wrap = GtkBox::builder()
        .orientation(Orientation::Vertical)
        .vexpand(true)
        .hexpand(true)
        .halign(gtk::Align::Center)
        .valign(gtk::Align::Center)
        .spacing(8)
        .margin_start(24)
        .margin_end(24)
        .build();
    let label = Label::builder()
        .label(format!("Couldn't load:\n{message}"))
        .wrap(true)
        .justify(gtk::Justification::Center)
        .css_classes(["error"])
        .build();
    wrap.append(&label);
    wrap
}
