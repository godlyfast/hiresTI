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


_singleton: Optional[_RustTidalCore] = None


def get_rust_tidal_core() -> _RustTidalCore:
    global _singleton
    if _singleton is None:
        _singleton = _RustTidalCore()
    return _singleton
