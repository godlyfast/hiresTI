"""ctypes loader for rust_tidal_core.

Phase 0 surface: just version() + echo_json(). Real entry points (auth,
endpoints, manifests) get added phase by phase. The loader silently no-ops if
the .so isn't present so the rest of the app keeps booting during the
transition window when tidalapi is still authoritative.
"""

from __future__ import annotations

import ctypes
import json
import logging
from pathlib import Path
from typing import Any, Optional

logger = logging.getLogger(__name__)


class _RustTidalCore:
    """Thin ctypes wrapper. JSON in, JSON out."""

    def __init__(self) -> None:
        self._lib: Optional[ctypes.CDLL] = None
        self._so_path: Optional[Path] = None
        self._load()

    def _candidate_paths(self) -> list[Path]:
        # Mirrors src/_rust/audio.py path search so dev + installed both work.
        here = Path(__file__).resolve()
        dev_root = here.parent.parent.parent / "src_rust" / "rust_tidal_core" / "target"
        installed_root = here.parent.parent.parent / "src_rust" / "rust_tidal_core" / "target"
        local = [
            dev_root / "release" / "librust_tidal_core.so",
            dev_root / "debug" / "librust_tidal_core.so",
            installed_root / "release" / "librust_tidal_core.so",
            installed_root / "debug" / "librust_tidal_core.so",
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

    def _take_json(self, raw_ptr: int) -> Optional[Any]:
        """Decode + free a JSON pointer returned by the crate."""
        if not raw_ptr:
            return None
        try:
            payload = ctypes.string_at(raw_ptr).decode("utf-8")
            return json.loads(payload)
        finally:
            assert self._lib is not None
            self._lib.rtc_free_string(raw_ptr)

    def version(self) -> Optional[dict]:
        if self._lib is None:
            return None
        return self._take_json(self._lib.rtc_version())

    def echo_json(self, payload: Any) -> Optional[Any]:
        """Phase 0 roundtrip helper. Removed once real entry points land."""
        if self._lib is None:
            return None
        encoded = json.dumps(payload, ensure_ascii=False).encode("utf-8")
        return self._take_json(self._lib.rtc_echo_json(encoded))


_singleton: Optional[_RustTidalCore] = None


def get_rust_tidal_core() -> _RustTidalCore:
    global _singleton
    if _singleton is None:
        _singleton = _RustTidalCore()
    return _singleton
