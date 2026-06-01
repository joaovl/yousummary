# Phase 1 — Transcript Foundation Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** A standalone, tested Python transcript layer (`yt_analyst.transcript`) that reliably returns a plain-text transcript for a YouTube URL — `youtube-transcript-api` fast-path, falling back to `yt-dlp` (json3 subtitles) with a PoToken provider, cookies, and proxy as pluggable anti-block levers — plus the box infrastructure (yt-dlp + `bgutil-ytdlp-pot-provider-rs` on `:4416`) to run it.

**Architecture:** Pure functions for URL→id and json3→text (unit-tested with fixtures), an orchestrator `fetch_transcript()` that escalates fast-path → yt-dlp, and a small CLI for live smoke. This is the foundation the Phase 2 box worker imports; it touches neither the Rust app nor the gate.

**Tech Stack:** Python 3.12, `youtube-transcript-api`, `yt-dlp` (system binary), `bgutil-ytdlp-pot-provider-rs` (Rust binary, HTTP PoToken provider), pytest.

**Conventions:** commits end with `Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>`. Work in `C:\tmp\yousummary`. Box SSH: `root@personal-projectsjl`.

---

## File Structure

| File | Responsibility |
|---|---|
| `ops/yt_analyst/__init__.py` | package marker |
| `ops/yt_analyst/transcript.py` | `video_id()`, `parse_json3()`, `_yt_dlp_fetch()`, `_fast_path()`, `fetch_transcript()` |
| `ops/yt_analyst/__main__.py` | CLI: `python -m yt_analyst <url>` → prints transcript or error |
| `ops/yt_analyst/requirements.txt` | `youtube-transcript-api`, `yt-dlp` |
| `ops/yt_analyst/tests/__init__.py` | marker |
| `ops/yt_analyst/tests/test_transcript.py` | unit tests (id extraction, json3 parsing, orchestrator fallback via monkeypatch) |
| `ops/yt_analyst/tests/fixtures/sample.json3` | a minimal json3 captions fixture |
| `ops/yt_analyst/pytest.ini` | pytest config (markers: `live`) |

---

## Task 1: Scaffold the package

**Files:** Create `ops/yt_analyst/__init__.py`, `ops/yt_analyst/requirements.txt`, `ops/yt_analyst/pytest.ini`, `ops/yt_analyst/tests/__init__.py`.

- [ ] **Step 1: Create `ops/yt_analyst/__init__.py`**
```python
"""yt_analyst — transcript layer for the agentic video analysis worker."""
```

- [ ] **Step 2: Create `ops/yt_analyst/requirements.txt`**
```
youtube-transcript-api>=1.2.4
yt-dlp>=2025.05.22
```

- [ ] **Step 3: Create `ops/yt_analyst/pytest.ini`**
```ini
[pytest]
markers =
    live: hits real YouTube/network (deselect with -m "not live")
testpaths = tests
```

- [ ] **Step 4: Create `ops/yt_analyst/tests/__init__.py`** (empty file)

- [ ] **Step 5: Create venv + install + commit**
```bash
cd /c/tmp/yousummary/ops/yt_analyst && python -m venv .venv && . .venv/Scripts/activate 2>/dev/null || . .venv/bin/activate
pip install -q -r requirements.txt pytest
cd /c/tmp/yousummary && git add ops/yt_analyst && git commit -q -m "chore: scaffold yt_analyst transcript package"
```
Expected: install succeeds; commit created. (`.venv/` is not committed — add `ops/yt_analyst/.venv/` to `.gitignore` if needed.)

---

## Task 2: `video_id()` (TDD)

**Files:** Test `ops/yt_analyst/tests/test_transcript.py`; impl `ops/yt_analyst/transcript.py`.

- [ ] **Step 1: Write the failing test**
```python
import pytest
from yt_analyst.transcript import video_id

@pytest.mark.parametrize("url,expected", [
    ("https://www.youtube.com/watch?v=jlK7UWHD3sY", "jlK7UWHD3sY"),
    ("https://youtu.be/xtKMO_ZH3Qk", "xtKMO_ZH3Qk"),
    ("https://www.youtube.com/watch?v=4WT7FXJah2I&t=30s", "4WT7FXJah2I"),
    ("https://www.youtube.com/shorts/abc123DEF45", "abc123DEF45"),
    ("https://www.youtube.com/embed/abc123DEF45", "abc123DEF45"),
    ("abc123DEF45", "abc123DEF45"),
])
def test_video_id(url, expected):
    assert video_id(url) == expected

def test_video_id_invalid():
    with pytest.raises(ValueError):
        video_id("https://example.com/not-a-video")
```

- [ ] **Step 2: Run, verify it fails**
Run: `cd /c/tmp/yousummary/ops/yt_analyst && PYTHONPATH=.. .venv/bin/python -m pytest tests/test_transcript.py -k video_id -q`
Expected: FAIL — `ModuleNotFoundError: yt_analyst.transcript` / `ImportError`.

- [ ] **Step 3: Implement (start `transcript.py`)**
```python
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
```

- [ ] **Step 4: Run, verify pass**
Run: `cd /c/tmp/yousummary/ops/yt_analyst && PYTHONPATH=.. .venv/bin/python -m pytest tests/test_transcript.py -k video_id -q`
Expected: PASS (7 cases).

- [ ] **Step 5: Commit**
```bash
cd /c/tmp/yousummary && git add ops/yt_analyst/transcript.py ops/yt_analyst/tests/test_transcript.py && git commit -q -m "feat: yt_analyst.video_id"
```

---

## Task 3: `parse_json3()` (TDD)

**Files:** fixture `ops/yt_analyst/tests/fixtures/sample.json3`; extend test + impl.

- [ ] **Step 1: Create the fixture `ops/yt_analyst/tests/fixtures/sample.json3`**
```json
{"events":[
 {"tStartMs":0,"dDurationMs":1200,"segs":[{"utf8":"Hello"},{"utf8":" world"}]},
 {"tStartMs":1200,"dDurationMs":900,"segs":[{"utf8":"\n"}]},
 {"tStartMs":1300,"dDurationMs":1000,"segs":[{"utf8":"second line"}]}
]}
```

- [ ] **Step 2: Write the failing test (append to test_transcript.py)**
```python
from pathlib import Path
from yt_analyst.transcript import parse_json3

def test_parse_json3():
    raw = (Path(__file__).parent / "fixtures" / "sample.json3").read_text(encoding="utf-8")
    text = parse_json3(raw)
    assert text == "Hello world second line"

def test_parse_json3_empty():
    import pytest
    with pytest.raises(ValueError):
        parse_json3('{"events":[]}')
```

- [ ] **Step 3: Run, verify it fails** (`-k parse_json3`) — FAIL: `parse_json3` undefined.

- [ ] **Step 4: Implement (append to transcript.py)**
```python
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
```

- [ ] **Step 5: Run, verify pass** (`-k parse_json3`) — PASS (2 cases).

- [ ] **Step 6: Commit**
```bash
cd /c/tmp/yousummary && git add ops/yt_analyst/transcript.py ops/yt_analyst/tests/ && git commit -q -m "feat: yt_analyst.parse_json3 + fixture"
```

---

## Task 4: `_yt_dlp_fetch()` — build command, run, locate + parse json3 (TDD)

**Files:** extend test + impl. The command is built by a pure helper so it's testable without network.

- [ ] **Step 1: Write the failing test (append)**
```python
from yt_analyst.transcript import build_yt_dlp_cmd

def test_build_yt_dlp_cmd_minimal():
    cmd = build_yt_dlp_cmd("VID12345678", out_dir="/tmp/x", cookies=None, proxy=None)
    assert cmd[0] == "yt-dlp"
    assert "--skip-download" in cmd
    assert "--write-auto-subs" in cmd and "--write-subs" in cmd
    assert "json3" in cmd
    assert "youtube:player_client=mweb" in " ".join(cmd)
    assert "--cookies" not in cmd and "--proxy" not in cmd
    assert cmd[-1] == "https://www.youtube.com/watch?v=VID12345678"

def test_build_yt_dlp_cmd_with_cookies_and_proxy():
    cmd = build_yt_dlp_cmd("VID12345678", out_dir="/tmp/x",
                           cookies="/root/cookies.txt", proxy="http://p:8080")
    s = " ".join(cmd)
    assert "--cookies /root/cookies.txt" in s
    assert "--proxy http://p:8080" in s
```

- [ ] **Step 2: Run, verify it fails** (`-k build_yt_dlp_cmd`) — FAIL: undefined.

- [ ] **Step 3: Implement (append to transcript.py)**
```python
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
```

- [ ] **Step 4: Run, verify pass** (`-k build_yt_dlp_cmd`) — PASS (2 cases).

- [ ] **Step 5: Commit**
```bash
cd /c/tmp/yousummary && git add ops/yt_analyst/transcript.py ops/yt_analyst/tests/ && git commit -q -m "feat: yt_analyst yt-dlp fallback (cmd builder + fetch)"
```

---

## Task 5: `fetch_transcript()` orchestrator — fast-path then fallback (TDD)

**Files:** extend test + impl. Tested with monkeypatch (no network).

- [ ] **Step 1: Write the failing test (append)**
```python
import yt_analyst.transcript as T

def test_fetch_uses_fast_path_when_it_works(monkeypatch):
    monkeypatch.setattr(T, "_fast_path", lambda vid, lang: "FAST TEXT")
    monkeypatch.setattr(T, "_yt_dlp_fetch", lambda *a, **k: (_ for _ in ()).throw(AssertionError("should not call yt-dlp")))
    assert T.fetch_transcript("https://youtu.be/VID12345678") == "FAST TEXT"

def test_fetch_falls_back_to_yt_dlp(monkeypatch):
    def boom(vid, lang): raise RuntimeError("blocked")
    monkeypatch.setattr(T, "_fast_path", boom)
    monkeypatch.setattr(T, "_yt_dlp_fetch", lambda vid, cookies, proxy: "YTDLP TEXT")
    assert T.fetch_transcript("https://youtu.be/VID12345678") == "YTDLP TEXT"

def test_fetch_raises_when_both_fail(monkeypatch):
    monkeypatch.setattr(T, "_fast_path", lambda v, l: (_ for _ in ()).throw(RuntimeError("a")))
    monkeypatch.setattr(T, "_yt_dlp_fetch", lambda v, c, p: (_ for _ in ()).throw(RuntimeError("b")))
    import pytest
    with pytest.raises(T.TranscriptUnavailable):
        T.fetch_transcript("VID12345678")
```

- [ ] **Step 2: Run, verify it fails** (`-k fetch_`) — FAIL: `_fast_path`/`fetch_transcript`/`TranscriptUnavailable` undefined.

- [ ] **Step 3: Implement (append to transcript.py)**
```python
import os


class TranscriptUnavailable(Exception):
    pass


def _fast_path(vid: str, lang: str = "en") -> str:
    """youtube-transcript-api — works without a PoToken when the IP isn't blocked."""
    from youtube_transcript_api import YouTubeTranscriptApi
    fetched = YouTubeTranscriptApi().fetch(vid, languages=[lang, "en"])
    text = " ".join(snippet.text for snippet in fetched if snippet.text.strip())
    text = re.sub(r"\s+", " ", text).strip()
    if not text:
        raise RuntimeError("fast-path returned empty transcript")
    return text


def fetch_transcript(url: str, lang: str = "en") -> str:
    """Return plain-text transcript. Fast-path first, then yt-dlp (+POT/cookies/proxy)."""
    vid = video_id(url)
    cookies = os.environ.get("YT_COOKIES_FILE") or None
    proxy = os.environ.get("YT_PROXY") or None
    errors = []
    try:
        return _fast_path(vid, lang)
    except Exception as e:  # noqa: BLE001 — escalate to yt-dlp
        errors.append(f"fast-path: {e}")
    try:
        return _yt_dlp_fetch(vid, cookies, proxy)
    except Exception as e:  # noqa: BLE001
        errors.append(f"yt-dlp: {e}")
    raise TranscriptUnavailable(f"{vid}: " + " | ".join(errors))
```

- [ ] **Step 4: Run, verify pass** (full suite, non-live)
Run: `cd /c/tmp/yousummary/ops/yt_analyst && PYTHONPATH=.. .venv/bin/python -m pytest -q -m "not live"`
Expected: PASS (all unit tests, 0 fail).

- [ ] **Step 5: Commit**
```bash
cd /c/tmp/yousummary && git add ops/yt_analyst/transcript.py ops/yt_analyst/tests/ && git commit -q -m "feat: yt_analyst.fetch_transcript orchestrator (fast-path -> yt-dlp)"
```

---

## Task 6: CLI entry + a `@live` smoke test

**Files:** Create `ops/yt_analyst/__main__.py`; append a live test.

- [ ] **Step 1: Create `ops/yt_analyst/__main__.py`**
```python
import sys
from .transcript import fetch_transcript, TranscriptUnavailable

def main() -> int:
    if len(sys.argv) != 2:
        print("usage: python -m yt_analyst <youtube-url>", file=sys.stderr)
        return 2
    try:
        text = fetch_transcript(sys.argv[1])
    except (TranscriptUnavailable, ValueError) as e:
        print(f"ERROR: {e}", file=sys.stderr)
        return 1
    print(text)
    return 0

if __name__ == "__main__":
    raise SystemExit(main())
```

- [ ] **Step 2: Append a live smoke test (skipped by default)**
```python
import pytest

@pytest.mark.live
@pytest.mark.parametrize("vid", ["jlK7UWHD3sY", "xtKMO_ZH3Qk", "4WT7FXJah2I"])
def test_live_fetch(vid):
    text = T.fetch_transcript(vid)
    assert len(text) > 200  # a real transcript, not an empty/blocked response
```

- [ ] **Step 3: Run non-live suite to confirm nothing broke**
Run: `cd /c/tmp/yousummary/ops/yt_analyst && PYTHONPATH=.. .venv/bin/python -m pytest -q -m "not live"`
Expected: PASS.

- [ ] **Step 4: Commit**
```bash
cd /c/tmp/yousummary && git add ops/yt_analyst/__main__.py ops/yt_analyst/tests/ && git commit -q -m "feat: yt_analyst CLI + live smoke test" && git push origin main
```

---

## Task 7: Box infrastructure + live verification (ops)

**Files:** none in-repo; box services. This is where the datacenter-IP risk surfaces.

- [ ] **Step 1: Install yt-dlp + Python deps on the box**
```bash
ssh root@personal-projectsjl 'set -e
  python3 -m pip install --break-system-packages -q "yt-dlp>=2025.05.22" "youtube-transcript-api>=1.2.4"
  yt-dlp --version'
```
Expected: prints a yt-dlp version date ≥ 2025.05.22.

- [ ] **Step 2: Run the bgutil PoToken provider (Rust binary) as a service on :4416**
```bash
ssh root@personal-projectsjl 'set -e
  docker rm -f bgutil-pot 2>/dev/null || true
  docker run -d --name bgutil-pot --restart unless-stopped \
    -p 127.0.0.1:4416:4416 ghcr.io/jim60105/bgutil-ytdlp-pot-provider:latest
  sleep 3
  curl -s -o /dev/null -w "POT provider /ping -> %{http_code}\n" http://127.0.0.1:4416/ping'
```
Expected: `200`. (If the `-rs` image tag differs, list tags at the repo and adjust; the Node image `brainicism/bgutil-ytdlp-pot-provider` is an equivalent fallback.)

- [ ] **Step 3: Clone the repo on the box + live-fetch the three videos**
```bash
ssh root@personal-projectsjl 'set -e
  git -C /opt/yousummary pull --ff-only origin main
  cd /opt/yousummary/ops
  for v in jlK7UWHD3sY xtKMO_ZH3Qk 4WT7FXJah2I; do
    echo "=== $v ===";
    python3 -m yt_analyst "$v" 2>&1 | head -c 160; echo;
  done'
```
Expected (success): each prints transcript text. **Expected (likely first run): `RequestBlocked`/empty** — the datacenter-IP block.

- [ ] **Step 4: If blocked — escalate with cookies/proxy (REQUIRES OWNER INPUT)**
🛑 If Step 3 shows blocked/empty, STOP and ask the owner for ONE of:
  - a Firefox `cookies.txt` (from a burner Google account on a residential connection) placed at `/root/yt-cookies.txt` → set `YT_COOKIES_FILE=/root/yt-cookies.txt`; or
  - a residential proxy URL → set `YT_PROXY=...`.
Then re-run Step 3 with the env var(s) exported. Do not attempt further workarounds without the owner — this is an external constraint, not a code bug.

- [ ] **Step 5: Record outcome**
Once at least the fast-path or yt-dlp path returns real transcripts for the three videos, note in `docs/answering-agent.md`-style doc which lever was needed (none / cookies / proxy). Phase 1 is complete when `python -m yt_analyst <url>` returns real text on the box.

---

## Self-review notes
- Covers spec §3 (yt-dlp + bgutil-rs + youtube-transcript-api fast-path), §5.1 (transcript service + cookies/proxy env), §10 (datacenter-IP risk made an explicit gated step). Prompts/worker/UI/ranking/tests are later phases (separate plans), per the phasing in spec §11.
- No placeholders: every step has concrete code/commands. `video_id`, `parse_json3`, `build_yt_dlp_cmd`, `_yt_dlp_fetch`, `_fast_path`, `fetch_transcript`, `TranscriptUnavailable` are defined where referenced and names are consistent across tasks.
- Note: `youtube-transcript-api` v1.x API (`YouTubeTranscriptApi().fetch(...)` returning snippet objects with `.text`) — if the installed minor version differs, adjust `_fast_path` accordingly; the orchestrator + tests do not depend on its internals (monkeypatched).
