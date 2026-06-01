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


def build_yt_dlp_cmd(vid: str, out_dir: str, cookies: str | None, proxy: str | None) -> list[str]:
    """Construct the yt-dlp argv for subtitle-only extraction (json3)."""
    cmd = [
        "yt-dlp", "--skip-download",
        "--write-subs", "--write-auto-subs",
        "--sub-langs", "en.*", "--sub-format", "json3",
        "--extractor-args", "youtube:player_client=mweb",
        "-o", f"{out_dir}/%(id)s.%(ext)s",
    ]
    if cookies:
        cmd += ["--cookies", cookies]
    if proxy:
        cmd += ["--proxy", proxy]
    cmd.append(f"https://www.youtube.com/watch?v={vid}")
    return cmd


def _yt_dlp_fetch(vid: str, cookies: str | None, proxy: str | None) -> str:
    """Run yt-dlp to download json3 subtitles to a temp dir and return the text."""
    with tempfile.TemporaryDirectory() as d:
        cmd = build_yt_dlp_cmd(vid, d, cookies, proxy)
        proc = subprocess.run(cmd, capture_output=True, text=True, timeout=120)
        files = sorted(Path(d).glob(f"{vid}*.json3"))
        if not files:
            raise RuntimeError(
                f"yt-dlp produced no json3 subs (rc={proc.returncode}): "
                f"{(proc.stderr or proc.stdout).strip()[:300]}")
        return parse_json3(files[0].read_text(encoding="utf-8"))
