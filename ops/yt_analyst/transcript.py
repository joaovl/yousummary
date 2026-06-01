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


def parse_json3(raw: str) -> str:
    """Convert YouTube json3 caption content to clean plain text."""
    data = json.loads(raw)
    parts: list[str] = []
    for ev in data.get("events", []):
        segs = ev.get("segs")
        if not segs:
            continue
        line = "".join(s.get("utf8", "") for s in segs if s.get("utf8") != "\n")
        line = line.strip()
        if line:
            parts.append(line)
    if not parts:
        raise ValueError("no text entries in json3 transcript")
    return re.sub(r"\s+", " ", " ".join(parts)).strip()
