# yousummary video analysis (`/analyze`)

Agentic video analysis running on the box, on the Anthropic **Max subscription**
(no metered key). Reached at `https://yousummary.joaovpl.uk/analyze` (behind the
Olimpus gate — unlock with the shared Authenticator code).

## What it does
Submit **one or more YouTube URLs and/or a pasted transcript**, choose **Mode**
and **Depth**, optionally say what you're looking for, and get:
- **Summary** — generic videos.
- **Tutorial** — step-by-step guide whose commands are **verified against the
  tool's official docs** (web), tagged `[confirmed]/[changed]/[deprecated]/
  [unverified]` with references.
- **Comparison-extract** — pulls what a single comparison video compares into a table.
- **Rank** (auto when ≥2 URLs) — a ranked table (top vs weak) with concrete,
  doc-verified red flags.

Controls: **Mode** (Auto/Summary/Tutorial/Comparison-extract/Rank), **Depth**
(Quick→sonnet,no web · Medium/Comprehensive→opus,web verification), an **intent**
box, and a **Custom instructions** box that overrides Mode/Depth for one-off asks.

## Architecture
- **Gate** (`deploy/gate/`, Node): `/analyze` page + `analysis_jobs` queue +
  token-gated agent endpoints (`/api/agent/analysis-jobs[/:id/claim|/resolve]`).
- **Worker** (`ops/yt_analyst/worker.py`, systemd `yousummary-analyst.service`):
  polls/claims jobs, fetches transcripts (`yt_analyst.transcript`), runs the
  Claude CLI agentically (read-only `WebFetch`/`WebSearch`, `UnsetEnvironment=
  ANTHROPIC_API_KEY` → Max sub), posts results back. Monitored by the avionics
  heartbeat.
- **Tests:** `deploy/gate/tests/e2e/` (Playwright/Chromium) — `npm run test:e2e`.

## Transcript ingestion (important)
- **Pasted transcript → always works** (no YouTube access needed).
- **YouTube URL auto-fetch → currently gated.** YouTube hard-blocks the Hetzner
  datacenter IP (yt-dlp bot-check, PoToken ineffective, public Invidious flaky).
  The fetch layer reads `YT_PROXY` / `YT_COOKIES_FILE` and lights up the moment
  one is configured — set on the worker service env, no code change. Until then,
  paste transcripts for URL videos.

## Operate
```bash
ssh root@personal-projectsjl 'systemctl status yousummary-analyst.service --no-pager | head -15'
ssh root@personal-projectsjl 'journalctl -u yousummary-analyst.service -n 40 --no-pager'
ssh root@personal-projectsjl 'systemctl restart yousummary-analyst.service'   # after prompt edits
ssh root@personal-projectsjl 'cat /run/avionics-heartbeat.status'             # health
```
Tune via the unit's `Environment=` (`ANALYST_POLL_SECONDS`, models live in
`ops/yt_analyst/analyst.py:model_for`). Prompts: `ops/prompts/*.md`.

Spec + plans: `docs/superpowers/specs/2026-06-01-agentic-video-analysis.md`,
`docs/superpowers/plans/2026-06-0{1,2}-*`.
