"""ctypes loader for rust_tidal_core.

Exposes a thin Python class hierarchy mirroring the parts of tidalapi.Session
that hiresTI actually uses. Phases progressively shift each surface from
tidalapi to this module:

  Phase 1 (current): auth + session persistence + check_login + token refresh.
  Phase 2: HTTP / page.get.
  Phase 3+: model constructors, favorites, playback, etc.

The loader silently no-ops if the .so isn't present so app boot stays
unaffected during the migration window — backend/tidal.py falls back to
tidalapi for any surface that doesn't yet have a Rust implementation.
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
# Hybrid model wrappers (Phase 3)
# ---------------------------------------------------------------------------
#
# Each Rust-fetched model is presented to backend/tidal.py as an object with
# attribute access matching the tidalapi shape callers already use:
#   track.id, track.name, track.duration, track.album.cover, ...
#
# Method calls (.tracks(), .items(), .add(), .delete(), .lyrics(),
# .get_stream()) are not yet implemented in Rust — those land in Phase 4-6.
# Until then the wrapper lazily delegates them to a tidalapi proxy
# constructed on demand. That bridge disappears in Phase 7.


_PROXY_FAILED = object()  # sentinel: tidalapi proxy construction has failed for this wrapper


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
    """Base for hybrid Rust+tidalapi models. Subclasses set
    `_tidalapi_factory` (a callable taking `tidalapi_session` -> proxy) so
    method calls fall through to tidalapi until Phase 4+ replace them."""

    _tidalapi_factory = None  # type: Any
    # Cheap aliases for fields whose name differs between Rust JSON and
    # tidalapi/TIDAL upstream naming. Hits here avoid building a tidalapi
    # proxy (which would round-trip GET albums/{id} just to read .title).
    # Bidirectional: Album/Track/Artist/Playlist use `name`, Mix uses
    # `title` — code that probes either should land on whichever the
    # underlying model stores.
    _FIELD_ALIASES = {"title": "name", "name": "title"}
    # Explicit allowlist of attribute names that should fall through to
    # the tidalapi proxy. Constructing the proxy fires an HTTP fetch
    # (tidalapi.Album/Track/Artist all GET on __init__), so any unknown
    # attribute we DON'T list here will raise AttributeError instead of
    # silently spending a network round-trip — important because
    # get_artwork_url and similar code paths probe ~12 attribute names
    # per item, which used to cost ~12 HTTP fetches per home-page card.
    _PROXY_METHODS: frozenset[str] = frozenset()

    def __init__(
        self,
        data: dict,
        tidalapi_session=None,
        rust_session: Optional["RustTidalSession"] = None,
    ) -> None:
        object.__setattr__(self, "_data", dict(data or {}))
        object.__setattr__(self, "_tidalapi_session", tidalapi_session)
        object.__setattr__(self, "_rust_session", rust_session)
        object.__setattr__(self, "_tidalapi_proxy", None)

    def __getattr__(self, name: str) -> Any:
        # Rust-known fields take priority. Nested dicts wrap as _NestedRef so
        # `track.album.cover` keeps working. Direct hit wins; otherwise try
        # the bidirectional alias (so name↔title works whichever side a
        # given model stores).
        data = self.__dict__.get("_data") or {}
        for key in (name, self._FIELD_ALIASES.get(name)):
            if key is not None and key in data:
                v = data[key]
                if isinstance(v, dict):
                    return _NestedRef(v)
                if isinstance(v, list) and v and isinstance(v[0], dict):
                    return [_NestedRef(item) for item in v]
                return v
        # Only known proxy methods fall through. Arbitrary attribute reads
        # that happen to miss our Rust data (cover_url, picture, image,
        # description, ...) raise AttributeError instead of triggering an
        # HTTP fetch.
        if name in self._PROXY_METHODS:
            proxy = self._ensure_proxy()
            if proxy is not None:
                return getattr(proxy, name)
        raise AttributeError(
            f"{type(self).__name__} has no attribute {name!r} "
            f"(rust fields: {sorted(data)})"
        )

    def _ensure_proxy(self):
        proxy = self.__dict__.get("_tidalapi_proxy")
        if proxy is _PROXY_FAILED:
            return None
        if proxy is not None:
            return proxy
        factory = type(self)._tidalapi_factory
        session = self.__dict__.get("_tidalapi_session")
        if not factory or session is None:
            object.__setattr__(self, "_tidalapi_proxy", _PROXY_FAILED)
            return None
        try:
            proxy = factory(session, self.__dict__["_data"])
        except Exception as e:  # noqa: BLE001
            # Cache the failure: tidalapi factories that raise (e.g.
            # session.album(id) → ObjectNotFound for a dead catalog entry)
            # will keep raising. Without this sentinel, every attribute
            # miss on the wrapper re-fires the same HTTP 404 — get_artwork_url
            # alone probes ~12 missing attrs and amplifies one dead ID into
            # a 12x request storm.
            logger.debug(
                "tidalapi proxy construction failed for %s: %s",
                type(self).__name__,
                e,
            )
            object.__setattr__(self, "_tidalapi_proxy", _PROXY_FAILED)
            return None
        object.__setattr__(self, "_tidalapi_proxy", proxy)
        return proxy

    def to_dict(self) -> dict:
        return dict(self._data)

    def __repr__(self) -> str:
        d = self.__dict__.get("_data") or {}
        return f"{type(self).__name__}(id={d.get('id')!r}, name={d.get('name') or d.get('title')!r})"


def _make_track_proxy(session, data):
    return session.track(data["id"]) if data.get("id") else None


def _make_album_proxy(session, data):
    return session.album(data["id"]) if data.get("id") else None


def _make_artist_proxy(session, data):
    return session.artist(data["id"]) if data.get("id") else None


def _make_playlist_proxy(session, data):
    return session.playlist(data["id"]) if data.get("id") else None


def _make_mix_proxy(session, data):
    return session.mix(data["id"]) if data.get("id") else None


def _make_folder_proxy(session, data):
    fn = getattr(session, "folder", None)
    if fn is None:
        return None
    return fn(data["id"]) if data.get("id") else None


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
    _tidalapi_factory = staticmethod(_make_track_proxy)
    # Stream methods are served by Rust directly via TidalBackend now;
    # the proxy fallback exists only for code paths that still touch
    # full_track.get_url() when the .so isn't loaded.
    _PROXY_METHODS = frozenset({
        "get_url", "get_stream", "get_stream_manifest", "get_manifest_data",
    })

    def lyrics(self):
        """Phase 6: Rust-native lyrics. Returns a small object with
        `.text`, `.subtitles`, `.right_to_left`, `.lyrics_provider`. Falls
        back to the tidalapi proxy if the Rust path errors (e.g. crate
        not loaded), so existing call sites keep working unchanged."""
        rust_session = self.__dict__.get("_rust_session")
        tid = self._data.get("id")
        if rust_session is None or not tid:
            proxy = self._ensure_proxy()
            if proxy is None:
                return None
            return proxy.lyrics()
        try:
            data = rust_session.track_lyrics(int(tid))
        except RustTidalCoreError as e:
            logger.debug("rust track_lyrics(%s) error [%s]: %s", tid, e.kind, e)
            proxy = self._ensure_proxy()
            if proxy is None:
                return None
            return proxy.lyrics()
        return _LyricsView(data)


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
    _tidalapi_factory = staticmethod(_make_album_proxy)
    _PROXY_METHODS = frozenset()

    def tracks(self, limit: Optional[int] = None, offset: int = 0):
        rust_session = self.__dict__.get("_rust_session")
        if rust_session is None or not self._data.get("id"):
            proxy = self._ensure_proxy()
            if proxy is None:
                return []
            return proxy.tracks() if limit is None else proxy.tracks(limit=limit, offset=offset)
        try:
            if limit is None:
                items = _drain_pages(
                    rust_session,
                    "album_tracks",
                    page_size=100,
                    id=int(self._data["id"]),
                )
            else:
                page = rust_session.list(
                    "album_tracks",
                    limit=int(limit),
                    offset=int(offset),
                    id=int(self._data["id"]),
                )
                items = (page or {}).get("items") or []
        except RustTidalCoreError:
            proxy = self._ensure_proxy()
            if proxy is None:
                return []
            return proxy.tracks() if limit is None else proxy.tracks(limit=limit, offset=offset)
        ts = self.__dict__.get("_tidalapi_session")
        return [wrap_model("track", t, tidalapi_session=ts, rust_session=rust_session) for t in items or []]

    def items(self, *args, **kwargs):
        return self.tracks(*args, **kwargs)


class RustArtist(_RustModelBase):
    _tidalapi_factory = staticmethod(_make_artist_proxy)
    _PROXY_METHODS = frozenset()


class RustPlaylist(_RustModelBase):
    _tidalapi_factory = staticmethod(_make_playlist_proxy)
    _PROXY_METHODS = frozenset()

    def tracks(self, limit: Optional[int] = None, offset: int = 0):
        rust_session = self.__dict__.get("_rust_session")
        pid = self._data.get("id")
        if rust_session is None or not pid:
            proxy = self._ensure_proxy()
            if proxy is None:
                return []
            return proxy.tracks() if limit is None else proxy.tracks(limit=limit, offset=offset)
        try:
            if limit is None:
                items = _drain_pages(
                    rust_session,
                    "playlist_tracks",
                    page_size=100,
                    id=str(pid),
                )
            else:
                page = rust_session.list(
                    "playlist_tracks",
                    limit=int(limit),
                    offset=int(offset),
                    id=str(pid),
                )
                items = (page or {}).get("items") or []
        except RustTidalCoreError:
            proxy = self._ensure_proxy()
            if proxy is None:
                return []
            return proxy.tracks() if limit is None else proxy.tracks(limit=limit, offset=offset)
        ts = self.__dict__.get("_tidalapi_session")
        return [wrap_model("track", t, tidalapi_session=ts, rust_session=rust_session) for t in items or []]

    def items(self, limit: Optional[int] = None, offset: int = 0):
        rust_session = self.__dict__.get("_rust_session")
        pid = self._data.get("id")
        if rust_session is None or not pid:
            proxy = self._ensure_proxy()
            if proxy is None:
                return []
            return proxy.items() if limit is None else proxy.items(limit=limit, offset=offset)
        try:
            if limit is None:
                items = _drain_pages(
                    rust_session,
                    "playlist_items",
                    page_size=100,
                    id=str(pid),
                )
            else:
                page = rust_session.list(
                    "playlist_items",
                    limit=int(limit),
                    offset=int(offset),
                    id=str(pid),
                )
                items = (page or {}).get("items") or []
        except RustTidalCoreError:
            proxy = self._ensure_proxy()
            if proxy is None:
                return []
            return proxy.items() if limit is None else proxy.items(limit=limit, offset=offset)
        ts = self.__dict__.get("_tidalapi_session")
        # Each item is {"kind": "track" | "video", ...}; extract the inner
        # model so callers can keep using attribute access.
        out = []
        for it in items or []:
            if not isinstance(it, dict):
                continue
            kind = it.get("kind", "track")
            inner = {k: v for k, v in it.items() if k != "kind"}
            out.append(wrap_model(kind, inner, tidalapi_session=ts, rust_session=rust_session))
        return out


class RustMix(_RustModelBase):
    _tidalapi_factory = staticmethod(_make_mix_proxy)
    _PROXY_METHODS = frozenset()

    def items(self, limit: Optional[int] = None, offset: int = 0):
        rust_session = self.__dict__.get("_rust_session")
        mid = self._data.get("id")
        if rust_session is None or not mid:
            proxy = self._ensure_proxy()
            if proxy is None:
                return []
            return proxy.items() if limit is None else proxy.items(limit=limit, offset=offset)
        try:
            if limit is None:
                items = _drain_pages(
                    rust_session,
                    "mix_items",
                    page_size=100,
                    id=str(mid),
                )
            else:
                page = rust_session.list(
                    "mix_items",
                    limit=int(limit),
                    offset=int(offset),
                    id=str(mid),
                )
                items = (page or {}).get("items") or []
        except RustTidalCoreError:
            proxy = self._ensure_proxy()
            if proxy is None:
                return []
            return proxy.items() if limit is None else proxy.items(limit=limit, offset=offset)
        ts = self.__dict__.get("_tidalapi_session")
        out = []
        for it in items or []:
            if not isinstance(it, dict):
                continue
            kind = it.get("kind", "track")
            inner = {k: v for k, v in it.items() if k != "kind"}
            out.append(wrap_model(kind, inner, tidalapi_session=ts, rust_session=rust_session))
        return out


class RustFolder(_RustModelBase):
    _tidalapi_factory = staticmethod(_make_folder_proxy)


class RustVideo(_RustModelBase):
    _tidalapi_factory = None


def wrap_model(kind: str, data: dict, tidalapi_session=None, rust_session=None):
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
    return cls(data, tidalapi_session=tidalapi_session, rust_session=rust_session)


_singleton: Optional[_RustTidalCore] = None


def get_rust_tidal_core() -> _RustTidalCore:
    global _singleton
    if _singleton is None:
        _singleton = _RustTidalCore()
    return _singleton
