# Phase 2 — Analysis Queue + /analyze UI + Box Agentic Worker

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development to implement task-by-task. Steps use `- [ ]` checkboxes.

**Goal:** End-to-end single-video analysis: a `/analyze` page (URL **or pasted transcript** + Mode + Depth + Intent + Custom-bypass) enqueues a job; a box-side agentic Claude worker (Max sub, WebFetch/WebSearch, read-only) produces a Summary or doc-verified Tutorial guide and posts it back; the UI polls and renders it.

**Architecture:** Extend the existing Node gate (`deploy/gate/index.js`) with an `analysis_jobs` table + public routes + token-gated agent routes (mirrors `compare_jobs`). New Python worker `ops/yousummary_analyst.py` mirrors `avionics_agent.py` but is **agentic** (Claude with read-only web tools) and imports `yt_analyst` for URL transcripts; pasted transcripts skip fetching. Prompts adapted from Fabric live in `ops/prompts/`.

**Tech Stack:** Node/express + better-sqlite3 (gate), Python 3 + Claude CLI (worker), systemd.

**Conventions:** commits end with `Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>`. Repo `C:\tmp\yousummary`, branch `main`. Box: `root@personal-projectsjl`.

---

## File Structure

| File | Responsibility |
|---|---|
| `deploy/gate/index.js` (modify) | `analysis_jobs` table + `/analyze`, `/api/analyze`, `/api/analyze/:id`, `/api/agent/analysis-jobs[/:id/claim|/resolve]` |
| `deploy/gate/analyze_page.js` (create) | exports the `/analyze` HTML string (keeps index.js lean) |
| `ops/prompts/summary.md` (create) | summary prompt (depth-parameterised) |
| `ops/prompts/tutorial.md` (create) | tutorial guide + command-verification protocol |
| `ops/yt_analyst/analyst.py` (create) | `build_prompt(job)`, `model_for(depth)`, `run_claude(prompt, model, allow_web)` |
| `ops/yt_analyst/worker.py` (create) | poll→claim→process→resolve loop against the gate |
| `ops/yt_analyst/tests/test_analyst.py` (create) | unit tests for `build_prompt`/`model_for` |
| `ops/systemd/yousummary-analyst.service` (create) | systemd unit for the worker |

---

## Task 1: `analysis_jobs` schema + enqueue/poll routes (gate)

**Files:** modify `deploy/gate/index.js`.

- [ ] **Step 1: Add the table** — after the existing `compare_jobs` `db.exec(...)` block, add:
```js
db.exec(`
  CREATE TABLE IF NOT EXISTS analysis_jobs (
    id            INTEGER PRIMARY KEY AUTOINCREMENT,
    urls_json     TEXT NOT NULL,
    transcript    TEXT,
    mode          TEXT NOT NULL DEFAULT 'auto',
    depth         TEXT NOT NULL DEFAULT 'medium',
    intent        TEXT,
    custom        TEXT,
    status        TEXT NOT NULL DEFAULT 'pending',
    result_md     TEXT,
    result_html   TEXT,
    error         TEXT,
    created_at    TEXT NOT NULL DEFAULT (datetime('now')),
    claimed_at    TEXT,
    resolved_at   TEXT
  );
  CREATE INDEX IF NOT EXISTS idx_analysis_status ON analysis_jobs(status);
`);
```

- [ ] **Step 2: Add validation constants + enqueue route** (near the compare routes):
```js
const MODES = new Set(["auto", "summary", "tutorial", "compare-extract", "rank"]);
const DEPTHS = new Set(["quick", "medium", "comprehensive"]);

// Submit (gated by Olimpus): URL(s) and/or pasted transcript.
app.post("/api/analyze", gateMiddleware, (req, res) => {
  const b = req.body || {};
  const urls = Array.isArray(b.urls) ? b.urls.map(String).map(s => s.trim()).filter(Boolean) : [];
  const transcript = b.transcript ? String(b.transcript).slice(0, 200000) : null;
  if (urls.length === 0 && !transcript) {
    return res.status(400).json({ detail: "provide at least one URL or a pasted transcript" });
  }
  if (urls.length > MAX_COMPARE_URLS) return res.status(400).json({ detail: `max ${MAX_COMPARE_URLS} URLs` });
  let mode = String(b.mode || "auto"); if (!MODES.has(mode)) mode = "auto";
  let depth = String(b.depth || "medium"); if (!DEPTHS.has(depth)) depth = "medium";
  if (urls.length > 1 && mode === "auto") mode = "rank";
  const intent = b.intent ? String(b.intent).slice(0, 2000) : null;
  const custom = b.custom ? String(b.custom).slice(0, 4000) : null;
  const info = db.prepare(
    "INSERT INTO analysis_jobs (urls_json, transcript, mode, depth, intent, custom) VALUES (?,?,?,?,?,?)"
  ).run(JSON.stringify(urls), transcript, mode, depth, intent, custom);
  return res.status(201).json({ id: Number(info.lastInsertRowid), status: "pending" });
});

// Poll (gated): status + result.
app.get("/api/analyze/:id", gateMiddleware, (req, res) => {
  const id = Number(req.params.id);
  if (!Number.isInteger(id) || id <= 0) return res.status(400).json({ detail: "bad id" });
  const row = db.prepare(
    "SELECT id, urls_json, mode, depth, status, result_html, error, created_at, resolved_at FROM analysis_jobs WHERE id=?"
  ).get(id);
  if (!row) return res.status(404).json({ detail: "job not found" });
  res.json({ ...row, urls: JSON.parse(row.urls_json) });
});
```

- [ ] **Step 3: Register `/analyze` page route** (import the page module at top of file: `import { ANALYZE_HTML } from "./analyze_page.js";`) and add:
```js
app.get("/analyze", gateMiddleware, (_req, res) =>
  res.set("content-type", "text/html; charset=utf-8").send(ANALYZE_HTML));
```

- [ ] **Step 4: Whitelist the new gated paths** — the catch-all `app.use` already routes unknown paths through `gateMiddleware`; confirm `/api/analyze` and `/analyze` are NOT in `PUBLIC_PATHS` (they must stay gated). No change needed if they aren't listed.

- [ ] **Step 5: Commit**
```bash
cd /c/tmp/yousummary && git add deploy/gate/index.js && git commit -q -m "feat(gate): analysis_jobs table + /analyze enqueue & poll routes"
```

---

## Task 2: Agent endpoints for the worker (gate)

**Files:** modify `deploy/gate/index.js`.

- [ ] **Step 1: Add list/claim/resolve under the existing `agentAuth`** (mirrors `/api/agent/jobs`):
```js
app.get("/api/agent/analysis-jobs", agentAuth, (_req, res) => {
  const rows = db.prepare(
    "SELECT id, urls_json, transcript, mode, depth, intent, custom, created_at FROM analysis_jobs WHERE status='pending' ORDER BY id"
  ).all();
  res.json({ count: rows.length, jobs: rows.map(r => ({ ...r, urls: JSON.parse(r.urls_json) })) });
});

app.post("/api/agent/analysis-jobs/:id/claim", agentAuth, (req, res) => {
  const id = Number(req.params.id);
  const r = db.prepare(
    "UPDATE analysis_jobs SET status='processing', claimed_at=datetime('now') WHERE id=? AND status='pending'"
  ).run(id);
  if (r.changes === 0) return res.status(409).json({ detail: "not claimable" });
  res.json({ ok: true });
});

app.post("/api/agent/analysis-jobs/:id/resolve", agentAuth, (req, res) => {
  const id = Number(req.params.id);
  const status = String(req.body?.status ?? "");
  if (!["done", "failed"].includes(status)) return res.status(400).json({ detail: "status must be done|failed" });
  const result_md = req.body?.result_md ? String(req.body.result_md) : null;
  const result_html = req.body?.result_html ? String(req.body.result_html) : null;
  const error = req.body?.error ? String(req.body.error).slice(0, 4000) : null;
  if (status === "done" && !result_html) return res.status(400).json({ detail: "result_html required when done" });
  const r = db.prepare(
    `UPDATE analysis_jobs SET status=?, result_md=?, result_html=?, error=?, resolved_at=datetime('now')
       WHERE id=? AND status IN ('processing','pending')`
  ).run(status, result_md, result_html, error, id);
  if (r.changes === 0) return res.status(404).json({ detail: "job not found or already resolved" });
  res.json({ ok: true });
});
```
Note: `/api/agent/` is already exempt from the TOTP gate and protected by `agentAuth` (token) inside the handler — same as the existing job endpoints.

- [ ] **Step 2: Commit**
```bash
cd /c/tmp/yousummary && git add deploy/gate/index.js && git commit -q -m "feat(gate): agent list/claim/resolve for analysis_jobs"
```

---

## Task 3: `/analyze` page (gate)

**Files:** create `deploy/gate/analyze_page.js`.

- [ ] **Step 1: Create the page** (self-contained; reuses the compare-poll JS pattern):
```js
export const ANALYZE_HTML = `<!doctype html><meta charset=utf-8><title>Analyze · yousummary</title>
<style>body{font-family:system-ui;max-width:920px;margin:2rem auto;padding:0 1rem;color:#111}
label{display:block;margin-top:1rem;font-weight:600}
input,textarea,select{width:100%;padding:.5rem;box-sizing:border-box;font:inherit}
textarea{min-height:90px;font:14px ui-monospace,Menlo,monospace}
.row{display:flex;gap:1rem}.row>div{flex:1}
button{margin-top:1rem;padding:.6rem 1.2rem;font-size:1rem}
.muted{color:#666;font-size:.9rem}.err{color:tomato;white-space:pre-wrap}
#out{margin-top:1.5rem}.spinner{display:inline-block;width:1em;height:1em;border:2px solid #ccc;border-top-color:#222;border-radius:50%;animation:spin .8s linear infinite;vertical-align:-.15em}@keyframes spin{to{transform:rotate(360deg)}}
table{border-collapse:collapse;width:100%}th,td{border:1px solid #ddd;padding:.5rem .7rem;text-align:left;vertical-align:top}th{background:#f5f5f5}</style>
<h1>Analyze videos</h1>
<p class=muted>Add one or more YouTube URLs (one per line) and/or paste a transcript. Several URLs → ranked comparison.</p>
<form id=f>
<label>YouTube URL(s)<textarea id=urls placeholder="https://www.youtube.com/watch?v=...&#10;https://youtu.be/..."></textarea></label>
<div class=row>
<div><label>Mode<select id=mode>
<option value=auto>Auto</option><option value=summary>Summary</option><option value=tutorial>Tutorial (verify commands)</option><option value=compare-extract>Comparison-extract</option><option value=rank>Rank multiple</option></select></div>
<div><label>Depth<select id=depth><option value=quick>Quick</option><option value=medium selected>Medium</option><option value=comprehensive>Comprehensive</option></select></div>
</div>
<label>What are you looking for? (optional)<input id=intent placeholder="e.g. set up X on Windows"></label>
<label>Paste transcript (optional — bypasses URL fetch)<textarea id=transcript placeholder="paste a transcript here to skip YouTube fetching"></textarea></label>
<label>Custom instructions (optional — overrides Mode/Depth)<textarea id=custom placeholder="free-form instructions for one-off asks"></textarea></label>
<button type=submit id=go>Analyze</button> <span id=status class=muted></span>
</form>
<div id=out></div>
<script>
const $=id=>document.getElementById(id);let timer=null;
async function poll(id){try{const r=await fetch('/api/analyze/'+id);const j=await r.json();
if(j.status==='done'){clearInterval(timer);$('status').textContent='done (#'+id+')';$('out').innerHTML=j.result_html||'<p class=muted>empty</p>';$('go').disabled=false;}
else if(j.status==='failed'){clearInterval(timer);$('status').innerHTML='<span class=err>failed: '+(j.error||'')+'</span>';$('go').disabled=false;}
else{$('status').innerHTML='<span class=spinner></span> #'+id+' '+j.status+'…';}}catch(e){}}
$('f').onsubmit=async ev=>{ev.preventDefault();$('out').innerHTML='';$('go').disabled=true;$('status').innerHTML='<span class=spinner></span> submitting…';
const urls=$('urls').value.split(/\\r?\\n/).map(s=>s.trim()).filter(Boolean);
const payload={urls,mode:$('mode').value,depth:$('depth').value,intent:$('intent').value||null,transcript:$('transcript').value||null,custom:$('custom').value||null};
try{const r=await fetch('/api/analyze',{method:'POST',headers:{'content-type':'application/json'},body:JSON.stringify(payload)});
const j=await r.json();if(!r.ok){$('status').innerHTML='<span class=err>'+(j.detail||r.status)+'</span>';$('go').disabled=false;return;}
$('status').innerHTML='<span class=spinner></span> queued #'+j.id+'…';poll(j.id);timer=setInterval(()=>poll(j.id),3000);}
catch(e){$('status').innerHTML='<span class=err>'+e.message+'</span>';$('go').disabled=false;}};
</script>`;
```

- [ ] **Step 2: Commit**
```bash
cd /c/tmp/yousummary && git add deploy/gate/analyze_page.js && git commit -q -m "feat(gate): /analyze page (URL or paste, mode/depth/intent/custom)"
```

---

## Task 4: Prompts (Fabric-adapted)

**Files:** create `ops/prompts/summary.md`, `ops/prompts/tutorial.md`.

- [ ] **Step 1: `ops/prompts/summary.md`**
```
You summarise a video from its transcript. Output clean Markdown only — no preamble.
Depth "quick": 3–5 bullets. "medium": a short intro + key sections as bullets. "comprehensive": intro + sectioned detail + a "Key takeaways" list.
If an intent is given, focus the summary on it. Do not invent content not in the transcript.
```

- [ ] **Step 2: `ops/prompts/tutorial.md`**
```
You turn a tutorial video transcript into a step-by-step guide in Markdown. Output Markdown only.
For every command, setting, or version-specific claim: verify it against the tool's OFFICIAL docs using your web tools (Context7 if available, else WebFetch the official page). Tag each as:
  [confirmed] / [changed: <correction>] / [deprecated] / [unverified]
and cite the doc URL. Then present the corrected step. End with a "References" section (doc links) and a "What to double-check" note for anything unverified.
Depth scales how many commands you verify (quick: none; medium: key ones; comprehensive: all). Honour the user's intent if given.
```

- [ ] **Step 3: Commit**
```bash
cd /c/tmp/yousummary && git add ops/prompts && git commit -q -m "feat: summary + tutorial prompts (Fabric-adapted, with verification protocol)"
```

---

## Task 5: `analyst.py` — prompt builder + claude runner (TDD)

**Files:** create `ops/yt_analyst/analyst.py`; test `ops/yt_analyst/tests/test_analyst.py`.

- [ ] **Step 1: Write the failing test**
```python
from yt_analyst.analyst import model_for, build_prompt

def test_model_for():
    assert model_for("quick") == "sonnet"
    assert model_for("medium") == "opus"
    assert model_for("comprehensive") == "opus"
    assert model_for("bogus") == "opus"

def test_build_prompt_custom_overrides():
    job = {"mode": "summary", "depth": "quick", "intent": "x", "custom": "JUST DO THIS"}
    p = build_prompt(job, transcript="T", rules={"summary": "S", "tutorial": "U"})
    assert "JUST DO THIS" in p and "T" in p

def test_build_prompt_tutorial_uses_rules_and_depth_intent():
    job = {"mode": "tutorial", "depth": "comprehensive", "intent": "set up X", "custom": None}
    p = build_prompt(job, transcript="TRANSCRIPT", rules={"summary": "S", "tutorial": "TUT-RULES"})
    assert "TUT-RULES" in p
    assert "comprehensive" in p
    assert "set up X" in p
    assert "TRANSCRIPT" in p
```

- [ ] **Step 2: Run, verify fail** — `cd ops/yt_analyst && PYTHONPATH=.. .venv/<py> -m pytest tests/test_analyst.py -q` → FAIL (module missing).

- [ ] **Step 3: Implement `ops/yt_analyst/analyst.py`**
```python
"""Prompt assembly + Claude CLI invocation for the analysis worker."""
from __future__ import annotations
import subprocess

_MODEL = {"quick": "sonnet", "medium": "opus", "comprehensive": "opus"}


def model_for(depth: str) -> str:
    return _MODEL.get(depth, "opus")


def build_prompt(job: dict, transcript: str, rules: dict) -> str:
    """Assemble the worker prompt. Custom instructions override the structured mode."""
    depth = job.get("depth", "medium")
    intent = (job.get("intent") or "").strip()
    custom = (job.get("custom") or "").strip()
    intent_line = f"\nUser intent: {intent}\n" if intent else ""
    if custom:
        head = f"Follow these instructions exactly:\n{custom}\n"
    else:
        mode = job.get("mode", "auto")
        rule = rules.get("tutorial" if mode == "tutorial" else "summary", rules.get("summary", ""))
        head = f"{rule}\nDepth: {depth}.{intent_line}"
    return f"{head}\n\n--- TRANSCRIPT ---\n{transcript}\n--- END TRANSCRIPT ---\n"


def run_claude(prompt: str, model: str, allow_web: bool, timeout: int = 600) -> str:
    """Invoke the Max-sub Claude CLI. Read-only web tools when allow_web."""
    tools = "WebFetch WebSearch" if allow_web else ""
    cmd = ["claude", "-p", "--output-format", "text", "--model", model]
    if tools:
        cmd += ["--allowedTools", tools]
    p = subprocess.run(cmd, input=prompt, capture_output=True, text=True, timeout=timeout)
    if p.returncode != 0:
        raise RuntimeError(f"claude rc={p.returncode}: {(p.stderr or '').strip()[:300]}")
    out = (p.stdout or "").strip()
    if not out:
        raise RuntimeError("claude produced no output")
    return out
```

- [ ] **Step 4: Run, verify pass** — same pytest command → PASS (3 cases).

- [ ] **Step 5: Commit**
```bash
cd /c/tmp/yousummary && git add ops/yt_analyst/analyst.py ops/yt_analyst/tests/test_analyst.py && git commit -q -m "feat: analyst prompt builder + claude runner"
```

---

## Task 6: `worker.py` — poll/claim/process/resolve loop

**Files:** create `ops/yt_analyst/worker.py`.

- [ ] **Step 1: Implement** (mirrors `avionics_agent.py`; gets the token from `/root/yousummary.env`):
```python
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
         "tutorial": (PROMPTS / "tutorial.md").read_text(encoding="utf-8")}


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
```

- [ ] **Step 2: Run the non-live unit suite (worker imports must resolve)**
Run: `cd /c/tmp/yousummary/ops/yt_analyst && PYTHONPATH=.. .venv/<py> -m pytest -q -m "not live"`
Expected: PASS (Phase-1 tests + analyst tests).

- [ ] **Step 3: Commit + push**
```bash
cd /c/tmp/yousummary && git add ops/yt_analyst/worker.py && git commit -q -m "feat: yousummary analysis worker loop" && git push origin main
```

---

## Task 7: systemd unit + ranking placeholder note

**Files:** create `ops/systemd/yousummary-analyst.service`.

- [ ] **Step 1: Create the unit**
```ini
[Unit]
Description=yousummary analysis worker (agentic Claude, Max sub)
After=network-online.target docker.service
Wants=network-online.target

[Service]
Type=simple
User=root
WorkingDirectory=/opt/yousummary/ops
UnsetEnvironment=ANTHROPIC_API_KEY
Environment=GATE_BASE=http://127.0.0.1:8160
Environment=ANALYST_POLL_SECONDS=30
Environment=PYTHONPATH=/opt/yousummary/ops
ExecStart=/usr/bin/python3 -m yt_analyst.worker
Restart=always
RestartSec=10

[Install]
WantedBy=multi-user.target
```
Note: `rank`/`compare-extract` modes are handled in Phase 3 (they extend `build_prompt` + `process`); for Phase 2 they fall through to the summary prompt, which is acceptable until Phase 3 lands.

- [ ] **Step 2: Commit + push**
```bash
cd /c/tmp/yousummary && git add ops/systemd/yousummary-analyst.service && git commit -q -m "feat: yousummary-analyst systemd unit" && git push origin main
```

---

## Self-review notes
- Covers spec §4 (gate queue + agentic worker), §5.2/5.3 (routes, worker, Max-sub, read-only web), §5.4 (prompts), §6 (mode/depth/intent/custom; multi-URL→rank; custom overrides), §8 (depth→model + web budget via `allow_web`, transcript truncation, one-at-a-time). Ranking detail = Phase 3; Playwright = Phase 4 (separate plans).
- Names consistent: `analysis_jobs`, `build_prompt(job, transcript, rules)`, `model_for(depth)`, `run_claude(prompt, model, allow_web)`, `process(job)`, agent routes `/api/agent/analysis-jobs[/:id/claim|/resolve]`.
- `_md_to_html` is deliberately minimal (owner-only content behind Olimpus); a richer Markdown renderer can come with Phase 4 polish.
- Deploy of this phase happens in Task #5 of the tracker (systemd enable + heartbeat) alongside Phases 3–4.
```
