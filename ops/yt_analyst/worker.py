"""yousummary analysis worker: drains analysis_jobs via the gate, runs Claude."""
from __future__ import annotations
import json, os, time, urllib.error, urllib.request
from pathlib import Path
from .transcript import fetch_transcript, TranscriptUnavailable
from .analyst import model_for, build_prompt, run_claude

BASE = os.environ.get("GATE_BASE", "http://127.0.0.1:8160")
POLL = int(os.environ.get("ANALYST_POLL_SECONDS", "30"))
ENV_FILE = os.environ.get("YOUSUMMARY_ENV_FILE", "/root/yousummary.env")
PROMPTS = Path(os.environ.get("ANALYST_PROMPTS", str(Path(__file__).resolve().parents[1] / "prompts")))


def _token() -> str | None:
    try:
        for line in open(ENV_FILE):
            if line.startswith("YOUSUMMARY_AGENT_TOKEN="):
                return line.split("=", 1)[1].strip()
    except OSError:
        pass
    return None


TOKEN = _token() or ""
RULES = {"summary": (PROMPTS / "summary.md").read_text(encoding="utf-8"),
         "tutorial": (PROMPTS / "tutorial.md").read_text(encoding="utf-8"),
         "rank": (PROMPTS / "rank.md").read_text(encoding="utf-8"),
         "compare_extract": (PROMPTS / "compare_extract.md").read_text(encoding="utf-8"),
         "research": (PROMPTS / "research.md").read_text(encoding="utf-8"),
         "product_score": (PROMPTS / "product_score.md").read_text(encoding="utf-8")}


def log(m): print(f"[analyst] {time.strftime('%H:%M:%S', time.gmtime())} {m}", flush=True)


def _api(method, path, payload=None):
    url = f"{BASE}{path}?token={TOKEN}"
    data = json.dumps(payload).encode() if payload is not None else None
    req = urllib.request.Request(url, data=data, method=method,
                                headers={"Content-Type": "application/json"} if data else {})
    try:
        with urllib.request.urlopen(req, timeout=30) as r:
            return r.status, json.load(r) if r.length != 0 else {}
    except urllib.error.HTTPError as e:
        return e.code, {"detail": e.read().decode()[:200]}
    except Exception as e:
        return None, {"detail": str(e)}


def _md_to_html(md: str) -> str:
    # minimal, safe-enough renderer; the gate already serves owner-only content.
    import html
    return "<div class=md><pre style='white-space:pre-wrap'>" + html.escape(md) + "</pre></div>"


def process(job: dict) -> tuple[str, str]:
    urls = job.get("urls") or []
    transcript = (job.get("transcript") or "").strip()
    if not transcript:
        if not urls:
            raise RuntimeError("no transcript and no URL")
        if job.get("mode") == "rank" or len(urls) > 1:
            blocks = []
            for n, url in enumerate(urls, 1):
                try:
                    text = fetch_transcript(url)
                except Exception as e:  # noqa: BLE001 — one bad url shouldn't abort the job
                    text = f"[transcript unavailable: {e}]"
                blocks.append(f"## Video {n} — {url}\n{text}\n")
            transcript = "\n".join(blocks)
        else:
            transcript = fetch_transcript(urls[0])
    depth = job.get("depth", "medium")
    prompt = build_prompt(job, transcript=transcript[:120000], rules=RULES)
    md = run_claude(prompt, model=model_for(depth), allow_web=(depth != "quick"))
    return md, _md_to_html(md)


def main():
    if not TOKEN:
        log("no YOUSUMMARY_AGENT_TOKEN — exiting"); raise SystemExit(1)
    log(f"loop start base={BASE} poll={POLL}s")
    fails = {}
    while True:
        try:
            _, d = _api("GET", "/api/agent/analysis-jobs")
            for job in sorted(d.get("jobs", []), key=lambda x: x["id"]):
                jid = job["id"]
                if fails.get(jid, 0) >= 3:
                    continue
                code, _ = _api("POST", f"/api/agent/analysis-jobs/{jid}/claim")
                if code != 200:
                    continue
                log(f"processing #{jid} mode={job.get('mode')} depth={job.get('depth')}")
                try:
                    md, html_ = process(job)
                    _api("POST", f"/api/agent/analysis-jobs/{jid}/resolve",
                         {"status": "done", "result_md": md, "result_html": html_})
                    log(f"#{jid} done ({len(md)} chars)")
                except (TranscriptUnavailable, Exception) as e:  # noqa: BLE001
                    fails[jid] = fails.get(jid, 0) + 1
                    _api("POST", f"/api/agent/analysis-jobs/{jid}/resolve",
                         {"status": "failed", "error": str(e)[:1000]})
                    log(f"#{jid} failed: {str(e)[:160]}")
        except Exception as e:  # noqa: BLE001
            log(f"cycle error: {e}")
        time.sleep(POLL)


if __name__ == "__main__":
    main()
