# Code Structure

This document describes the project structure of hiresTI for developers and contributors.

## Overview

hiresTI is a native Linux desktop client for TIDAL written entirely in Rust. The shipped binary statically links a four-crate workspace:

- `rust_ui` — gtk4-rs / libadwaita / Relm4 UI, the binary entry point
- `rust_audio_core` — direct ALSA + USB Rawlink V2 transports, integrated DSP graph
- `rust_viz_core` — FFT / spectrum mapping, viz state machine, render-mode helpers
- `rust_tidal_core` — TIDAL REST + PKCE / device-code OAuth client

No Python runtime, no GStreamer, no third-party SDK in the loop.

## Directory Structure

```
hiresTI/
├── src_rust/
│   ├── Cargo.toml          # Workspace root
│   ├── rust_ui/            # GTK4 binary `hiresti`
│   ├── rust_audio_core/    # Audio engine (ALSA + USB Rawlink V2)
│   ├── rust_viz_core/      # FFT + viz helpers
│   └── rust_tidal_core/    # TIDAL REST client
├── aur/                    # AUR PKGBUILD
├── icons/                  # Application icons
├── screenshots/
├── package.sh              # Local + Docker package builds
├── Dockerfile.build        # Multi-distro Docker builder
├── version.txt
├── README.md / README_CN.md
├── CHANGELOG.md
└── audio-optimization-guide.md
```

## `rust_ui/` — UI binary

Entry point at `src/main.rs`; the Relm4 component graph is rooted in `src/app.rs::AppController`. Module layout:

| Path                                | Description                                                            |
|-------------------------------------|------------------------------------------------------------------------|
| `src/app.rs`                        | Root SimpleComponent. Owns the engine + child controllers.             |
| `src/messages.rs`                   | `AppInput` enum — every UI event flows through one variant.            |
| `src/model.rs`                      | `AppModel` — settings + auth + playback + queue state.                 |
| `src/settings.rs`                   | Settings round-trip with forward-compatible `extra: serde_json::Map`.  |
| `src/state/`                        | Auth + playback + queue substates.                                     |
| `src/components/`                   | One module per Relm4 component.                                        |
| `src/components/views/`             | Library / discovery / detail pages.                                    |
| `src/services/`                     | Long-lived services (see below).                                       |

### `src/components/` highlights

| Module                              | Role                                                                   |
|-------------------------------------|------------------------------------------------------------------------|
| `header.rs`                         | App-bar with search, login, settings, back button.                     |
| `sidebar.rs`                        | Discover / library / recent navigation.                                |
| `content_stack.rs`                  | Page swap container with detail-page overlay.                          |
| `mini_player.rs`                    | Bottom transport bar with seek + cover.                                |
| `visualizer.rs`                     | Bars / line / spiral cairo renderer.                                   |
| `dr_meter.rs`                       | LUFS / DR readout strip.                                               |
| `lyric_strip.rs`                    | Single-line synchronized lyric overlay.                                |
| `settings_dialog.rs`, `dsp_preset_dialog.rs`, `signal_path_window.rs`, `diagnostics_dialog.rs`, `about_dialog.rs` | Modal dialogs.                                                |
| `login_dialog.rs`, `pkce_login_dialog.rs` | OAuth device-code + PKCE flows.                                  |

### `src/services/`

Long-lived workers that the UI consumes via cheap-clone handles. Most run on dedicated threads with an `async-io` or `tiny_http` event loop.

| Module                | Thread model                                          | Role                                                                   |
|-----------------------|-------------------------------------------------------|------------------------------------------------------------------------|
| `tidal_session.rs`    | Worker pool via `spawn_blocking`                      | TIDAL REST + auth (`rust_tidal_core` Session).                         |
| `covers.rs`           | Per-fetch worker thread                               | Album-art fetch + on-disk cache.                                       |
| `tray.rs`             | Dedicated `ksni` thread                               | Linux StatusNotifierItem.                                              |
| `mpris.rs`            | Dedicated async-io thread (`!Send` `Player`)          | `org.mpris.MediaPlayer2.hiresti` D-Bus interface.                      |
| `scrobbler.rs`        | Per-submit worker thread                              | Last.fm + ListenBrainz scrobbling.                                     |
| `lyrics.rs`           | Worker via `spawn_blocking`                           | LRC fetch + parse + LRU cache.                                         |
| `alsa_reserve.rs`     | Dedicated async-io thread                             | `org.freedesktop.ReserveDevice1` for ALSA exclusive.                   |
| `remote_api.rs`       | Dedicated `tiny_http` thread                          | HTTP / JSON-RPC remote control.                                        |
| `dsp_preset.rs`       | Synchronous (called from UI)                          | DSP preset import / export.                                            |

## `rust_audio_core/`

Audio engine: integrates ALSA-direct and USB Rawlink V2 transports, the DSP graph (PEQ, convolution, tube/tape color, stereo widening, limiter, resampler), LUFS / spectrum DSP nodes, and TIDAL stream segmenting (isahc-backed, HTTP/2 with parallel multi-segment prefetch). Exposes both an `Engine` Rust API (consumed by `rust_ui`) and a C FFI surface (used historically by Python).

## `rust_viz_core/`

FFT + viz helpers: `VizStateEngine` (EMA + peak hold + bass extraction), `map_log_spectrum` / `map_linear_spectrum` for FFT-to-bar mapping, and point-list builders for the bars / line / spiral / ring / fall / dots viz modes. Dual-link `cdylib + rlib`; consumed by `rust_ui` via the rlib.

## `rust_tidal_core/`

Pure-Rust TIDAL client: REST endpoints, OAuth device-code + PKCE flows, persisted token round-trip, models (`Track`, `Album`, `Artist`, `Playlist`, `Mix`, …), tail surfaces (`Lyrics`, `Bio`, `Page`). Dual-link `cdylib + rlib`.

## Building

Single workspace command:

```bash
cargo build --manifest-path src_rust/Cargo.toml --release --bin hiresti
```

Produces `src_rust/target/release/hiresti` (~16 MB, fully self-contained).

## Architecture Notes

1. **Single-process, single-thread main loop.** GTK widgets are `!Send`, so the GTK main thread owns the `AppModel`, the audio `Engine`, and every Relm4 controller. Background work (HTTP, D-Bus, scrobbling, lyrics fetch, MPRIS state push) runs on dedicated worker threads and posts results back through `relm4::Sender<AppInput>` (which is `Send + Clone`).

2. **State machine via `AppInput`.** Every user action — sidebar nav, transport buttons, dialog submissions, tray menu activations, MPRIS method calls, remote-API RPC commands — produces an `AppInput` variant. The root `update` arm matches exhaustively, so the compiler catches missing wiring.

3. **Settings carry an `extra: serde_json::Map`.** Unknown fields round-trip unchanged, so a newer or older build can read each other's config without losing user state.

4. **Cover-art / lyrics / token fetches are guarded by `request_id`.** A monotonic play counter (`AppController::play_request_counter`) gates stale resolves so a fast-skip doesn't push the previous track's cover or lyrics into the mini-player.

## Contributing

When adding new functionality:

1. Place new code in the appropriate module directory.
2. Add an `AppInput` variant if it produces state changes.
3. Update this document if the structure changes.
