"""A playlist queue whose metadata is fetched independently of the track view."""

from threading import Lock

from models.local import LocalAlbumInfo, LocalArtist, LocalTrack


class PlaylistQueue:
    def __init__(self, total, fetch_track, name="Playlist", shuffled=False):
        self.total = total
        self.name = name
        self.shuffled = shuffled
        self._fetch_track = fetch_track
        self._tracks = {}
        self._lock = Lock()

    def resolve(self, position):
        # Playback and prefetch can request the same entry concurrently.
        with self._lock:
            if position not in self._tracks:
                track = self._fetch_track(position)
                if track is None or getattr(track, "id", None) is None:
                    raise ValueError("Could not load the playlist track. Please try again.")
                self._tracks[position] = track
            return self._tracks[position]

    def entries(self):
        return [PlaylistQueueTrack(self, position) for position in range(self.total)]


class PlaylistQueueTrack:
    """Nonblocking queue entry. Only resolve() performs network I/O."""

    def __init__(self, source, position):
        self.source = source
        self.position = position
        self._placeholder = LocalTrack(
            name=f"Track {position + 1} — details pending",
            artist=LocalArtist(name=""),
            album=LocalAlbumInfo(name=source.name),
        )

    @property
    def resolved_track(self):
        return self.source._tracks.get(self.position)

    def resolve(self):
        return self.source.resolve(self.position)

    def __getattr__(self, name):
        track = self.resolved_track
        return getattr(track if track is not None else self._placeholder, name)
