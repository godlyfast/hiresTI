"""Phase 1 token persistence round-trip via rust_tidal_core.

Doesn't touch the network — only exercises the JSON schema compatibility
between the existing hiresti_token.json format and the Rust read/write path.
"""

import json
import os
import sys
import tempfile

import pytest

sys.path.insert(0, os.path.join(os.path.dirname(__file__), "..", "src"))

from _rust.tidal import get_rust_tidal_core, RustTidalCoreError


@pytest.fixture(scope="module")
def core():
    c = get_rust_tidal_core()
    if not c.available:
        pytest.skip("rust_tidal_core .so not built yet (cargo build --release)")
    return c


def test_token_read_existing_legacy_schema(core, tmp_path):
    # Legacy schema written by previous app versions: flat fields, ISO 8601
    # datetime without timezone (the existing default), is_pkce True.
    fixture = {
        "token_type": "Bearer",
        "access_token": "tok",
        "refresh_token": "ref",
        "expiry_time": "2026-12-31T23:59:59",
        "is_pkce": True,
    }
    path = tmp_path / "hiresti_token.json"
    path.write_text(json.dumps(fixture), encoding="utf-8")
    persisted = core.token_read_file(path)
    assert persisted == fixture


def test_token_write_creates_atomic_with_strict_perms(core, tmp_path):
    persisted = {
        "token_type": "Bearer",
        "access_token": "tok",
        "refresh_token": "ref",
        "expiry_time": "2027-01-01T00:00:00+00:00",
        "is_pkce": False,
    }
    path = tmp_path / "out.json"
    core.token_write_file(path, persisted)
    assert path.exists()
    # POSIX 0o600 means owner-only read/write.
    mode = path.stat().st_mode & 0o777
    assert mode == 0o600
    # File contains valid JSON matching what we wrote.
    back = json.loads(path.read_text(encoding="utf-8"))
    assert back == persisted


def test_token_read_missing_returns_typed_error(core, tmp_path):
    path = tmp_path / "does_not_exist.json"
    with pytest.raises(RustTidalCoreError) as excinfo:
        core.token_read_file(path)
    # The Rust error_kind taxonomy collapses "no such file" into "unknown"
    # since it's an io error — verify the kind string is one of the
    # documented values, not None.
    assert excinfo.value.kind in {"unknown", "client", "not_found"}


def test_token_read_nested_data_schema_compat(core, tmp_path):
    # tidalapi's save_session_to_file wraps each field in {"data": ...}; our
    # reader has to accept that legacy form too in case a user round-tripped
    # through the upstream tooling.
    fixture = {
        "token_type": {"data": "Bearer"},
        "access_token": {"data": "tok"},
        "refresh_token": {"data": "ref"},
        "is_pkce": {"data": True},
    }
    path = tmp_path / "nested.json"
    path.write_text(json.dumps(fixture), encoding="utf-8")
    persisted = core.token_read_file(path)
    assert persisted["access_token"] == "tok"
    assert persisted["refresh_token"] == "ref"
    assert persisted["is_pkce"] is True


def test_session_lifecycle_no_network(core):
    sess = core.new_session()
    try:
        assert sess.handle != 0
        assert sess.user_snapshot() is None
        assert sess.token_snapshot() is None
        assert sess.check_login() is False
    finally:
        sess.close()
