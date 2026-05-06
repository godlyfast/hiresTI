//! Per-NavTarget view components. Each view fetches its own data
//! through `TidalSessionService` (worker threads) and renders into a
//! GTK widget the content stack adopts. They share a small set of
//! state primitives (`ViewLoadState`, the `LibraryViewOutput`
//! navigation enum) defined in `common`.

pub mod albums;
pub mod artists;
pub mod common;
pub mod history;
pub mod mixes;
pub mod playlists;
pub mod tracks;
