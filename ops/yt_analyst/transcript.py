"""Transcript retrieval: fast-path (youtube-transcript-api) then yt-dlp fallback."""
from __future__ import annotations
import json
import re
import subprocess
import tempfile
from pathlib import Path

_ID_RE = re.compile(r"^[A-Za-z0-9_-]{11}$")


def video_id(url: str) -> str:
    """Extract the 11-char YouTube id from a URL or bare id."""
    url = url.strip()
    if _ID_RE.match(url):
        return url
    m = re.search(r"(?:v=|/shorts/|/embed/|youtu\.be/)([A-Za-z0-9_-]{11})", url)
    if not m:
        raise ValueError(f"no YouTube video id in: {url}")
    return m.group(1)
