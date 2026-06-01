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
