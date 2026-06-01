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
