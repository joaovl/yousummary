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

import pytest

@pytest.mark.live
@pytest.mark.parametrize("vid", ["jlK7UWHD3sY", "xtKMO_ZH3Qk", "4WT7FXJah2I"])
def test_live_fetch(vid):
    text = T.fetch_transcript(vid)
    assert len(text) > 200  # a real transcript, not an empty/blocked response
