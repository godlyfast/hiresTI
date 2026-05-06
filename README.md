# hiresTI Music Player

![Rust](https://img.shields.io/badge/Rust-1.80%2B-orange?logo=rust)
![GTK4](https://img.shields.io/badge/UI-GTK4%20%2B%20Libadwaita-green)
![License](https://img.shields.io/badge/License-GPL--3.0-purple)

`hiresTI` is a native Linux TIDAL client built for audiophiles, combining high-fidelity playback, rock-solid stability, and a modern GTK4 / Libadwaita user experience.

> [!IMPORTANT]
> **Re-login required for FLAC / Hi-Res quality.**
> The legacy OAuth (device-code) login that previous versions used is now capped by TIDAL at 320 kbps AAC. The **PKCE** login flow restores access to full FLAC CD-quality and Hi-Res Lossless streams.
>
> If you're still on the old OAuth session, open **Account**, sign out, and sign back in — the login dialog will use PKCE automatically and your library will resume at the higher tiers.

> [!NOTE]
> **1.10 is a full rewrite to native Rust.** The Python UI layer is gone; the entire app is now a single self-contained Rust binary built on gtk4-rs / libadwaita-rs / Relm4. No Python runtime is required.

## Highlights

- High-performance Rust audio engine with bit-perfect playback flow + optional exclusive output controls
- Built-in USB Rawlink driver enables direct USB passthrough, bypassing OS drivers and mixing for purer sound
- High-flexibility DSP workspace with reorderable processing, PEQ, convolution, tube/tape color, stereo widening, limiter, and resampler
- TIDAL PKCE login (recommended for FLAC / Hi-Res) with legacy OAuth fallback, account-scoped library access
- TIDAL Max Hi-Res Lossless streaming up to 24-bit / 192 kHz
- Built-in queue, click-to-seek synced lyrics, and three visualizer modes (bars / line / spiral)
- Live LUFS / dynamic-range readout (momentary, short-term, integrated, LRA, 4 s DR)
- MPRIS support (`org.mpris.MediaPlayer2.hiresti`) for desktop media controls + media-key shortcuts
- Last.fm and ListenBrainz scrobbling
- Linux system tray (StatusNotifierItem)
- ALSA exclusive `org.freedesktop.ReserveDevice1` reservation so PipeWire / WirePlumber yield the device on demand
- Built-in remote control with HTTP JSON-RPC

Audio Optimization Guide: [audio-optimization-guide.md](audio-optimization-guide.md)

## Screenshots
### Main Window
![Main Window](screenshots/1.8.0-2.png)
![Main Window](screenshots/1.4.5-2.png)
![Main Window](screenshots/1.6.0-1.png)
![Main Window](screenshots/1.8.0-1.png)
![Main Window](screenshots/1.6.5-1.png)


### Mini Mode
<img src="screenshots/1.0.4-5.png" width="400">


## Tech Stack

- Rust 1.80+ with edition 2021
- gtk4-rs 0.11 + libadwaita-rs 0.9 + Relm4 0.11 (native GTK4 / Libadwaita bindings)
- Audio engine: `rust_audio_core` — direct ALSA + USB Rawlink V2 transports, integrated DSP graph, isahc/HTTP-2 segment streaming
- Visualizer / DSP helpers: `rust_viz_core`
- TIDAL integration: `rust_tidal_core` — direct REST + PKCE / device-code OAuth, no third-party SDK
- D-Bus: zbus 5 (MPRIS, ALSA reserve, tray)

## Runtime Requirements

The shipped binary is fully self-contained. Required system libraries on the target machine:

- gtk4
- libadwaita
- pipewire (or pulseaudio)
- alsa-lib
- libusb-1.0
- openssl

No Python or GStreamer runtime is needed.

## Quick Start (Source)

```bash
cargo build --manifest-path src_rust/Cargo.toml --release --bin hiresti
./src_rust/target/release/hiresti
```

The single workspace build links `rust_audio_core`, `rust_viz_core`, and `rust_tidal_core` statically into the `hiresti` binary (~16 MB).

## Install Prebuilt Packages

Please download prebuilt packages from the release page.

### Debian / Ubuntu (DEB)

```bash
sudo apt install ./hiresti_<version>_amd64.deb
```

### Fedora (RPM)

```bash
sudo dnf install ./hiresti-<version>-1.fedora.<arch>.rpm
```

### openSUSE Tumbleweed (RPM)

```bash
sudo zypper install ./hiresti-<version>-1.opensuse.<arch>.rpm
```

### Arch Linux

```bash
sudo pacman -U ./hiresti-<version>-1-<arch>.pkg.tar.zst
```

## Upgrade Guide

### Playlist migration note

Starting from `v1.1.0`, local playlists are removed. Only cloud playlists are supported.

### Migrating to 1.10

The 1.10 rewrite drops the Python runtime entirely. Settings / token files in `~/.config/hiresti/` are read-compatible — your account stays signed in across the upgrade.

If you have an older OAuth-only session, sign out + sign back in to switch to PKCE and restore Hi-Res quality.

### Fedora / RPM upgrades

Use upgrade mode when moving to a newer version:

```bash
sudo dnf upgrade ./hiresti-<version>-1.fedora.<arch>.rpm
```

or:

```bash
sudo rpm -Uvh ./hiresti-<version>-1.fedora.<arch>.rpm
```

Do not use `rpm -i` for upgrades, because it installs side-by-side and can cause file conflict errors.

## Support

If you run into issues, have feature requests, or want to report bugs, please open a GitHub issue:

- https://github.com/yelanxin/hiresTI/issues

## Troubleshooting With Logs

If you hit a problem, please start the app from terminal and attach logs in your issue:

```bash
hiresti 2>&1 | tee /tmp/hiresti.log
```

For richer tracing output:

```bash
RUST_LOG=hiresti_ui=debug,rust_audio_core=info hiresti 2>&1 | tee /tmp/hiresti.log
```

When reporting, include:

- your distro and desktop environment
- app version
- steps to reproduce
- relevant log snippets (or the full log file path above)

## Acknowledgements

Special thanks to everyone who shares feedback. In particular, [ilijagosp](https://github.com/ilijagosp) has provided feedback and suggestions with every new release.

## Sponsors

Thanks to those supporting hiresTI ❤

<a href="https://github.com/AriZone"><img src="https://github.com/AriZone.png" width="60" height="60" alt="AriZone" /></a>

If you'd like to support development, you can sponsor via [GitHub Sponsors](https://github.com/sponsors/yelanxin).

## License

GPL-3.0
