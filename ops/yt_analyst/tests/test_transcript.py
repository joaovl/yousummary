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
