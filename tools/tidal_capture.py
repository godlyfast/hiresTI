#!/usr/bin/env python3
"""Capture live tidalapi traffic + responses for cross-validation.

Run this once you have a logged-in session; it loads the same token file the
app uses (~/.config/hiresti/hiresti_token.json), hits a representative set of
endpoints, and writes:

  tests/fixtures/tidal/captured/<bucket>/<name>.http.json   — request envelope
  tests/fixtures/tidal/captured/<bucket>/<name>.body.json   — raw response body
  tests/fixtures/tidal/captured/<bucket>/<name>.parsed.json — parsed object
                                                              snapshot (when
                                                              applicable)

These fixtures drive cross-validation of rust_tidal_core: Rust should hit the
same URLs and produce equivalent parsed outputs given the same response bodies.

Capture output is gitignored — it contains your library, IDs, and a session
token. Curated, sanitized samples for committed tests live in
tests/fixtures/tidal/samples/ (created by hand from these dumps).
"""

from __future__ import annotations

import argparse
import json
import logging
import os
import sys
import time
import traceback
from pathlib import Path
from typing import Any, Callable, Optional

# Make src/ imports work when run from repo root.
REPO_ROOT = Path(__file__).resolve().parent.parent
sys.path.insert(0, str(REPO_ROOT / "src"))

import tidalapi  # noqa: E402

DEFAULT_TOKEN = Path(
    os.environ.get(
        "HIRESTI_TOKEN_FILE",
        str(Path.home() / ".config" / "hiresti" / "hiresti_token.json"),
    )
)
DEFAULT_OUT = REPO_ROOT / "tests" / "fixtures" / "tidal" / "captured"

logger = logging.getLogger("tidal_capture")


# ---------------------------------------------------------------------------
# HTTP recording
# ---------------------------------------------------------------------------


class HttpRecorder:
    """Monkey-patch session.request.request to record every HTTP exchange."""

    def __init__(self, session: tidalapi.Session) -> None:
        self.session = session
        self.records: list[dict] = []
        self._original = None

    def __enter__(self) -> "HttpRecorder":
        request_obj = self.session.request
        self._original = request_obj.request

        def wrapper(method, path, *args, **kwargs):
            t0 = time.time()
            err = None
            response = None
            try:
                response = self._original(method, path, *args, **kwargs)
                return response
            except Exception as e:  # noqa: BLE001
                err = repr(e)
                raise
            finally:
                self._record(method, path, args, kwargs, response, err, time.time() - t0)

        request_obj.request = wrapper  # type: ignore[assignment]
        return self

    def __exit__(self, exc_type, exc, tb) -> None:
        if self._original is not None:
            self.session.request.request = self._original  # type: ignore[assignment]

    def _record(
        self,
        method: str,
        path: str,
        args: tuple,
        kwargs: dict,
        response,
        err: Optional[str],
        elapsed: float,
    ) -> None:
        body_preview = None
        status = None
        if response is not None:
            # tidalapi often returns parsed dict/list/object — try a small JSON
            # preview so the recorded request is still useful even when raw
            # bytes aren't available.
            try:
                body_preview = response if isinstance(response, (dict, list)) else None
            except Exception:  # noqa: BLE001
                body_preview = None
        self.records.append(
            {
                "method": str(method),
                "path": str(path),
                "args": [_jsonable(a) for a in args],
                "kwargs": {k: _jsonable(v) for k, v in kwargs.items()},
                "status": status,
                "elapsed_s": round(elapsed, 4),
                "error": err,
                "body": body_preview,
            }
        )


def _jsonable(v: Any) -> Any:
    try:
        json.dumps(v)
        return v
    except (TypeError, ValueError):
        return repr(v)


# ---------------------------------------------------------------------------
# Capture buckets
# ---------------------------------------------------------------------------


def _safe_call(label: str, fn: Callable[[], Any]) -> Optional[Any]:
    try:
        return fn()
    except Exception as e:  # noqa: BLE001
        logger.warning("capture %s failed: %s", label, e)
        traceback.print_exc()
        return None


def _dump(out_dir: Path, bucket: str, name: str, *, http: list[dict], parsed: Any) -> None:
    folder = out_dir / bucket
    folder.mkdir(parents=True, exist_ok=True)
    (folder / f"{name}.http.json").write_text(
        json.dumps(http, ensure_ascii=False, indent=2, default=_jsonable),
        encoding="utf-8",
    )
    (folder / f"{name}.parsed.json").write_text(
        json.dumps(parsed, ensure_ascii=False, indent=2, default=_jsonable),
        encoding="utf-8",
    )


def _model_snapshot(obj: Any, fields: list[str]) -> dict:
    out: dict[str, Any] = {}
    for f in fields:
        try:
            out[f] = getattr(obj, f, None)
        except Exception as e:  # noqa: BLE001
            out[f] = f"<error: {e}>"
    return out


def capture_track(session: tidalapi.Session, out: Path, track_id: int) -> None:
    bucket = "track"
    with HttpRecorder(session) as rec:
        track = _safe_call("track", lambda: session.track(track_id))
        snapshot = _model_snapshot(
            track,
            ["id", "name", "duration", "track_num", "isrc", "explicit"],
        ) if track else None
        artists = []
        if track is not None:
            for a in getattr(track, "artists", []) or []:
                artists.append(_model_snapshot(a, ["id", "name"]))
        album = _model_snapshot(track.album, ["id", "name", "cover"]) if track and getattr(track, "album", None) else None
    _dump(out, bucket, str(track_id), http=rec.records, parsed={"track": snapshot, "artists": artists, "album": album})


def capture_track_stream(session: tidalapi.Session, out: Path, track_id: int) -> None:
    bucket = "track_stream"
    with HttpRecorder(session) as rec:
        track = _safe_call("track_stream:track", lambda: session.track(track_id))
        stream = _safe_call("track_stream:get_stream", lambda: track.get_stream() if track else None)
        manifest = _safe_call("track_stream:manifest", lambda: stream.get_stream_manifest() if stream else None)
        manifest_data = _safe_call(
            "track_stream:manifest_data",
            lambda: stream.get_manifest_data() if stream else None,
        )
        info = {
            "audio_quality": getattr(stream, "audio_quality", None),
            "bit_depth": getattr(stream, "bit_depth", None),
            "sample_rate": getattr(stream, "sample_rate", None),
            "manifest_mime_type": getattr(stream, "manifest_mime_type", None),
            "is_bts": getattr(manifest, "is_bts", None),
            "is_mpd": getattr(manifest, "is_mpd", None),
            "urls_count": len(manifest.get_urls()) if manifest and getattr(manifest, "is_bts", False) else 0,
            "manifest_data_len": len(manifest_data) if isinstance(manifest_data, (str, bytes)) else None,
        }
    _dump(out, bucket, str(track_id), http=rec.records, parsed=info)


def capture_album(session: tidalapi.Session, out: Path, album_id: int) -> None:
    bucket = "album"
    with HttpRecorder(session) as rec:
        album = _safe_call("album", lambda: session.album(album_id))
        tracks = _safe_call("album:tracks", lambda: list(album.tracks()) if album else [])
        snap = _model_snapshot(
            album, ["id", "name", "cover", "duration", "release_date", "num_tracks"]
        ) if album else None
        track_snaps = [_model_snapshot(t, ["id", "name", "duration", "track_num"]) for t in (tracks or [])]
    _dump(out, bucket, str(album_id), http=rec.records, parsed={"album": snap, "tracks": track_snaps})


def capture_artist(session: tidalapi.Session, out: Path, artist_id: int) -> None:
    bucket = "artist"
    with HttpRecorder(session) as rec:
        artist = _safe_call("artist", lambda: session.artist(artist_id))
        snap = _model_snapshot(artist, ["id", "name", "picture"]) if artist else None
    _dump(out, bucket, str(artist_id), http=rec.records, parsed=snap)


def capture_playlist(session: tidalapi.Session, out: Path, playlist_id: str) -> None:
    bucket = "playlist"
    with HttpRecorder(session) as rec:
        pl = _safe_call("playlist", lambda: session.playlist(playlist_id))
        items = _safe_call("playlist:items", lambda: list(pl.tracks(limit=50, offset=0)) if pl else [])
        snap = _model_snapshot(pl, ["id", "name", "trn", "description", "num_tracks", "duration"]) if pl else None
        item_snaps = [_model_snapshot(i, ["id", "name", "duration"]) for i in (items or [])]
    _dump(out, bucket, str(playlist_id), http=rec.records, parsed={"playlist": snap, "tracks": item_snaps})


def capture_mix(session: tidalapi.Session, out: Path, mix_id: str) -> None:
    bucket = "mix"
    with HttpRecorder(session) as rec:
        mix = _safe_call("mix", lambda: session.mix(mix_id))
        items = _safe_call("mix:items", lambda: list(mix.items()) if mix else [])
        snap = _model_snapshot(mix, ["id", "title", "sub_title"]) if mix else None
        item_snaps = []
        for item in items or []:
            track = getattr(item, "track", None) or item
            item_snaps.append(_model_snapshot(track, ["id", "name", "duration"]))
    _dump(out, bucket, str(mix_id), http=rec.records, parsed={"mix": snap, "items": item_snaps})


def capture_search(session: tidalapi.Session, out: Path, query: str) -> None:
    bucket = "search"
    with HttpRecorder(session) as rec:
        result = _safe_call("search", lambda: session.search(query, limit=20))
        snap = {
            "artists": [_model_snapshot(a, ["id", "name"]) for a in getattr(result, "artists", []) or []],
            "albums": [_model_snapshot(a, ["id", "name"]) for a in getattr(result, "albums", []) or []],
            "tracks": [_model_snapshot(t, ["id", "name"]) for t in getattr(result, "tracks", []) or []],
        } if result else None
    _dump(out, bucket, _safe_filename(query), http=rec.records, parsed=snap)


def capture_favorites(session: tidalapi.Session, out: Path) -> None:
    bucket = "favorites"
    user = session.user
    fav = user.favorites
    with HttpRecorder(session) as rec:
        albums = _safe_call("fav:albums", lambda: list(fav.albums(limit=20, offset=0)))
        tracks = _safe_call("fav:tracks", lambda: list(fav.tracks(limit=20, offset=0)))
        artists = _safe_call("fav:artists", lambda: list(fav.artists(limit=20, offset=0)))
        playlists = _safe_call("fav:playlists", lambda: list(fav.playlists()))
        folders = _safe_call("fav:folders", lambda: list(fav.playlist_folders())) if hasattr(fav, "playlist_folders") else None
    _dump(
        out,
        bucket,
        "page1",
        http=rec.records,
        parsed={
            "albums": [_model_snapshot(a, ["id", "name"]) for a in albums or []],
            "tracks": [_model_snapshot(t, ["id", "name"]) for t in tracks or []],
            "artists": [_model_snapshot(a, ["id", "name"]) for a in artists or []],
            "playlists": [_model_snapshot(p, ["id", "name", "trn"]) for p in playlists or []],
            "folders": [_model_snapshot(f, ["id", "name"]) for f in folders or []] if folders else None,
        },
    )


def capture_pages(session: tidalapi.Session, out: Path) -> None:
    bucket = "pages"
    with HttpRecorder(session) as rec:
        if hasattr(session, "page") and hasattr(session.page, "get"):
            for path in ("pages/genre_page", "pages/moods_page", "pages/hires"):
                _safe_call(f"page:{path}", lambda p=path: session.page.get(p, params={"deviceType": "BROWSER"}))
    _dump(out, bucket, "discovery", http=rec.records, parsed=None)


def capture_lyrics(session: tidalapi.Session, out: Path, track_id: int) -> None:
    bucket = "lyrics"
    with HttpRecorder(session) as rec:
        track = _safe_call("lyrics:track", lambda: session.track(track_id))
        lyrics = _safe_call("lyrics", lambda: track.lyrics() if track else None)
        snap = {
            "subtitles": getattr(lyrics, "subtitles", None),
            "text": getattr(lyrics, "text", None),
        } if lyrics else None
    _dump(out, bucket, str(track_id), http=rec.records, parsed=snap)


def _safe_filename(s: str) -> str:
    keep = "abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789-_."
    return "".join(c if c in keep else "_" for c in s)[:60] or "blank"


# ---------------------------------------------------------------------------
# Session boot
# ---------------------------------------------------------------------------


def load_session(token_path: Path) -> tidalapi.Session:
    if not token_path.exists():
        raise SystemExit(f"token file not found: {token_path}")
    data = json.loads(token_path.read_text(encoding="utf-8"))

    config = tidalapi.Config()
    if "quality" in data:
        try:
            config.quality = tidalapi.Quality(data["quality"])
        except Exception:  # noqa: BLE001
            pass
    session = tidalapi.Session(config)

    is_pkce = bool(data.get("is_pkce", False))
    session.load_oauth_session(
        data["token_type"],
        data["access_token"],
        data.get("refresh_token"),
        data.get("expiry_time"),
        is_pkce=is_pkce,
    )
    if not session.check_login():
        raise SystemExit("session.check_login() returned False — refresh your token first")
    return session


# ---------------------------------------------------------------------------
# Main
# ---------------------------------------------------------------------------


def main() -> int:
    p = argparse.ArgumentParser(description="Capture live tidalapi responses for cross-validation.")
    p.add_argument("--token", type=Path, default=DEFAULT_TOKEN, help="Path to hiresti_token.json")
    p.add_argument("--out", type=Path, default=DEFAULT_OUT, help="Fixture output directory")
    p.add_argument("--track", type=int, action="append", default=[], help="Track ID to capture (repeatable)")
    p.add_argument("--album", type=int, action="append", default=[], help="Album ID to capture (repeatable)")
    p.add_argument("--artist", type=int, action="append", default=[], help="Artist ID to capture (repeatable)")
    p.add_argument("--playlist", type=str, action="append", default=[], help="Playlist UUID to capture (repeatable)")
    p.add_argument("--mix", type=str, action="append", default=[], help="Mix ID to capture (repeatable)")
    p.add_argument("--search", type=str, action="append", default=[], help="Search query to capture (repeatable)")
    p.add_argument("--no-favorites", action="store_true", help="Skip favorites pagination capture")
    p.add_argument("--no-pages", action="store_true", help="Skip discovery page capture")
    p.add_argument("--lyrics", type=int, action="append", default=[], help="Track ID for lyrics capture (repeatable)")
    p.add_argument("--stream", type=int, action="append", default=[], help="Track ID for stream/manifest capture (repeatable)")
    args = p.parse_args()

    logging.basicConfig(level=logging.INFO, format="%(asctime)s %(levelname)s %(message)s")

    session = load_session(args.token)
    args.out.mkdir(parents=True, exist_ok=True)
    logger.info("captured fixtures will be written under %s", args.out)

    for tid in args.track:
        capture_track(session, args.out, tid)
    for tid in args.stream:
        capture_track_stream(session, args.out, tid)
    for aid in args.album:
        capture_album(session, args.out, aid)
    for aid in args.artist:
        capture_artist(session, args.out, aid)
    for pid in args.playlist:
        capture_playlist(session, args.out, pid)
    for mid in args.mix:
        capture_mix(session, args.out, mid)
    for q in args.search:
        capture_search(session, args.out, q)
    for tid in args.lyrics:
        capture_lyrics(session, args.out, tid)
    if not args.no_favorites:
        capture_favorites(session, args.out)
    if not args.no_pages:
        capture_pages(session, args.out)

    logger.info("done")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
