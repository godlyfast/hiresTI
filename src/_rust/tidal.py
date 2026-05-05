"""ctypes loader for rust_tidal_core.

The Rust crate handles every TIDAL surface hiresTI actually uses: auth +
token persistence, generic authenticated HTTP, model fetchers + parsers,
favorites + library listings, album/playlist/mix item drains, stream
URLs + manifest decoding, lyrics, and the artist tail surfaces (top
tracks, albums, EPs/singles, similar). The Python side here is just the
ctypes bindings + a small wrapper class hierarchy that gives backend
code attribute access in the tidalapi shape it already used.

Wrappers are pure read-only views over the Rust JSON. There's no lazy
proxy fallback — an unknown attribute raises AttributeError instead of
silently spending a network round-trip. The handful of surfaces still
served via tidalapi (pages, home v1, search fallback) live in
backend/tidal.py and are accessed there directly, not through these
wrappers.

The loader silently no-ops if the .so isn't present so app boot stays
unaffected when the crate hasn't been built; backend/tidal.py treats
that as a hard requirement for the live paths and falls back to tidalapi
only for the bootstrap case.
"""

from __future__ import annotations

import ctypes
import json
import logging
from pathlib import Path
from typing import Any, Optional

logger = logging.getLogger(__name__)


class RustTidalCoreUnavailable(RuntimeError):
    """Raised when an entry point is called but the .so couldn't be loaded."""


class RustTidalCoreError(RuntimeError):
    """Structured error returned by the Rust core. Carries the same kind
    strings (auth/network/server/not_found/client/unknown) used by
    core.errors.classify_exception so callers can dispatch uniformly."""

    def __init__(self, payload: dict) -> None:
        message = payload.get("error") or payload.get("message") or "rust_tidal_core error"
        super().__init__(message)
        self.payload = payload
        self.kind = payload.get("kind", "unknown")
        self.status = payload.get("status")


class _RustTidalCore:
    """Thin ctypes wrapper. JSON in, JSON out."""

    def __init__(self) -> None:
        self._lib: Optional[ctypes.CDLL] = None
        self._so_path: Optional[Path] = None
        self._load()

    def _candidate_paths(self) -> list[Path]:
        here = Path(__file__).resolve()
        dev_root = here.parent.parent.parent / "src_rust" / "rust_tidal_core" / "target"
        local = [
            dev_root / "release" / "librust_tidal_core.so",
            dev_root / "debug" / "librust_tidal_core.so",
        ]
        installed = [
            Path("/app/share/hiresti/src_rust/rust_tidal_core/target/release/librust_tidal_core.so"),
            Path("/usr/share/hiresti/src_rust/rust_tidal_core/target/release/librust_tidal_core.so"),
        ]
        return local + installed

    def _load(self) -> None:
        existing = [p for p in self._candidate_paths() if p.exists()]
        if not existing:
            logger.info(
                "rust_tidal_core .so not found; tried=%s",
                [str(p) for p in self._candidate_paths()],
            )
            return
        so_path = max(existing, key=lambda p: p.stat().st_mtime)
        try:
            lib = ctypes.CDLL(str(so_path))

            lib.rtc_version.restype = ctypes.c_void_p
            lib.rtc_version.argtypes = []

            lib.rtc_echo_json.restype = ctypes.c_void_p
            lib.rtc_echo_json.argtypes = [ctypes.c_char_p]

            lib.rtc_free_string.restype = None
            lib.rtc_free_string.argtypes = [ctypes.c_void_p]

            lib.rtc_session_new.restype = ctypes.c_void_p
            lib.rtc_session_new.argtypes = [ctypes.c_int]

            lib.rtc_session_free.restype = None
            lib.rtc_session_free.argtypes = [ctypes.c_void_p]

            for name in (
                "rtc_session_token_snapshot",
                "rtc_session_user_snapshot",
                "rtc_session_persisted_snapshot",
                "rtc_session_check_login",
                "rtc_session_pkce_login_url",
                "rtc_session_oauth_device_start",
                "rtc_session_oauth_device_poll",
                "rtc_session_refresh_token",
            ):
                fn = getattr(lib, name)
                fn.restype = ctypes.c_void_p
                fn.argtypes = [ctypes.c_void_p]

            lib.rtc_session_load_token.restype = ctypes.c_void_p
            lib.rtc_session_load_token.argtypes = [ctypes.c_void_p, ctypes.c_char_p]

            lib.rtc_session_pkce_finish.restype = ctypes.c_void_p
            lib.rtc_session_pkce_finish.argtypes = [ctypes.c_void_p, ctypes.c_char_p]

            lib.rtc_session_request.restype = ctypes.c_void_p
            lib.rtc_session_request.argtypes = [ctypes.c_void_p, ctypes.c_char_p]

            lib.rtc_session_fetch_track.restype = ctypes.c_void_p
            lib.rtc_session_fetch_track.argtypes = [ctypes.c_void_p, ctypes.c_int64]

            lib.rtc_session_fetch_album.restype = ctypes.c_void_p
            lib.rtc_session_fetch_album.argtypes = [ctypes.c_void_p, ctypes.c_int64]

            lib.rtc_session_fetch_artist.restype = ctypes.c_void_p
            lib.rtc_session_fetch_artist.argtypes = [ctypes.c_void_p, ctypes.c_int64]

            lib.rtc_session_fetch_playlist.restype = ctypes.c_void_p
            lib.rtc_session_fetch_playlist.argtypes = [ctypes.c_void_p, ctypes.c_char_p]

            lib.rtc_session_fetch_mix.restype = ctypes.c_void_p
            lib.rtc_session_fetch_mix.argtypes = [ctypes.c_void_p, ctypes.c_char_p]

            lib.rtc_session_fetch_folder.restype = ctypes.c_void_p
            lib.rtc_session_fetch_folder.argtypes = [ctypes.c_void_p, ctypes.c_char_p]

            lib.rtc_session_search.restype = ctypes.c_void_p
            lib.rtc_session_search.argtypes = [ctypes.c_void_p, ctypes.c_char_p, ctypes.c_int]

            lib.rtc_session_page_get_raw.restype = ctypes.c_void_p
            lib.rtc_session_page_get_raw.argtypes = [ctypes.c_void_p, ctypes.c_char_p]

            lib.rtc_parse_model.restype = ctypes.c_void_p
            lib.rtc_parse_model.argtypes = [ctypes.c_char_p]

            lib.rtc_session_favorites_add.restype = ctypes.c_void_p
            lib.rtc_session_favorites_add.argtypes = [
                ctypes.c_void_p, ctypes.c_char_p, ctypes.c_char_p,
            ]

            lib.rtc_session_favorites_remove.restype = ctypes.c_void_p
            lib.rtc_session_favorites_remove.argtypes = [
                ctypes.c_void_p, ctypes.c_char_p, ctypes.c_char_p,
            ]

            lib.rtc_session_favorites_mix_toggle.restype = ctypes.c_void_p
            lib.rtc_session_favorites_mix_toggle.argtypes = [
                ctypes.c_void_p, ctypes.c_char_p, ctypes.c_int,
            ]

            lib.rtc_session_list.restype = ctypes.c_void_p
            lib.rtc_session_list.argtypes = [ctypes.c_void_p, ctypes.c_char_p]

            lib.rtc_session_count.restype = ctypes.c_void_p
            lib.rtc_session_count.argtypes = [ctypes.c_void_p, ctypes.c_char_p]

            lib.rtc_session_fetch_stream.restype = ctypes.c_void_p
            lib.rtc_session_fetch_stream.argtypes = [ctypes.c_void_p, ctypes.c_char_p]

            lib.rtc_session_fetch_legacy_url.restype = ctypes.c_void_p
            lib.rtc_session_fetch_legacy_url.argtypes = [ctypes.c_void_p, ctypes.c_char_p]

            lib.rtc_session_track_lyrics.restype = ctypes.c_void_p
            lib.rtc_session_track_lyrics.argtypes = [ctypes.c_void_p, ctypes.c_int64]

            lib.rtc_session_artist_bio.restype = ctypes.c_void_p
            lib.rtc_session_artist_bio.argtypes = [ctypes.c_void_p, ctypes.c_int64]

            lib.rtc_token_read_file.restype = ctypes.c_void_p
            lib.rtc_token_read_file.argtypes = [ctypes.c_char_p]

            lib.rtc_token_write_file.restype = ctypes.c_void_p
            lib.rtc_token_write_file.argtypes = [ctypes.c_char_p, ctypes.c_char_p]
        except OSError as e:
            logger.warning("Failed to load rust_tidal_core from %s: %s", so_path, e)
            return

        self._lib = lib
        self._so_path = so_path
        logger.info("rust_tidal_core loaded from %s", so_path)

    @property
    def available(self) -> bool:
        return self._lib is not None

    @property
    def so_path(self) -> Optional[Path]:
        return self._so_path

    def _take_json(self, raw_ptr: int) -> Any:
        if not raw_ptr:
            return None
        try:
            payload = ctypes.string_at(raw_ptr).decode("utf-8")
            return json.loads(payload)
        finally:
            assert self._lib is not None
            self._lib.rtc_free_string(raw_ptr)

    def _result_or_raise(self, raw_ptr: int) -> Any:
        value = self._take_json(raw_ptr)
        if isinstance(value, dict) and isinstance(value.get("error"), str) and value.get("kind"):
            raise RustTidalCoreError(value)
        return value

    def _require_lib(self) -> ctypes.CDLL:
        if self._lib is None:
            raise RustTidalCoreUnavailable(
                "rust_tidal_core .so not loaded — run `cargo build --release` "
                "under src_rust/rust_tidal_core/"
            )
        return self._lib

    def version(self) -> Optional[dict]:
        if self._lib is None:
            return None
        return self._take_json(self._lib.rtc_version())

    def echo_json(self, payload: Any) -> Any:
        lib = self._require_lib()
        return self._take_json(
            lib.rtc_echo_json(json.dumps(payload, ensure_ascii=False).encode("utf-8"))
        )

    def parse_model(self, kind: str, value: Any) -> dict:
        """Run a raw TIDAL JSON dict through one of the named model parsers.
        Returns the parsed model JSON (or raises RustTidalCoreError)."""
        lib = self._require_lib()
        body = json.dumps({"kind": str(kind), "value": value}, ensure_ascii=False).encode("utf-8")
        return self._result_or_raise(lib.rtc_parse_model(body))

    # ------------------------------------------------------------------ files
    def token_read_file(self, path: str | Path) -> dict:
        lib = self._require_lib()
        return self._result_or_raise(lib.rtc_token_read_file(str(path).encode("utf-8")))

    def token_write_file(self, path: str | Path, persisted: dict) -> dict:
        lib = self._require_lib()
        body = json.dumps(persisted, ensure_ascii=False).encode("utf-8")
        return self._result_or_raise(
            lib.rtc_token_write_file(str(path).encode("utf-8"), body)
        )

    # ---------------------------------------------------------------- session
    def new_session(self, pool_size: int = 0) -> "RustTidalSession":
        lib = self._require_lib()
        handle = lib.rtc_session_new(int(pool_size))
        if not handle:
            raise RustTidalCoreError(
                {"error": "rtc_session_new returned null", "kind": "unknown"}
            )
        return RustTidalSession(self, handle)


class RustTidalSession:
    """Owns a Rust Session handle. Free with .close() / context manager."""

    def __init__(self, core: _RustTidalCore, handle: int) -> None:
        self._core = core
        self._handle = handle

    def close(self) -> None:
        if self._handle and self._core._lib is not None:
            self._core._lib.rtc_session_free(self._handle)
        self._handle = 0

    def __enter__(self) -> "RustTidalSession":
        return self

    def __exit__(self, *_exc) -> None:
        self.close()

    def __del__(self) -> None:
        try:
            self.close()
        except Exception:  # noqa: BLE001
            pass

    @property
    def handle(self) -> int:
        return self._handle

    @property
    def available(self) -> bool:
        return self._handle != 0 and self._core.available

    def _call_session(self, name: str) -> Any:
        lib = self._core._require_lib()
        fn = getattr(lib, name)
        return self._core._result_or_raise(fn(self._handle))

    def _call_session_with_str(self, name: str, arg: str) -> Any:
        lib = self._core._require_lib()
        fn = getattr(lib, name)
        return self._core._result_or_raise(fn(self._handle, arg.encode("utf-8")))

    def _call_session_with_json(self, name: str, payload: Any) -> Any:
        lib = self._core._require_lib()
        fn = getattr(lib, name)
        body = json.dumps(payload, ensure_ascii=False).encode("utf-8")
        return self._core._result_or_raise(fn(self._handle, body))

    # ---------------------------------------------------------------- ops
    def load_token(self, persisted: dict) -> dict:
        """Load a saved token (PersistedToken JSON), call /v1/sessions, and
        return UserInfo {user_id, session_id, country_code, locale}."""
        return self._call_session_with_json("rtc_session_load_token", persisted)

    def token_snapshot(self) -> Optional[dict]:
        return self._call_session("rtc_session_token_snapshot")

    def persisted_snapshot(self) -> Optional[dict]:
        return self._call_session("rtc_session_persisted_snapshot")

    def user_snapshot(self) -> Optional[dict]:
        return self._call_session("rtc_session_user_snapshot")

    def check_login(self) -> bool:
        out = self._call_session("rtc_session_check_login")
        return bool(out.get("ok")) if isinstance(out, dict) else False

    def pkce_login_url(self) -> str:
        out = self._call_session("rtc_session_pkce_login_url")
        return out["url"] if isinstance(out, dict) else ""

    def pkce_finish(self, redirect_url: str) -> dict:
        return self._call_session_with_str("rtc_session_pkce_finish", redirect_url)

    def oauth_device_start(self) -> dict:
        return self._call_session("rtc_session_oauth_device_start")

    def oauth_device_poll(self) -> dict:
        """Returns {"status": "pending"} or {"status": "ok", "user": {...}}."""
        return self._call_session("rtc_session_oauth_device_poll")

    def refresh_token(self) -> dict:
        """Returns the new PersistedToken JSON."""
        return self._call_session("rtc_session_refresh_token")

    # ---------------------------------------------------------------- models
    def fetch_track(self, track_id: int) -> dict:
        lib = self._core._require_lib()
        return self._core._result_or_raise(
            lib.rtc_session_fetch_track(self._handle, ctypes.c_int64(int(track_id)))
        )

    def fetch_album(self, album_id: int) -> dict:
        lib = self._core._require_lib()
        return self._core._result_or_raise(
            lib.rtc_session_fetch_album(self._handle, ctypes.c_int64(int(album_id)))
        )

    def fetch_artist(self, artist_id: int) -> dict:
        lib = self._core._require_lib()
        return self._core._result_or_raise(
            lib.rtc_session_fetch_artist(self._handle, ctypes.c_int64(int(artist_id)))
        )

    def fetch_playlist(self, playlist_id: str) -> dict:
        return self._call_session_with_str("rtc_session_fetch_playlist", str(playlist_id))

    def fetch_mix(self, mix_id: str) -> dict:
        return self._call_session_with_str("rtc_session_fetch_mix", str(mix_id))

    def fetch_folder(self, folder_id: str) -> dict:
        return self._call_session_with_str("rtc_session_fetch_folder", str(folder_id))

    def search(self, query: str, limit: int = 50) -> dict:
        lib = self._core._require_lib()
        return self._core._result_or_raise(
            lib.rtc_session_search(
                self._handle, str(query).encode("utf-8"), int(max(1, min(300, limit)))
            )
        )

    def page_get_raw(self, path: str, params: Optional[dict] = None) -> dict:
        return self._call_session_with_json(
            "rtc_session_page_get_raw",
            {"path": str(path), "params": _scrub_params(params)},
        )

    def page_get(self, path: str, params: Optional[dict] = None) -> "_PageView":
        """Fetch a /pages/* endpoint and return a tidalapi.Page-shaped view
        backed entirely by Rust HTTP + the Python normalizer below."""
        raw = self.page_get_raw(path, params=params)
        return _PageView(raw, rust_session=self)

    # ----- Favorites + listings (Phase 4) -----
    def favorites_add(self, kind: str, item_id: Any) -> bool:
        lib = self._core._require_lib()
        out = self._core._result_or_raise(
            lib.rtc_session_favorites_add(
                self._handle, str(kind).encode("utf-8"), str(item_id).encode("utf-8")
            )
        )
        return bool(out.get("ok")) if isinstance(out, dict) else False

    def favorites_remove(self, kind: str, item_id: Any) -> bool:
        lib = self._core._require_lib()
        out = self._core._result_or_raise(
            lib.rtc_session_favorites_remove(
                self._handle, str(kind).encode("utf-8"), str(item_id).encode("utf-8")
            )
        )
        return bool(out.get("ok")) if isinstance(out, dict) else False

    def favorites_mix_toggle(self, mix_id: str, add: bool) -> bool:
        lib = self._core._require_lib()
        out = self._core._result_or_raise(
            lib.rtc_session_favorites_mix_toggle(
                self._handle, str(mix_id).encode("utf-8"), 1 if add else 0
            )
        )
        return bool(out.get("ok")) if isinstance(out, dict) else False

    def list(
        self,
        kind: str,
        *,
        limit: int = 50,
        offset: int = 0,
        order: Optional[str] = None,
        order_direction: Optional[str] = None,
        id: Any = None,
        folder_id: Optional[str] = None,
    ) -> dict:
        """Returns {items, total_number_of_items, limit, offset}."""
        return self._call_session_with_json(
            "rtc_session_list",
            {
                "kind": str(kind),
                "limit": int(limit),
                "offset": int(offset),
                "order": order,
                "order_direction": order_direction,
                "id": id,
                "folder_id": folder_id,
            },
        )

    def count(self, kind: str) -> int:
        lib = self._core._require_lib()
        out = self._core._result_or_raise(
            lib.rtc_session_count(self._handle, str(kind).encode("utf-8"))
        )
        return int(out.get("count", 0)) if isinstance(out, dict) else 0

    # ----- Stream + manifest (Phase 5) -----
    def fetch_stream(
        self,
        track_id: int,
        audio_quality: str,
        *,
        playback_mode: str = "STREAM",
        asset_presentation: str = "FULL",
    ) -> dict:
        """Fetch + decode the playbackinfopostpaywall envelope. Returns a
        StreamInfo dict — already base64-decoded; for BTS the inner JSON's
        urls/codecs/etc are flattened in."""
        return self._call_session_with_json(
            "rtc_session_fetch_stream",
            {
                "track_id": int(track_id),
                "audio_quality": str(audio_quality),
                "playback_mode": str(playback_mode),
                "asset_presentation": str(asset_presentation),
            },
        )

    def fetch_legacy_url(self, track_id: int, audio_quality: str) -> str:
        """Legacy urlpostpaywall fallback. Returns the first URL."""
        out = self._call_session_with_json(
            "rtc_session_fetch_legacy_url",
            {"track_id": int(track_id), "audio_quality": str(audio_quality)},
        )
        return str(out.get("url") or "") if isinstance(out, dict) else ""

    # ----- Lyrics + artist bio (Phase 6) -----
    def track_lyrics(self, track_id: int) -> dict:
        lib = self._core._require_lib()
        return self._core._result_or_raise(
            lib.rtc_session_track_lyrics(self._handle, ctypes.c_int64(int(track_id)))
        )

    def artist_bio(self, artist_id: int) -> dict:
        lib = self._core._require_lib()
        return self._core._result_or_raise(
            lib.rtc_session_artist_bio(self._handle, ctypes.c_int64(int(artist_id)))
        )

    def request(
        self,
        method: str,
        path: str,
        *,
        base_url: Optional[str] = None,
        params: Optional[dict] = None,
        headers: Optional[dict] = None,
        json_body: Any = None,
        form_body: bool = False,
    ) -> dict:
        """Generic authenticated HTTP. Returns {"ok", "status", "body"}.
        4xx/5xx come back as ok=False with the parsed error body — callers
        decide whether to bubble or retry. Transport / auth errors raise
        RustTidalCoreError instead."""
        args = {
            "method": method,
            "path": path,
            "base_url": base_url,
            "params": _scrub_params(params),
            "headers": headers,
            "json_body": json_body,
            "form_body": bool(form_body),
        }
        return self._call_session_with_json("rtc_session_request", args)


def _scrub_params(params: Optional[dict]) -> Optional[dict]:
    if not params:
        return None
    out = {}
    for k, v in params.items():
        if v is None:
            continue
        out[str(k)] = v
    return out or None


# ---------------------------------------------------------------------------
# Rust model wrappers
# ---------------------------------------------------------------------------
#
# Each Rust-fetched model is presented to backend/tidal.py as an object with
# attribute access matching the tidalapi shape callers already use:
#   track.id, track.name, track.duration, track.album.cover, ...
#
# Phase 7: the lazy tidalapi proxy is gone — every method that callers used
# to reach via attribute access (.tracks, .items, .lyrics, .get_url,
# .get_stream, ...) is either a real method on the subclass (drains the
# Rust list dispatcher) or backed by a TidalBackend method that hits Rust
# directly. Wrappers are now pure read-only views; an unknown attribute
# raises AttributeError instead of silently spending an HTTP round-trip.


class _NestedRef:
    """Read-only attribute view over a nested dict (e.g. track.album.cover)."""

    __slots__ = ("_data",)

    def __init__(self, data: dict) -> None:
        self._data = data or {}

    def __getattr__(self, name: str) -> Any:
        if name in self._data:
            v = self._data[name]
            if isinstance(v, dict):
                return _NestedRef(v)
            return v
        raise AttributeError(name)

    def __getitem__(self, key):
        return self._data[key]

    def __contains__(self, key):
        return key in self._data

    def __repr__(self) -> str:
        return f"_NestedRef({sorted(self._data)})"

    def to_dict(self) -> dict:
        return dict(self._data)


class _RustModelBase:
    """Read-only view over a Rust model dict.

    Attribute access maps directly to Rust JSON keys. Nested dicts are
    auto-wrapped as `_NestedRef` so chains like `track.album.cover` keep
    working without consulting any side-channel proxy. The bidirectional
    `title`↔`name` alias is the only naming smoothing we apply, since
    Album/Track/Artist/Playlist use `name` while Mix uses `title`.
    """

    # Bidirectional alias — Album/Track/Artist/Playlist store name, Mix
    # stores title. Either probe order returns the value whichever side
    # the underlying Rust struct uses.
    _FIELD_ALIASES = {"title": "name", "name": "title"}

    def __init__(
        self,
        data: dict,
        rust_session: Optional["RustTidalSession"] = None,
    ) -> None:
        object.__setattr__(self, "_data", dict(data or {}))
        object.__setattr__(self, "_rust_session", rust_session)

    def __getattr__(self, name: str) -> Any:
        data = self.__dict__.get("_data") or {}
        for key in (name, self._FIELD_ALIASES.get(name)):
            if key is not None and key in data:
                v = data[key]
                if isinstance(v, dict):
                    return _NestedRef(v)
                if isinstance(v, list) and v and isinstance(v[0], dict):
                    return [_NestedRef(item) for item in v]
                return v
        raise AttributeError(
            f"{type(self).__name__} has no attribute {name!r} "
            f"(rust fields: {sorted(data)})"
        )

    def to_dict(self) -> dict:
        return dict(self._data)

    def __repr__(self) -> str:
        d = self.__dict__.get("_data") or {}
        return f"{type(self).__name__}(id={d.get('id')!r}, name={d.get('name') or d.get('title')!r})"


def _drain_pages(rust_session, kind: str, *, page_size: int = 100, **list_kwargs):
    """Phase 4 helper: page through a `_rust_session.list(kind=...)` endpoint
    until the server says we have everything. Used by .tracks() / .items()
    on the wrapped models so callers see the full list, the way tidalapi
    returns it."""
    if rust_session is None:
        return None
    items: list = []
    offset = int(list_kwargs.get("offset", 0) or 0)
    total: Optional[int] = None
    while True:
        page = rust_session.list(
            kind,
            limit=page_size,
            offset=offset,
            **{k: v for k, v in list_kwargs.items() if k != "offset"},
        )
        if not isinstance(page, dict):
            break
        chunk = page.get("items") or []
        items.extend(chunk)
        if total is None:
            total = page.get("total_number_of_items")
            if total is not None and total < 0:
                total = None
        offset += len(chunk)
        if not chunk:
            break
        if total is not None and offset >= total:
            break
        if len(chunk) < page_size:
            break
    return items


class RustTrack(_RustModelBase):
    def lyrics(self):
        """Rust-native lyrics. Returns a `_LyricsView` (text/subtitles/
        right_to_left/lyrics_provider). The view is falsy when neither
        text nor subtitles are present, so callers can do `if lyrics:`."""
        rust_session = self.__dict__.get("_rust_session")
        tid = self._data.get("id")
        if rust_session is None or not tid:
            return None
        try:
            return _LyricsView(rust_session.track_lyrics(int(tid)))
        except RustTidalCoreError as e:
            logger.debug("rust track_lyrics(%s) [%s]: %s", tid, e.kind, e)
            return None


class _LyricsView:
    """Tidalapi-Lyrics-shaped read view over the Rust dict."""

    __slots__ = ("_d",)

    def __init__(self, data):
        self._d = data or {}

    @property
    def text(self):
        return self._d.get("text") or ""

    @property
    def subtitles(self):
        return self._d.get("subtitles") or ""

    @property
    def right_to_left(self):
        return bool(self._d.get("right_to_left"))

    @property
    def lyrics_provider(self):
        return self._d.get("lyrics_provider") or ""

    @property
    def track_id(self):
        return int(self._d.get("track_id") or 0)

    def __bool__(self):
        return bool(self._d.get("text") or self._d.get("subtitles"))


class RustAlbum(_RustModelBase):
    def tracks(self, limit: Optional[int] = None, offset: int = 0):
        rust_session = self.__dict__.get("_rust_session")
        if rust_session is None or not self._data.get("id"):
            return []
        try:
            if limit is None:
                items = _drain_pages(
                    rust_session, "album_tracks",
                    page_size=100, id=int(self._data["id"]),
                )
            else:
                page = rust_session.list(
                    "album_tracks",
                    limit=int(limit), offset=int(offset),
                    id=int(self._data["id"]),
                )
                items = (page or {}).get("items") or []
        except RustTidalCoreError as e:
            logger.debug("rust album_tracks(%s) [%s]: %s", self._data.get("id"), e.kind, e)
            return []
        return [wrap_model("track", t, rust_session=rust_session) for t in items or []]

    def items(self, *args, **kwargs):
        return self.tracks(*args, **kwargs)


class RustArtist(_RustModelBase):
    pass


class RustPlaylist(_RustModelBase):
    def tracks(self, limit: Optional[int] = None, offset: int = 0):
        rust_session = self.__dict__.get("_rust_session")
        pid = self._data.get("id")
        if rust_session is None or not pid:
            return []
        try:
            if limit is None:
                items = _drain_pages(
                    rust_session, "playlist_tracks",
                    page_size=100, id=str(pid),
                )
            else:
                page = rust_session.list(
                    "playlist_tracks",
                    limit=int(limit), offset=int(offset),
                    id=str(pid),
                )
                items = (page or {}).get("items") or []
        except RustTidalCoreError as e:
            logger.debug("rust playlist_tracks(%s) [%s]: %s", pid, e.kind, e)
            return []
        return [wrap_model("track", t, rust_session=rust_session) for t in items or []]

    def items(self, limit: Optional[int] = None, offset: int = 0):
        rust_session = self.__dict__.get("_rust_session")
        pid = self._data.get("id")
        if rust_session is None or not pid:
            return []
        try:
            if limit is None:
                items = _drain_pages(
                    rust_session, "playlist_items",
                    page_size=100, id=str(pid),
                )
            else:
                page = rust_session.list(
                    "playlist_items",
                    limit=int(limit), offset=int(offset),
                    id=str(pid),
                )
                items = (page or {}).get("items") or []
        except RustTidalCoreError as e:
            logger.debug("rust playlist_items(%s) [%s]: %s", pid, e.kind, e)
            return []
        out = []
        for it in items or []:
            if not isinstance(it, dict):
                continue
            kind = it.get("kind", "track")
            inner = {k: v for k, v in it.items() if k != "kind"}
            out.append(wrap_model(kind, inner, rust_session=rust_session))
        return out


class RustMix(_RustModelBase):
    def items(self, limit: Optional[int] = None, offset: int = 0):
        rust_session = self.__dict__.get("_rust_session")
        mid = self._data.get("id")
        if rust_session is None or not mid:
            return []
        try:
            if limit is None:
                items = _drain_pages(
                    rust_session, "mix_items",
                    page_size=100, id=str(mid),
                )
            else:
                page = rust_session.list(
                    "mix_items",
                    limit=int(limit), offset=int(offset),
                    id=str(mid),
                )
                items = (page or {}).get("items") or []
        except RustTidalCoreError as e:
            logger.debug("rust mix_items(%s) [%s]: %s", mid, e.kind, e)
            return []
        out = []
        for it in items or []:
            if not isinstance(it, dict):
                continue
            kind = it.get("kind", "track")
            inner = {k: v for k, v in it.items() if k != "kind"}
            out.append(wrap_model(kind, inner, rust_session=rust_session))
        return out


class RustFolder(_RustModelBase):
    pass


class RustVideo(_RustModelBase):
    pass


def wrap_model(kind: str, data: dict, rust_session=None):
    cls = {
        "track": RustTrack,
        "album": RustAlbum,
        "artist": RustArtist,
        "playlist": RustPlaylist,
        "mix": RustMix,
        "folder": RustFolder,
        "video": RustVideo,
    }.get(str(kind).lower())
    if cls is None:
        return data
    return cls(data, rust_session=rust_session)


# ---------------------------------------------------------------------
# Page view wrappers. TIDAL's /pages/* responses come in two shapes:
#   V1: { "title": ..., "rows": [ { "modules": [ <module>, ... ] }, ... ] }
#   V2: { "title": ..., "items": [ <category>, ... ] }
# tidalapi.Page normalizes both into `.categories[]` where each category
# exposes title / subtitle / description / items / _more. We mirror just
# enough of that surface for the call sites we have.
#
# Items inside a category can be:
#   - model dicts (track / album / artist / playlist / mix / video) — wrapped
#     via wrap_model so callers see RustTrack/RustAlbum/... shapes
#   - PageItem-style cards (header / shortHeader / imageId / type)
#   - PageLink-style links (title / apiPath / imageId)
# We pick which form via the keys present on the JSON dict.
# ---------------------------------------------------------------------


class _PageItem:
    """PageItem/PageLink-shaped read view. Matches tidalapi.PageItem fields
    that the backend already probes (header / short_header / image_id /
    type / api_path / title)."""

    __slots__ = ("_d",)

    def __init__(self, data: dict) -> None:
        self._d = data or {}

    def __getattr__(self, name: str):
        d = object.__getattribute__(self, "_d")
        camel = {
            "image_id": "imageId",
            "short_header": "shortHeader",
            "short_sub_header": "shortSubHeader",
            "api_path": "apiPath",
            "artifact_id": "artifactId",
        }.get(name, name)
        for key in (name, camel):
            if key in d:
                return d[key]
        raise AttributeError(
            f"_PageItem has no attribute {name!r} (json keys: {sorted(d)})"
        )

    def to_dict(self) -> dict:
        return dict(self._d)


class _More:
    __slots__ = ("api_path", "title")

    def __init__(self, api_path: str, title: Optional[str]) -> None:
        self.api_path = api_path
        self.title = title

    @classmethod
    def parse(cls, json_obj: dict) -> Optional["_More"]:
        show_more = json_obj.get("showMore")
        view_all = json_obj.get("viewAll")
        if isinstance(show_more, dict) and show_more.get("apiPath"):
            return cls(api_path=show_more["apiPath"], title=show_more.get("title"))
        if isinstance(view_all, str) and view_all:
            return cls(api_path=view_all, title=json_obj.get("title"))
        return None


class _PageCategory:
    """tidalapi.PageCategory-shaped view over a single TIDAL page module."""

    __slots__ = ("_raw", "_rust_session", "title", "subtitle", "description",
                 "type", "_more", "_items_cache")

    def __init__(self, raw: dict, rust_session=None) -> None:
        self._raw = raw or {}
        self._rust_session = rust_session
        self.title = self._raw.get("title")
        self.subtitle = self._raw.get("subtitle")
        self.description = self._raw.get("description") or self.title
        self.type = self._raw.get("type")
        self._more = _More.parse(self._raw)
        self._items_cache: Optional[list] = None

    @property
    def subTitle(self):
        return self.subtitle

    @property
    def items(self) -> list:
        if self._items_cache is not None:
            return self._items_cache
        self._items_cache = self._build_items()
        return self._items_cache

    def _build_items(self) -> list:
        raw = self._raw
        cat_type = (raw.get("type") or "").upper()

        # MIX_HEADER / ARTIST_HEADER / ALBUM_HEADER: single-item header.
        if cat_type == "MIX_HEADER" and isinstance(raw.get("mix"), dict):
            return [wrap_model("mix", raw["mix"], rust_session=self._rust_session)]
        if cat_type == "ARTIST_HEADER" and isinstance(raw.get("artist"), dict):
            return [wrap_model("artist", raw["artist"], rust_session=self._rust_session)]
        if cat_type == "ALBUM_HEADER" and isinstance(raw.get("album"), dict):
            return [wrap_model("album", raw["album"], rust_session=self._rust_session)]

        # V2 typed items: top-level "items" with each carrying { type, data }.
        v2_items = raw.get("items")
        if isinstance(v2_items, list) and any(
            isinstance(it, dict) and "data" in it for it in v2_items
        ):
            return [self._wrap_v2_item(it) for it in v2_items if it is not None]

        # V1: pagedList holds the items.
        paged = raw.get("pagedList")
        if isinstance(paged, dict):
            inner = paged.get("items") or []
        else:
            inner = []

        # FEATURED_PROMOTIONS / MULTIPLE_TOP_PROMOTIONS: items are PageItem cards.
        if cat_type in ("FEATURED_PROMOTIONS", "MULTIPLE_TOP_PROMOTIONS"):
            return [_PageItem(it) for it in (raw.get("items") or []) if it]

        # PAGE_LINKS / PAGE_LINKS_CLOUD: pagedList items are PageLink cards.
        if cat_type in ("PAGE_LINKS", "PAGE_LINKS_CLOUD"):
            return [_PageItem(it) for it in inner if it]

        # ARTICLE_LIST / SOCIAL: tidalapi rewires these into LinkList shapes.
        if cat_type == "ARTICLE_LIST":
            return [_PageItem(it) for it in inner if it]
        if cat_type == "SOCIAL":
            return [_PageItem(it) for it in (raw.get("socialProfiles") or []) if it]

        # ITEM_LIST_WITH_ROLES: each item wraps an inner item + roles.
        if cat_type == "ITEM_LIST_WITH_ROLES":
            out = []
            for entry in inner:
                if not isinstance(entry, dict):
                    continue
                inner_item = entry.get("item")
                if isinstance(inner_item, dict):
                    inner_item = dict(inner_item)
                    inner_item["artistRoles"] = entry.get("roles")
                    out.append(self._wrap_typed("track", inner_item))
            return out

        # HIGHLIGHT_MODULE: items live under "highlights[].item".
        if cat_type == "HIGHLIGHT_MODULE":
            return [
                self._wrap_v2_item({"data": h.get("item"), "type": (h.get("item") or {}).get("type")})
                for h in (raw.get("highlights") or [])
                if isinstance(h, dict) and h.get("item")
            ]

        # MIXED_TYPES_LIST: each item is { type, ... } where type names a model.
        if cat_type in ("MIXED_TYPES_LIST", "ALBUM_ITEMS"):
            return [self._wrap_v2_item(it) for it in inner if isinstance(it, dict)]

        # The common ITEM_LIST family: TRACK_LIST / ALBUM_LIST / ARTIST_LIST /
        # PLAYLIST_LIST / VIDEO_LIST / MIX_LIST. Items are model dicts.
        kind_map = {
            "TRACK_LIST": "track",
            "ALBUM_LIST": "album",
            "ARTIST_LIST": "artist",
            "PLAYLIST_LIST": "playlist",
            "VIDEO_LIST": "video",
            "MIX_LIST": "mix",
        }
        kind = kind_map.get(cat_type)
        if kind:
            return [self._wrap_typed(kind, it) for it in inner if isinstance(it, dict)]

        # Fallback: surface raw items as _PageItem so attribute probes still work.
        return [_PageItem(it) for it in inner if isinstance(it, dict)]

    def _wrap_typed(self, kind: str, data: dict):
        if not isinstance(data, dict):
            return None
        return wrap_model(kind, data, rust_session=self._rust_session)

    def _wrap_v2_item(self, entry: dict):
        # V2 entry: { "type": "TRACK"|"ALBUM"|..., "data": {...} } or directly
        # a typed dict where "type" is the kind tag.
        if not isinstance(entry, dict):
            return None
        item_type = (entry.get("type") or "").upper()
        data = entry.get("data") if "data" in entry else entry
        if not isinstance(data, dict):
            return None
        kind_map = {
            "TRACK": "track",
            "ALBUM": "album",
            "ARTIST": "artist",
            "PLAYLIST": "playlist",
            "VIDEO": "video",
            "MIX": "mix",
        }
        kind = kind_map.get(item_type)
        if kind:
            return self._wrap_typed(kind, data)
        return _PageItem(data)


class _PageView:
    """tidalapi.Page-shaped view over the raw page_get_raw() JSON."""

    __slots__ = ("_raw", "_rust_session", "title", "categories")

    def __init__(self, raw: dict, rust_session=None) -> None:
        self._raw = raw or {}
        self._rust_session = rust_session
        self.title = self._raw.get("title")
        self.categories = self._build_categories()

    def _build_categories(self) -> list:
        raw = self._raw
        rows = raw.get("rows")
        if isinstance(rows, list) and rows:
            cats: list = []
            for row in rows:
                modules = (row or {}).get("modules") if isinstance(row, dict) else None
                if not modules:
                    continue
                # tidalapi only takes modules[0] per row.
                cats.append(_PageCategory(modules[0] or {}, rust_session=self._rust_session))
            return cats
        items = raw.get("items")
        if isinstance(items, list):
            return [_PageCategory(it or {}, rust_session=self._rust_session) for it in items]
        return []

    def to_dict(self) -> dict:
        return dict(self._raw)


_singleton: Optional[_RustTidalCore] = None


def get_rust_tidal_core() -> _RustTidalCore:
    global _singleton
    if _singleton is None:
        _singleton = _RustTidalCore()
    return _singleton
