//! Per-NavTarget view components. Each view fetches its own data
//! through `TidalSessionService` (worker threads) and renders into a
//! GTK widget the content stack adopts. They share a small set of
//! state primitives (`ViewLoadState`, the `LibraryViewOutput`
//! navigation enum) defined in `common`.

pub mod album_detail;
pub mod albums;
pub mod artist_detail;
pub mod artists;
pub mod common;
pub mod discovery;
pub mod history;
pub mod mix_detail;
pub mod mixes;
pub mod playlist_detail;
pub mod playlists;
pub mod tracks;
