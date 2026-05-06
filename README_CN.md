# HiresTI - Linux 平台的高保真 Tidal 播放器

![Rust](https://img.shields.io/badge/Rust-1.80%2B-orange?logo=rust)
![GTK4](https://img.shields.io/badge/UI-GTK4%20%2B%20Libadwaita-green)
![License](https://img.shields.io/badge/License-GPL--3.0-purple)

![Logo](icons/hicolor/64x64/apps/hiresti.png)

HiresTI 是一款专为发烧友设计的原生、现代 Linux Tidal 桌面客户端。与 Electron 套壳应用不同,HiresTI 基于 Rust + GTK4 / Libadwaita 构建,资源占用极低,并能与 GNOME / KDE 桌面环境无缝集成。

> [!IMPORTANT]
> **需要重新登录才能播放 FLAC / Hi-Res 音质。**
> 旧版本使用的 OAuth(设备码)登录方式已被 Tidal 限制为 320 kbps AAC。**PKCE** 登录方式可恢复完整的 FLAC CD 音质与 Hi-Res Lossless 流。
>
> 如果你目前仍在使用旧的 OAuth 会话,请打开 **设置 → 账户**,先退出登录再重新登录——登录对话框会自动改用 PKCE,你的曲库将恢复到更高的音质档位。

> [!NOTE]
> **1.10 是完整的 Rust 原生重写。** Python UI 层已移除,整个应用现在是一个独立的 Rust 二进制,基于 gtk4-rs / libadwaita-rs / Relm4 构建,运行时无需 Python。

![Main Interface](screenshots/1.png)

## ✨ 核心功能

- 🎧 **高解析度音频**: 支持 Tidal 的 Max(最高 24-bit/192kHz)、High 和 Low 音质流媒体
- ⚡ **位完美模式 (Bit-Perfect)**: 绕过软件混音器和 EQ,将原始音频流精确传输至 DAC
- 🔒 **独占模式**: 直接接管设备的硬件控制权 (ALSA),并通过 `org.freedesktop.ReserveDevice1` 让 PipeWire / WirePlumber 主动让出设备
- 🎨 **现代 UI**: 基于 Libadwaita 构建的美观、自适应界面
- 🛠️ **原生性能**: 单一 Rust 二进制,启动快、占用低
- 🎹 **媒体键 + MPRIS 支持**: 暴露 `org.mpris.MediaPlayer2.hiresti`,支持桌面媒体控件、锁屏小部件
- 📊 **三种可视化模式**: bars / line / spiral,通过设置即时切换
- 📈 **实时 LUFS / DR 表**: 显示 momentary、short-term、integrated LUFS 以及 LRA 和 4 秒 DR
- 🎵 **同步歌词**: 支持点击歌词行跳转到对应时间戳
- 📤 **Last.fm + ListenBrainz Scrobble**: 后台异步提交,与 Python 版本规则一致(≥ 30 秒 AND ≥ min(时长 / 2, 240 秒))
- ☁️ **云端歌单与文件夹**: 支持云端 Playlist 管理、Folder 分组、封面拼贴预览
- 🔌 **远程控制**: 内置 HTTP / JSON-RPC 接口

📌 版本说明:从 v1.1.0 开始已移除本地 Playlist,仅保留云端 Playlist。

## 📸 Screenshots

| Album Detail | Setting |
|:---:|:---:|
| ![Detail](screenshots/2.png) | ![Search](screenshots/3.png) |

## 📥 安装

我们为主流 Linux 发行版提供预构建的安装包。无需手动安装任何运行时依赖。

### 🐧 Debian / Ubuntu / Linux Mint / Deepin

从 [Releases Page](../../releases) 页面下载最新的 .deb 版本。

```bash
sudo apt install ./hiresti_<版本>_amd64.deb
```

### 🎩 Fedora / openSUSE Tumbleweed

从 [Releases Page](../../releases) 页面下载最新的 .rpm 版本。

```bash
# Fedora
sudo dnf install ./hiresti-<版本>-1.fedora.x86_64.rpm

# openSUSE Tumbleweed
sudo zypper install ./hiresti-<版本>-1.opensuse.x86_64.rpm
```

### 🏹 Arch Linux

```bash
sudo pacman -U ./hiresti-<版本>-1-x86_64.pkg.tar.zst
```

## 🦀 从源码构建

```bash
cargo build --manifest-path src_rust/Cargo.toml --release --bin hiresti
./src_rust/target/release/hiresti
```

工作区一次性构建会把 `rust_audio_core` / `rust_viz_core` / `rust_tidal_core` 静态链接进 `hiresti` 二进制(~16 MB)。

构建依赖:gtk4-devel、libadwaita-devel、alsa-lib-devel、pipewire-devel、libusb1-devel、openssl-devel、clang。运行依赖只需:gtk4、libadwaita、pipewire、libpulse、alsa-lib、libusb、openssl。

## License

GPL-3.0

## 赞助 Sponsors

感谢赞助 hiresTI 的朋友 ❤

<a href="https://github.com/AriZone"><img src="https://github.com/AriZone.png" width="60" height="60" alt="AriZone" /></a>

如果你也想支持本项目,可以通过 [GitHub Sponsors](https://github.com/sponsors/yelanxin) 赞助。

## 问题排查与日志反馈

如果遇到问题,请优先通过命令行启动并采集日志,然后在 Issue 中反馈:

```bash
hiresti 2>&1 | tee /tmp/hiresti.log
```

如果需要更详细的 trace 日志:

```bash
RUST_LOG=hiresti_ui=debug,rust_audio_core=info hiresti 2>&1 | tee /tmp/hiresti.log
```

提交 Issue 时建议附带:

- 发行版与桌面环境(例如 Ubuntu 24.04 + KDE)
- 软件版本号
- 复现步骤
- 关键日志片段(或上述完整日志文件)
