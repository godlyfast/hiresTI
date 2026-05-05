"""Phase 0 smoke test for rust_tidal_core.

Verifies the .so is built and the FFI roundtrip works. Skips cleanly if the
crate hasn't been built yet so the existing test suite stays green pre-build.
"""

import os
import sys

import pytest

sys.path.insert(0, os.path.join(os.path.dirname(__file__), "..", "src"))

from _rust.tidal import get_rust_tidal_core


@pytest.fixture(scope="module")
def core():
    c = get_rust_tidal_core()
    if not c.available:
        pytest.skip("rust_tidal_core .so not built yet (cargo build --release)")
    return c


def test_version_returns_crate_metadata(core):
    info = core.version()
    assert info is not None
    assert info["crate_name"] == "rust_tidal_core"
    assert isinstance(info["version"], str) and info["version"]


def test_echo_json_roundtrips_unicode(core):
    payload = {"hello": "世界", "n": 42, "list": [1, 2, 3]}
    out = core.echo_json(payload)
    assert out == payload
