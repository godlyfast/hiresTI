"""Phase 3 model parser + wrapper tests.

Verifies the Rust-side parse_model entry point produces correctly normalized
JSON, and the Python hybrid wrapper exposes attribute access matching the
tidalapi shape callers depend on.
"""

import os
import sys

import pytest

sys.path.insert(0, os.path.join(os.path.dirname(__file__), "..", "src"))

from _rust.tidal import (
    get_rust_tidal_core,
    wrap_model,
    RustTrack,
    RustAlbum,
    RustArtist,
    RustPlaylist,
)


@pytest.fixture(scope="module")
def core():
    c = get_rust_tidal_core()
    if not c.available:
        pytest.skip("rust_tidal_core .so not built yet (cargo build --release)")
    return c


def test_parse_track_normalizes_camelcase_to_snake(core):
    raw = {
        "id": 12345,
        "title": "Get Lucky",
        "duration": 248,
        "trackNumber": 8,
        "audioQuality": "LOSSLESS",
        "audioModes": ["STEREO"],
        "artist": {"id": 5, "name": "Daft Punk", "type": "MAIN"},
        "artists": [{"id": 5, "name": "Daft Punk", "type": "MAIN"}],
        "album": {"id": 99, "title": "Random Access Memories", "cover": "uuid"},
    }
    parsed = core.parse_model("track", raw)
    assert parsed["id"] == 12345
    assert parsed["name"] == "Get Lucky"
    assert parsed["track_num"] == 8
    assert parsed["audio_quality"] == "LOSSLESS"
    assert parsed["album"]["name"] == "Random Access Memories"
    assert parsed["artists"][0]["name"] == "Daft Punk"


def test_parse_album_handles_missing_optional_fields(core):
    raw = {"id": 42, "title": "Some Album"}
    parsed = core.parse_model("album", raw)
    assert parsed["id"] == 42
    assert parsed["name"] == "Some Album"
    # Optional numeric fields can come back as None; the underlying Rust
    # serializes Option::None to JSON null.
    assert parsed.get("num_tracks") in (None, 0)


def test_parse_playlist_synthesizes_trn_from_uuid(core):
    raw = {
        "uuid": "abcd-1234",
        "title": "My Playlist",
        "numberOfTracks": 7,
        "duration": 1800,
    }
    parsed = core.parse_model("playlist", raw)
    assert parsed["id"] == "abcd-1234"
    assert parsed["trn"] == "trn:playlist:abcd-1234"
    assert parsed["num_tracks"] == 7


def test_parse_unknown_kind_returns_error(core):
    from _rust.tidal import RustTidalCoreError

    with pytest.raises(RustTidalCoreError):
        core.parse_model("nope", {"x": 1})


def test_wrap_model_exposes_attribute_access():
    track = wrap_model(
        "track",
        {
            "id": 1,
            "name": "T",
            "duration": 200,
            "album": {"id": 9, "name": "A", "cover": "c"},
            "artists": [{"id": 5, "name": "X"}],
        },
    )
    assert isinstance(track, RustTrack)
    assert track.id == 1
    assert track.name == "T"
    assert track.duration == 200
    # Nested dicts: track.album.cover → "c"
    assert track.album.cover == "c"
    assert track.artists[0].name == "X"


def test_wrap_model_returns_correct_class_per_kind():
    assert isinstance(wrap_model("track", {}), RustTrack)
    assert isinstance(wrap_model("album", {}), RustAlbum)
    assert isinstance(wrap_model("artist", {}), RustArtist)
    assert isinstance(wrap_model("playlist", {}), RustPlaylist)


def test_wrap_model_unknown_kind_returns_raw_data():
    raw = {"x": 1}
    out = wrap_model("nope", raw)
    assert out is raw


def test_wrap_model_to_dict_roundtrips():
    track = wrap_model("track", {"id": 42, "name": "X"})
    d = track.to_dict()
    assert d == {"id": 42, "name": "X"}
