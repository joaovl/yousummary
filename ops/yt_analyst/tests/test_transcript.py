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
