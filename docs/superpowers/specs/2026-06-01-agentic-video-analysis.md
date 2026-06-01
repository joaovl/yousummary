# Agentic Video Analysis for yousummary — Design Spec

> **Date:** 2026-06-01 · **Status:** Design, awaiting review
> **Repo:** `joaovl/yousummary` · **Audience:** implementer with no prior context.
> Personal-use tool (no commercial concern) — liberal reuse of open source is fine.

---

## 1. Goal

Turn yousummary into an agentic video-analysis tool. Given **one or more YouTube
URLs**, an optional **intent**, and a **depth** level, a box-side Claude worker
(Anthropic **Max subscription**, agentic with **web access**) produces the right
artifact for the video kind:

- **Generic video →** a summary (at the chosen depth).
- **Tutorial →** a step-by-step guide whose commands are **verified against the
  tool's official docs**, with outdated/wrong ones flagged and references added.
- **A comparison video** (one video comparing things) **→** a table extracting
  what it compares and the data it shows.
- **Multiple videos →** a **ranked comparison table** ("top" vs "weak"), where
  "what to look for in the bad ones" is concrete, doc-verified red flags.

It is for the owner only, reached through the private Olimpus gate.

## 2. Why now / what's broken

The current Rust transcript fetcher (ANDROID innertube client `v19.09.37` + direct
`timedtext`) is **dead**: YouTube returns HTTP 400 for that client and serves
empty `timedtext` without a PoToken, so summaries fail across most videos
(confirmed by reproduction on the box). The metered `ANTHROPIC_API_KEY` path also
contradicts the owner's preference to use the Max subscription via the `claude`
CLI. This design fixes both.

## 3. Reuse decisions (from the 2026 ecosystem research)

| Concern | Decision | Source |
|---|---|---|
| Transcript extraction | **yt-dlp** (the spine) + **`jim60105/bgutil-ytdlp-pot-provider-rs`** (GPL-3.0, Rust single binary, HTTP PoToken provider on `:4416`). `youtube-transcript-api` as a no-PoToken fast-path first attempt. | yt-dlp PO Token Guide; bgutil-rs |
| Anti-block (datacenter IP) | Firefox cookies (`--cookies`), and a **residential proxy** (`--proxy`) as the decisive lever. Pluggable, escalating. | yt-dlp FAQ; youtube-transcript-api docs |
| Summary / tutorial / scoring prompts | Adopt **Fabric** Markdown patterns (`youtube_summary`, `extract_wisdom`, `rate_content`) + martinopiaggi/summarize prompt styles. | danielmiessler/fabric |
| Doc-verification | **Context7 MCP** (`resolve-library-id` → `query-docs`) when available to the box `claude`, else the worker's **WebFetch/WebSearch**. | upstash/context7 |
| Multi-video ranking | **Build ourselves** (no maintained tool). Fixed rubric, structured after **PAIR llm-comparator** + Fabric `rate_content`. | PAIR-code/llm-comparator |

We **do not extend the Rust transcript code** and **do not reinvent prompts**; we
own the orchestration, ranking, and doc-verification logic.

## 4. Architecture

```
Browser (private, behind Olimpus)
   │  new tab: /analyze  (URLs + Mode + Depth + Intent + Custom-bypass)
   ▼
yousummary-gate  (Node/express — we edit this)
   │  POST /api/analyze  -> enqueue analysis_jobs row (sqlite)
   │  GET  /api/analyze/:id -> poll status/result
   │  GET  /api/agent/analysis-jobs        (token-gated, for the worker)
   │  POST /api/agent/analysis-jobs/:id/claim   /resolve
   ▼  (token-gated agent endpoints, localhost)
yousummary-analyst worker  (box, systemd — new; mirrors avionics-agent)
   │  1. claim oldest pending job
   │  2. fetch transcript(s):  youtube-transcript-api fast-path
   │       └─ fallback: yt-dlp --write-auto-subs --sub-format json3 (uses POT provider :4416, cookies, proxy)
   │  3. run Claude CLI (Max sub), AGENTIC with read-only web tools:
   │       claude -p --model <by depth> --allowedTools "WebFetch WebSearch [+Context7]"
   │       (prompt = mode + depth + intent + custom; transcript(s) inline)
   │  4. POST result (markdown + rendered HTML) back to the gate
   ▼
bgutil-ytdlp-pot-provider-rs  (box, systemd, HTTP :4416)  ── yt-dlp auto-detects it
```

**Why this shape:** the gate already has the exact queue+agent-endpoint pattern
(`compare_jobs`), and the box already runs Claude on the Max sub via systemd
(`claude-on-box.md`) with a heartbeat. The worker needs only **read-only web
tools** (WebFetch/WebSearch/Context7) — **no shell** — so it's safe (the worker
script, not Claude, runs yt-dlp and the HTTP I/O). The Rust app is untouched by
this feature; the legacy metered `/api/summarize` can be retired later.

## 5. Components

### 5.1 Transcript service (box)
- `bgutil-ytdlp-pot-provider-rs` as a systemd service on `127.0.0.1:4416`.
- `yt-dlp` installed on the box (or in the worker's environment).
- A worker helper `fetch_transcript(url)` that: (1) tries `youtube-transcript-api`;
  (2) on failure runs `yt-dlp --skip-download --write-subs --write-auto-subs
  --sub-langs en --sub-format json3 --extractor-args "youtube:player_client=mweb"
  [--cookies <firefox.txt>] [--proxy <residential>] <url>` and parses json3.
- Config via env: `YT_COOKIES_FILE`, `YT_PROXY` (both optional, escalating).

### 5.2 Gate: queue + UI (Node, in `deploy/gate/`)
- **Table `analysis_jobs`:** `id, urls_json, mode, depth, intent, custom_instructions,
  status('pending'|'processing'|'done'|'failed'), result_md, result_html, error,
  created_at, claimed_at, resolved_at`.
- **Public (gated by Olimpus) routes:** `GET /analyze` (the page), `POST /api/analyze`
  (validate + enqueue), `GET /api/analyze/:id` (poll).
- **Agent routes (token-gated, localhost only):** `GET /api/agent/analysis-jobs`
  (list pending), `POST /api/agent/analysis-jobs/:id/claim` (pending→processing,
  atomic), `POST /api/agent/analysis-jobs/:id/resolve` (done|failed + result).
- Reuse `/compare`'s polling JS and the existing `agentAuth` middleware.

### 5.3 Worker (box, new — `ops/yousummary_analyst.py`, systemd `yousummary-analyst.service`)
- Mirrors `avionics_agent.py`: poll → claim → process → resolve, `MAX_FAILS` guard,
  one job at a time. Uses the Max-sub OAuth (`UnsetEnvironment=ANTHROPIC_API_KEY`).
- Claude invocation: `claude -p --output-format text --model <model> --allowedTools "WebFetch WebSearch"`
  (append Context7 MCP tools if the box `claude` has the server configured).
- Added to the heartbeat's `HEARTBEAT_UNITS`.

### 5.4 Prompts (`ops/prompts/`, adapted from Fabric)
- `summary.md`, `tutorial.md` (with the command-verification protocol),
  `compare_extract.md`, `rank.md` (rubric: accuracy-vs-docs, completeness, clarity,
  recency). Each parameterised by depth + intent.

## 6. Inputs & behaviour matrix

**Mode** (`Auto` default): `Auto | Summary | Tutorial | Compare-extract | Rank`.
In `Auto`, the worker classifies the video(s) from title/metadata/transcript.
Multiple URLs ⇒ `Rank` regardless.

**Depth** controls model + web budget + output length (a Max-quota guardrail):

| Depth | Model | Web verification | Output |
|---|---|---|---|
| Quick | sonnet | none | short summary / simple table |
| Medium | opus | top commands / key claims only (≤ ~5 fetches) | structured guide / table |
| Comprehensive | opus | full (≤ ~15 fetches), with citations | detailed guide + references / detailed ranked table with per-item reasoning |

**Intent** (optional text): focuses summary/ranking ("set up X on Windows").

**Custom instructions (the bypass):** if non-empty, it becomes the worker's
primary instruction; Mode/Depth become hints only. Power-user escape hatch.

## 7. Doc-verification protocol (Tutorial / Rank)
For each command or factual claim the video makes: resolve the tool's docs
(Context7 `resolve-library-id`+`query-docs`, else WebFetch the official page),
then classify **confirmed / changed / deprecated / unverified**, emit the
corrected command, and cite the doc URL. Output tags each item with its evidence
status so the owner sees what's trustworthy.

## 8. Cost / safety guardrails (Max subscription)
- One job processed at a time; transcript truncated to a max char budget.
- Web fetches capped per job by depth (above). `Quick` does zero web calls.
- Worker uses the Max-sub OAuth only (never the metered key).
- Claude has **read-only** web tools, no Bash/Edit/Write — minimal blast radius.

## 9. Testing — Chrome GUI automation as the data-quality gate
Reuse harvester's **Playwright** toolchain (Chromium). Tests drive the real
`/analyze` UI and assert **content shape**, not just HTTP 200:
- single known-good video → summary renders, non-empty, expected sections present;
- tutorial → guide has **steps + a verified-commands/references** section, each
  command tagged with an evidence status;
- multiple videos → a **ranked table** with columns + ≥1 row + per-row "why" notes;
- custom-bypass text visibly drives the output;
- no empty/error state for known-good inputs; table well-formed; structure stable
  across repeat runs (consistency check).
- **Determinism:** a fixture set of video IDs + a **worker stub** (canned results)
  for fast CI, plus a `@live` smoke test hitting real yt-dlp + Claude for true
  compatibility. Tests gate merges.

## 10. Risks
- **Datacenter-IP blocking is the dominant risk.** No tool fully solves it; yt-dlp
  + PoToken may still hit `RequestBlocked` from Hetzner. Mitigation ladder:
  Firefox cookies (burner Google account, refresh ~biweekly) → residential proxy.
  The transcript layer is built pluggable so we can escalate without code changes.
- **Context7 on the box:** may not be configured for the box `claude`; the worker
  falls back to WebFetch/WebSearch. (Optional follow-up: configure Context7 there.)
- **Max-quota:** Comprehensive + web verification is token-heavy; depth caps + the
  one-at-a-time worker bound it.

## 11. Phasing (each phase ships working software)
1. **Transcript foundation:** POT provider + yt-dlp fast-path/fallback; verify the
   three previously-broken videos now yield transcripts.
2. **Queue + worker + UI (single video):** `analysis_jobs`, `/analyze` page,
   worker doing Summary + Tutorial(with doc-verification) at all depths.
3. **Ranking:** multi-video rubric + ranked table.
4. **Playwright Chrome GUI tests** as the quality gate (fixtures + stub + `@live`).

## 12. Out of scope
- Replacing/removing the Rust app's legacy `/api/summarize` (can retire later).
- Non-YouTube sources. Audio-only Whisper transcription (yt-dlp subs only for now).
- Multi-user / auth changes (Olimpus already gates everything).
