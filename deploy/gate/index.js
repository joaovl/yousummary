import express from "express";
import { createProxyMiddleware } from "http-proxy-middleware";
import { authenticator } from "otplib";
import { createHmac, timingSafeEqual } from "node:crypto";
import { mkdirSync } from "node:fs";
import { dirname } from "node:path";
import Database from "better-sqlite3";

const PORT = Number(process.env.PORT ?? 8160);
const UPSTREAM = process.env.UPSTREAM ?? "http://127.0.0.1:8170";
const HARVESTER_BUGS_URL =
  process.env.HARVESTER_BUGS_URL ?? "http://127.0.0.1:8150/api/bugs";
const TOTP_SECRET = process.env.YOUSUMMARY_TOTP_SECRET;
const SESSION_SECRET =
  process.env.YOUSUMMARY_SESSION_SECRET || "y-session:" + (TOTP_SECRET ?? "");
const AGENT_TOKEN = process.env.YOUSUMMARY_AGENT_TOKEN;
const DB_PATH = process.env.YOUSUMMARY_DB_PATH ?? "/data/yousummary.db";
const COOKIE = "y_session";
const TTL = 12 * 3600;

if (!TOTP_SECRET) {
  console.error("[gate] YOUSUMMARY_TOTP_SECRET unset — refusing to start");
  process.exit(1);
}
if (!AGENT_TOKEN) {
  console.error("[gate] YOUSUMMARY_AGENT_TOKEN unset — refusing to start");
  process.exit(1);
}

// ---------- DB ----------
mkdirSync(dirname(DB_PATH), { recursive: true });
const db = new Database(DB_PATH);
db.pragma("journal_mode = WAL");
db.exec(`
  CREATE TABLE IF NOT EXISTS compare_jobs (
    id            INTEGER PRIMARY KEY AUTOINCREMENT,
    urls_json     TEXT NOT NULL,
    reporter      TEXT,
    status        TEXT NOT NULL DEFAULT 'pending',
    created_at    TEXT NOT NULL DEFAULT (datetime('now')),
    response_html TEXT,
    error         TEXT,
    resolved_at   TEXT
  );
  CREATE INDEX IF NOT EXISTS idx_compare_jobs_status ON compare_jobs(status);
`);

// ---------- auth helpers ----------
function sign(exp) {
  const sig = createHmac("sha256", SESSION_SECRET).update(String(exp)).digest("hex");
  return `${exp}.${sig}`;
}
function valid(cookie) {
  if (!cookie || !cookie.includes(".")) return false;
  const [expStr, sig] = cookie.split(".");
  if (!expStr || !sig) return false;
  const expected = createHmac("sha256", SESSION_SECRET).update(expStr).digest("hex");
  try {
    const a = Buffer.from(sig, "hex");
    const b = Buffer.from(expected, "hex");
    if (a.length !== b.length || !timingSafeEqual(a, b)) return false;
  } catch {
    return false;
  }
  const exp = Number(expStr);
  return Number.isFinite(exp) && exp > Math.floor(Date.now() / 1000);
}
const attempts = new Map();
function rateLimited(ip) {
  const now = Date.now();
  const rec = attempts.get(ip);
  if (!rec || now - rec.firstAt > 10 * 60 * 1000) {
    attempts.set(ip, { count: 1, firstAt: now });
    return false;
  }
  rec.count += 1;
  return rec.count > 5;
}
function extractVideoId(url) {
  try {
    const u = new URL(url);
    if (u.hostname.includes("youtu.be")) return u.pathname.replace(/^\//, "");
    return u.searchParams.get("v") || "";
  } catch { return ""; }
}

// ---------- app ----------
const app = express();
app.disable("x-powered-by");
app.use(express.json({ limit: "256kb" }));

function gateMiddleware(req, res, next) {
  const raw = (req.headers.cookie || "")
    .split(";").map(s => s.trim()).find(s => s.startsWith(COOKIE + "="));
  const v = raw ? decodeURIComponent(raw.slice(COOKIE.length + 1)) : undefined;
  if (valid(v)) return next();
  if (req.path.startsWith("/api/")) {
    return res.status(401).json({ detail: "authentication required" });
  }
  const next_ = req.originalUrl;
  return res.redirect(302, `/unlock?next=${encodeURIComponent(next_)}`);
}
function agentAuth(req, res, next) {
  const token = req.query.token || "";
  if (!AGENT_TOKEN || token !== AGENT_TOKEN) {
    return res.status(404).type("text/plain").send("not found");
  }
  next();
}

// ---------- Public: unlock, health ----------
app.get("/unlock", (_req, res) => {
  res.set("content-type", "text/html; charset=utf-8");
  res.send(`<!doctype html><meta charset=utf-8><title>Unlock yousummary</title>
<style>body{font-family:system-ui;max-width:360px;margin:4rem auto;padding:0 1rem;color:#111}
input{font-size:1.5rem;letter-spacing:.2em;text-align:center;width:100%;padding:.5rem;box-sizing:border-box}
button{width:100%;padding:.6rem;font-size:1rem;margin-top:.6rem}
.err{color:tomato}</style>
<h1>Unlock</h1><p>Enter the 6-digit code from your authenticator.</p>
<form id=f><input id=c inputmode=numeric autocomplete=one-time-code placeholder="123 456" autofocus>
<button type=submit>Unlock</button><p id=e class=err></p></form>
<script>
const f=document.getElementById('f'),c=document.getElementById('c'),e=document.getElementById('e');
f.onsubmit=async ev=>{ev.preventDefault();e.textContent='';
const r=await fetch('/api/unlock',{method:'POST',headers:{'content-type':'application/json'},body:JSON.stringify({code:c.value})});
if(r.ok){const u=new URL(location.href);location.href=u.searchParams.get('next')||'/';}
else{const j=await r.json().catch(()=>({detail:'error'}));e.textContent=j.detail||r.status;}};
</script>`);
});

app.post("/api/unlock", (req, res) => {
  const ip = (req.headers["cf-connecting-ip"] || req.headers["x-forwarded-for"] || req.ip || "")
    .toString().split(",")[0].trim();
  if (rateLimited(ip)) return res.status(429).json({ detail: "too many attempts" });
  const code = String(req.body?.code ?? "").trim().replace(/\s+/g, "");
  if (!code) return res.status(400).json({ detail: "code required" });
  authenticator.options = { window: 1 };
  if (!authenticator.check(code, TOTP_SECRET)) {
    return res.status(401).json({ detail: "invalid code" });
  }
  const exp = Math.floor(Date.now() / 1000) + TTL;
  res.cookie(COOKIE, sign(exp), {
    httpOnly: true, sameSite: "strict",
    secure: process.env.NODE_ENV === "production",
    path: "/", maxAge: TTL * 1000,
  });
  res.json({ ok: true });
});

app.get("/api/health", (_req, res) => {
  const open = db.prepare("SELECT COUNT(*) AS c FROM compare_jobs WHERE status='pending'").get();
  res.json({ ok: true, service: "yousummary-gate", pending_jobs: open.c });
});

// ---------- Public: bug intake (forwards to harvester) ----------
const MAX_TITLE = 200;
const MAX_BODY = 8000;
const SEVS = new Set(["low", "medium", "high"]);

app.get("/report", (_req, res) => {
  res.set("content-type", "text/html; charset=utf-8");
  res.send(`<!doctype html><meta charset=utf-8><title>Report a bug · yousummary</title>
<style>body{font-family:system-ui;max-width:560px;margin:2rem auto;padding:0 1rem;color:#111}
label{display:block;margin-top:1rem;font-weight:600}
input,textarea,select{width:100%;padding:.5rem;box-sizing:border-box;font:inherit}
textarea{min-height:160px}
button{margin-top:1rem;padding:.6rem 1.2rem;font-size:1rem}
.ok{color:#0a7}.err{color:tomato}.muted{color:#666;font-size:.9rem}</style>
<h1>Report a bug · yousummary</h1>
<p class=muted>This goes to the shared backlog. Don't include secrets.</p>
<form id=f>
<label>Title (≤200)<input id=title maxlength=200 required></label>
<label>Body — steps, expected vs actual (≤8000)<textarea id=body maxlength=8000 required></textarea></label>
<label>Severity<select id=sev><option value=low>low</option><option value=medium selected>medium</option><option value=high>high</option></select></label>
<label>Reporter (optional)<input id=reporter maxlength=100></label>
<button type=submit>Submit</button>
<p id=msg></p>
</form>
<script>
const $=id=>document.getElementById(id);
$('f').onsubmit=async ev=>{ev.preventDefault();$('msg').textContent='';$('msg').className='';
const url=location.origin;
const payload={title:$('title').value,body:$('body').value,severity:$('sev').value,reporter:$('reporter').value||null,url};
const r=await fetch('/api/bugs',{method:'POST',headers:{'content-type':'application/json'},body:JSON.stringify(payload)});
const j=await r.json().catch(()=>({detail:'error'}));
if(r.ok){$('msg').textContent='Filed as bug #'+j.id+'. Check status at /api/bugs/'+j.id+'.';$('msg').className='ok';}
else{$('msg').textContent='Error: '+(j.detail||r.status);$('msg').className='err';}};
</script>`);
});

app.post("/api/bugs", async (req, res) => {
  try {
    const b = req.body || {};
    const title = String(b.title ?? "").trim();
    const body = String(b.body ?? "").trim();
    if (!title || !body) return res.status(400).json({ detail: "title and body required" });
    if (title.length > MAX_TITLE) return res.status(400).json({ detail: `title > ${MAX_TITLE} chars` });
    if (body.length > MAX_BODY) return res.status(400).json({ detail: `body > ${MAX_BODY} chars` });
    const severity = String(b.severity ?? "medium");
    if (!SEVS.has(severity)) return res.status(400).json({ detail: "severity must be low|medium|high" });
    const payload = {
      title: `[yousummary] ${title}`,
      body,
      severity,
      url: b.url ? String(b.url).slice(0, 500) : null,
      reporter: b.reporter ? String(b.reporter).slice(0, 100) : null,
    };
    const upstream = await fetch(HARVESTER_BUGS_URL, {
      method: "POST",
      headers: { "content-type": "application/json" },
      body: JSON.stringify(payload),
    });
    const text = await upstream.text();
    res.status(upstream.status).type("application/json").send(text);
  } catch (e) {
    res.status(502).json({ detail: "upstream unreachable", error: String(e) });
  }
});

app.get("/api/bugs/:id", async (req, res) => {
  try {
    const id = Number(req.params.id);
    if (!Number.isInteger(id) || id <= 0) return res.status(400).json({ detail: "bad id" });
    const upstream = await fetch(`${HARVESTER_BUGS_URL}/${id}`);
    const text = await upstream.text();
    res.status(upstream.status).type("application/json").send(text);
  } catch (e) {
    res.status(502).json({ detail: "upstream unreachable", error: String(e) });
  }
});

// ---------- Compare: gated UI, gated submit, public status poll, agent endpoints ----------
const MAX_COMPARE_URLS = 5;

app.get("/compare", gateMiddleware, (_req, res) => {
  res.set("content-type", "text/html; charset=utf-8");
  res.send(`<!doctype html><meta charset=utf-8><title>Compare YouTube videos · yousummary</title>
<style>body{font-family:system-ui;max-width:920px;margin:2rem auto;padding:0 1rem;color:#111}
textarea{width:100%;min-height:140px;padding:.5rem;font:14px ui-monospace,Menlo,monospace;box-sizing:border-box}
button{padding:.6rem 1.2rem;font-size:1rem;margin-top:1rem}
table{border-collapse:collapse;width:100%;margin-top:1rem;font-size:.95rem}
th,td{border:1px solid #ddd;padding:.5rem .7rem;vertical-align:top;text-align:left}
th{background:#f5f5f5}
.muted{color:#666;font-size:.9rem}
.err{color:tomato;white-space:pre-wrap}
#out{margin-top:1.5rem}
#status{margin-top:.6rem}
.spinner{display:inline-block;width:1em;height:1em;border:2px solid #ccc;border-top-color:#222;border-radius:50%;animation:spin .8s linear infinite;vertical-align:-.15em}
@keyframes spin{to{transform:rotate(360deg)}}</style>
<h1>Compare YouTube videos</h1>
<p class=muted>Paste up to ${MAX_COMPARE_URLS} YouTube URLs (one per line). The request is queued; a local agent watches the queue and posts a comparison table back. Result appears below once ready.</p>
<form id=f>
<label><strong>URLs</strong></label>
<textarea id=urls placeholder="https://www.youtube.com/watch?v=...&#10;https://www.youtube.com/watch?v=..."></textarea>
<button type=submit id=go>Compare</button>
<span id=status class=muted></span>
</form>
<div id=out></div>
<script>
const $=id=>document.getElementById(id);
let pollTimer=null;
async function poll(id){
  try{
    const r=await fetch('/api/compare/'+id);
    const j=await r.json();
    if(j.status==='done'){clearInterval(pollTimer);$('status').textContent='done (job #'+id+').';$('out').innerHTML=j.response_html||'<p class=muted>empty response</p>';$('go').disabled=false;}
    else if(j.status==='failed'){clearInterval(pollTimer);$('status').innerHTML='<span class=err>failed: '+(j.error||'')+'</span>';$('go').disabled=false;}
    else{$('status').innerHTML='<span class=spinner></span> job #'+id+' '+j.status+'… (agent watches the queue; usually <2 min)';}
  }catch(e){/* keep polling */}
}
$('f').onsubmit=async ev=>{ev.preventDefault();$('out').innerHTML='';
const urls=$('urls').value.split(/\\r?\\n/).map(s=>s.trim()).filter(Boolean);
if(urls.length<2){$('status').innerHTML='<span class=err>need at least 2 URLs</span>';return;}
$('go').disabled=true;$('status').innerHTML='<span class=spinner></span> submitting…';
try{
  const r=await fetch('/api/compare',{method:'POST',headers:{'content-type':'application/json'},body:JSON.stringify({urls})});
  const j=await r.json();
  if(!r.ok){$('status').innerHTML='<span class=err>'+(j.detail||r.status)+'</span>';$('go').disabled=false;return;}
  $('status').innerHTML='<span class=spinner></span> queued as job #'+j.id+'…';
  poll(j.id);
  pollTimer=setInterval(()=>poll(j.id),3000);
}catch(e){$('status').innerHTML='<span class=err>'+e.message+'</span>';$('go').disabled=false;}
};
</script>`);
});

// Submit → enqueue (gated)
app.post("/api/compare", gateMiddleware, (req, res) => {
  const urls = Array.isArray(req.body?.urls) ? req.body.urls.map(String) : [];
  if (urls.length < 2) return res.status(400).json({ detail: "need at least 2 URLs" });
  if (urls.length > MAX_COMPARE_URLS) return res.status(400).json({ detail: `max ${MAX_COMPARE_URLS} URLs` });
  for (const u of urls) {
    if (!extractVideoId(u)) return res.status(400).json({ detail: `not a YouTube URL: ${u}` });
  }
  const reporter = req.body?.reporter ? String(req.body.reporter).slice(0, 100) : null;
  const info = db
    .prepare("INSERT INTO compare_jobs (urls_json, reporter) VALUES (?, ?)")
    .run(JSON.stringify(urls), reporter);
  return res.status(201).json({ id: Number(info.lastInsertRowid), status: "pending" });
});

// Status poll (public — same shape as /api/bugs/<id>, no secrets exposed)
app.get("/api/compare/:id", (req, res) => {
  const id = Number(req.params.id);
  if (!Number.isInteger(id) || id <= 0) return res.status(400).json({ detail: "bad id" });
  const row = db
    .prepare("SELECT id, urls_json, status, created_at, response_html, error, resolved_at FROM compare_jobs WHERE id = ?")
    .get(id);
  if (!row) return res.status(404).json({ detail: "job not found" });
  res.json({
    id: row.id,
    urls: JSON.parse(row.urls_json),
    status: row.status,
    created_at: row.created_at,
    response_html: row.response_html,
    error: row.error,
    resolved_at: row.resolved_at,
  });
});

// Agent: list pending jobs
app.get("/api/agent/jobs", agentAuth, (_req, res) => {
  const rows = db
    .prepare("SELECT id, urls_json, reporter, created_at FROM compare_jobs WHERE status='pending' ORDER BY id")
    .all();
  res.json({
    count: rows.length,
    jobs: rows.map(r => ({
      id: r.id,
      urls: JSON.parse(r.urls_json),
      reporter: r.reporter,
      created_at: r.created_at,
    })),
  });
});

// Agent: resolve a job
app.post("/api/agent/jobs/:id/resolve", agentAuth, (req, res) => {
  const id = Number(req.params.id);
  if (!Number.isInteger(id) || id <= 0) return res.status(400).json({ detail: "bad id" });
  const status = String(req.body?.status ?? "");
  if (!["done", "failed"].includes(status)) {
    return res.status(400).json({ detail: "status must be done|failed" });
  }
  const response_html = req.body?.response_html ? String(req.body.response_html) : null;
  const error = req.body?.error ? String(req.body.error).slice(0, 2000) : null;
  if (status === "done" && !response_html) {
    return res.status(400).json({ detail: "response_html required when status=done" });
  }
  const r = db
    .prepare(
      `UPDATE compare_jobs
         SET status = ?, response_html = ?, error = ?, resolved_at = datetime('now')
       WHERE id = ? AND status = 'pending'`,
    )
    .run(status, response_html, error, id);
  if (r.changes === 0) return res.status(404).json({ detail: "job not pending or not found" });
  res.json({ ok: true });
});

// ---------- TOTP gate for everything else ----------
const PUBLIC_PATHS = new Set([
  "/unlock", "/api/unlock", "/api/health",
  "/report", "/api/bugs",
]);
app.use((req, res, next) => {
  if (PUBLIC_PATHS.has(req.path)) return next();
  if (req.path.startsWith("/api/bugs/")) return next();
  if (req.path.startsWith("/api/compare/")) return next();   // public poll
  if (req.path.startsWith("/api/agent/")) return next();     // token-gated inside handler
  return gateMiddleware(req, res, next);
});

// ---------- Inject buttons into upstream root HTML ----------
const REPORT_WIDGET = `<style>
.yr-fab{position:fixed;right:1rem;z-index:9999;background:#222;color:#fff;text-decoration:none;
  border:0;border-radius:999px;padding:.6rem 1rem;font:600 14px system-ui;cursor:pointer;
  box-shadow:0 4px 12px rgba(0,0,0,.25);display:inline-block}
.yr-fab:hover{background:#000}
#yr-cmp{bottom:4rem;background:#1e6bcc}
#yr-cmp:hover{background:#125299}
#yr-btn{bottom:1rem}
</style>
<a id="yr-cmp" class="yr-fab" href="/compare" title="Compare 2+ videos in a table">📊 Compare videos</a>
<a id="yr-btn" class="yr-fab" href="/report" title="Report a bug against yousummary">🐞 Report a bug</a>`;

app.get("/", async (req, res, next) => {
  try {
    const upstream = await fetch(UPSTREAM + "/", { headers: { accept: "text/html" } });
    const ct = upstream.headers.get("content-type") || "text/html";
    if (!ct.includes("text/html")) {
      const buf = Buffer.from(await upstream.arrayBuffer());
      return res.status(upstream.status).type(ct).send(buf);
    }
    let html = await upstream.text();
    if (html.includes("</body>")) {
      html = html.replace("</body>", REPORT_WIDGET + "</body>");
    } else {
      html += REPORT_WIDGET;
    }
    res.status(upstream.status).set("content-type", "text/html; charset=utf-8").send(html);
  } catch (e) {
    next(e);
  }
});

// Everything else: passthrough proxy.
app.use(
  "/",
  createProxyMiddleware({
    target: UPSTREAM,
    changeOrigin: true,
    ws: false,
    proxyTimeout: 120_000,
    timeout: 120_000,
  }),
);

const BIND = process.env.GATE_BIND || "127.0.0.1";
app.listen(PORT, BIND, () => {
  console.log(`[gate] listening on ${BIND}:${PORT}, upstream=${UPSTREAM}, bugs->${HARVESTER_BUGS_URL}, db=${DB_PATH}`);
});
