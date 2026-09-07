# Hetzner → MacBook Migration Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Move every service behind `index.joaovpl.uk` off the Hetzner box `personal-projectsjl` onto the MacBook `my-linux-mint`, one service at a time, with each service's tests passing before its DNS is cut over.

**Architecture:** Both hosts are amd64, so images rebuild unchanged. Ingress is a Cloudflare Tunnel: each hostname maps to `http://localhost:<port>` in `/etc/cloudflared/config.yml`. A second tunnel (`home-mac`) runs on the MacBook; cutover per hostname is a DNS route change plus an ingress entry, and rollback is the same change reversed. The Hetzner box keeps running until every service is moved, so any failed cutover falls back within a minute.

**Tech Stack:** Docker + docker compose, Cloudflare Tunnel (`cloudflared`), Tailscale (private access during verification), Postgres 16 (`pg_dump`/`pg_restore` for state), systemd for host-level workers.

**Spec:** No separate spec document — scope was agreed in conversation on 2026-09-07: all 14 portal services plus the portal itself, the `olimpus` router, and four host-level systemd workers.

## Global Constraints

- Source host: `personal-projectsjl` (Hetzner, 4 vCPU, 7.6 GB RAM, x86_64). Target host: `my-linux-mint` (MacBookPro12,1, i5-5257U 2c/4t, 7.7 GB RAM, 75 GB free, Docker 29.1.3, Linux Mint 22.2). SSH as `root` on both.
- Target RAM is the binding constraint: the source runs ~4.4 GB of containers in 7.7 GB. Do not run both `outline` and `firmgraph` alongside everything else without checking `free -h` first.
- Never delete anything on Hetzner during migration. Stop containers (`docker compose stop`), never `down -v`. The box is decommissioned only after every service is verified on the MacBook.
- Every service keeps its current localhost port number on the new host, so `cloudflared` ingress entries port-match the old config.
- Postgres state moves as `pg_dump`, never as a copy of `/var/lib/postgresql`. Volume data for non-DB services moves with `tar` over ssh.
- Secrets live in `.env` files next to each compose file and in `/root/*.env`. They are not in git — copy them explicitly, and never print their contents to a log.
- A service is "moved" only when: its tests pass on the MacBook, its hostname serves 200 through the new tunnel, and its Hetzner containers are stopped.

---

### Task 0: Prepare the MacBook as a 24/7 host

**Files:**
- Modify: `/etc/systemd/logind.conf` (on `my-linux-mint`)
- Create: `/etc/cloudflared/config.yml` (on `my-linux-mint`)
- Create: `/root/migration/` (working directory for transferred sources)

**Interfaces:**
- Produces: a running `cloudflared` tunnel named `home-mac` with a tunnel UUID used by every later task's DNS cutover; `/root/migration/` as the standard location for copied service sources.

- [ ] **Step 1: Stop the machine suspending when the lid closes**

```bash
ssh root@my-linux-mint 'sed -i "s/^#\?HandleLidSwitch=.*/HandleLidSwitch=ignore/; s/^#\?HandleLidSwitchExternalPower=.*/HandleLidSwitchExternalPower=ignore/" /etc/systemd/logind.conf && systemctl restart systemd-logind && grep -E "^HandleLidSwitch" /etc/systemd/logind.conf'
```

Expected: two lines, both `ignore`.

- [ ] **Step 2: Confirm the machine survives a reboot unattended**

```bash
ssh root@my-linux-mint 'systemctl set-default multi-user.target; systemctl is-enabled docker; systemctl is-enabled tailscaled'
```

Expected: `enabled` for both. (Keeping the graphical target is fine too if you use the laptop directly — then skip `set-default`.)

- [ ] **Step 3: Install cloudflared**

```bash
ssh root@my-linux-mint 'curl -fsSL https://pkg.cloudflare.com/cloudflare-main.gpg -o /usr/share/keyrings/cloudflare-main.gpg && echo "deb [signed-by=/usr/share/keyrings/cloudflare-main.gpg] https://pkg.cloudflare.com/cloudflared any main" > /etc/apt/sources.list.d/cloudflared.list && apt-get update -qq && apt-get install -y cloudflared && cloudflared --version'
```

Expected: a version string.

- [ ] **Step 4: Authenticate and create the tunnel**

This step needs a browser — it prints a URL to authorize against the `joaovpl.uk` zone.

```bash
ssh root@my-linux-mint 'cloudflared tunnel login'
ssh root@my-linux-mint 'cloudflared tunnel create home-mac && cloudflared tunnel list'
```

Expected: `home-mac` listed with a UUID. Record the UUID — later tasks call it `<TUNNEL_UUID>`.

- [ ] **Step 5: Write the initial ingress config with no hostnames**

```bash
ssh root@my-linux-mint 'mkdir -p /etc/cloudflared && cat > /etc/cloudflared/config.yml <<EOF
tunnel: <TUNNEL_UUID>
credentials-file: /root/.cloudflared/<TUNNEL_UUID>.json
ingress:
  - service: http_status:404
EOF
cloudflared service install && systemctl enable --now cloudflared && systemctl is-active cloudflared'
```

Expected: `active`.

- [ ] **Step 6: Create the working directory and verify free space**

```bash
ssh root@my-linux-mint 'mkdir -p /root/migration && df -h / | tail -1 && free -h | head -2'
```

Expected: at least 70 GB free.

- [ ] **Step 7: Commit the plan itself**

```bash
git add docs/superpowers/plans/2026-09-07-migrate-hetzner-to-macbook.md
git commit -m "docs: plan the hetzner to macbook migration"
```

---

### Task 1: Move `flight-simulator` (pipeline shakeout)

Simplest service on the box: one stateless container, 9 MB RSS, one data directory. Its job here is to prove copy → build → verify → cutover → stop works before anything with state moves.

**Files:**
- Copy: `/opt/flight-simulator` (Hetzner) → `/root/migration/flight-simulator` (MacBook)
- Copy: `/root/flight-data` (Hetzner) → `/root/flight-data` (MacBook)
- Copy: `/root/flight-simulator-deploy.env` (Hetzner) → same path (MacBook)
- Modify: `/etc/cloudflared/config.yml` (MacBook)

**Interfaces:**
- Consumes: `<TUNNEL_UUID>` from Task 0.
- Produces: the verified copy → build → test → cutover → stop sequence that Tasks 2-9 repeat.

- [ ] **Step 1: Copy source, data and secrets**

```bash
ssh root@personal-projectsjl 'tar czf - -C /opt flight-simulator' | ssh root@my-linux-mint 'mkdir -p /root/migration && tar xzf - -C /root/migration'
ssh root@personal-projectsjl 'tar czf - -C /root flight-data flight-simulator-deploy.env' | ssh root@my-linux-mint 'tar xzf - -C /root'
ssh root@my-linux-mint 'ls /root/migration/flight-simulator && du -sh /root/flight-data'
```

Expected: the source tree and data directory both present.

- [ ] **Step 2: Build the image on the MacBook**

```bash
ssh root@my-linux-mint 'cd /root/migration/flight-simulator && docker compose build 2>&1 | tail -3'
```

Expected: `Built`. If the project has no compose file, use the `docker build -t flight-simulator:latest .` the box used.

- [ ] **Step 3: Run the project's tests**

```bash
ssh root@my-linux-mint 'cd /root/migration/flight-simulator && ls package.json Makefile pytest.ini 2>/dev/null'
```

If a test runner exists, run it (`npm test`, `pytest`, `make test`) and require a pass. If the project genuinely has no tests, the smoke check in Step 5 is the gate — record in the commit message that this service shipped without a suite.

- [ ] **Step 4: Start it on the same port the box uses (8170)**

```bash
ssh root@my-linux-mint 'cd /root/migration/flight-simulator && docker compose up -d && sleep 5 && docker ps --filter name=flight --format "{{.Names}} {{.Status}} {{.Ports}}"'
```

Expected: running, published on `127.0.0.1:8170`.

- [ ] **Step 5: Smoke-test it privately over Tailscale before any DNS change**

```bash
ssh root@my-linux-mint 'curl -s -o /dev/null -w "%{http_code}\n" http://127.0.0.1:8170/'
```

Expected: `200`.

- [ ] **Step 6: Add the hostname to the MacBook tunnel**

```bash
ssh root@my-linux-mint 'python3 - <<EOF
import re
p = "/etc/cloudflared/config.yml"
s = open(p).read()
entry = "  - hostname: flight.joaovpl.uk\n    service: http://localhost:8170\n"
s = s.replace("  - service: http_status:404", entry + "  - service: http_status:404")
open(p, "w").write(s)
EOF
systemctl restart cloudflared && systemctl is-active cloudflared'
```

Expected: `active`.

- [ ] **Step 7: Point DNS at the new tunnel**

```bash
ssh root@my-linux-mint 'cloudflared tunnel route dns home-mac flight.joaovpl.uk'
```

Expected: confirmation the CNAME now targets `<TUNNEL_UUID>.cfargotunnel.com`. This overwrites the old record — that is the cutover.

- [ ] **Step 8: Verify publicly**

```bash
curl -s -o /dev/null -w "%{http_code}\n" https://flight.joaovpl.uk/
```

Expected: `200`. If it fails, roll back with `cloudflared tunnel route dns <old-tunnel-name> flight.joaovpl.uk` run on the Hetzner box, then debug.

- [ ] **Step 9: Stop the Hetzner copy (do not remove it)**

```bash
ssh root@personal-projectsjl 'docker stop flight-simulator && docker ps -a --filter name=flight --format "{{.Names}} {{.Status}}"'
```

Expected: `Exited`. Re-verify `https://flight.joaovpl.uk/` still returns 200 — this proves traffic is genuinely served by the MacBook.

- [ ] **Step 10: Commit the migration log**

```bash
git add docs/superpowers/plans/2026-09-07-migrate-hetzner-to-macbook.md
git commit -m "docs: mark flight-simulator migrated"
```

---

### Task 2: Move the standalone singles — `lyrics`, `weather`, `riteschool`, `portal`

Four independent containers, each with one data directory and no database. Repeat Task 1's ten steps once per service with these substitutions:

| Service | Source dir | Data dir | Port | Hostname |
|---|---|---|---|---|
| `lyrics` | `/root/lyrics` | `/root/lyrics-data` | 8190 | `lyrics.joaovpl.uk` |
| `weather` | `/root/weather` | `/root/weather-data` | 8140 | `weather.joaovpl.uk` |
| `riteschool` | `/opt/riteschool` | `/root/riteschool-data` | 8000 | `riteschool.joaovpl.uk` |
| `portal` | `/root/portal` | (none — nginx serving static files) | 8090 | `index.joaovpl.uk` |

**Interfaces:**
- Consumes: the sequence from Task 1.
- Produces: `index.joaovpl.uk` served from the MacBook, so the portal links can be used to eyeball every remaining service during later cutovers.

- [ ] **Step 1: Move `lyrics` through all ten steps of Task 1**

Note its image is 1.61 GB — build time on the MacBook's dual-core CPU will be several minutes.

- [ ] **Step 2: Move `weather` through all ten steps of Task 1**

`weather` holds 961 MB RSS, the largest of the singles. After starting it, check `free -h` on the MacBook and confirm at least 1 GB stays available.

- [ ] **Step 3: Move `riteschool` through all ten steps of Task 1**

Its data dir is `/root/riteschool-data/feedback` — copy the whole `/root/riteschool-data` tree so the mount path matches.

- [ ] **Step 4: Move `portal` through all ten steps of Task 1**

`portal` is `nginx:alpine` serving `/root/portal`; no build step, so skip Step 2 and pull the image instead: `docker pull nginx:alpine`.

- [ ] **Step 5: Commit**

```bash
git commit -am "docs: mark lyrics, weather, riteschool and portal migrated"
```

---

### Task 3: Move `jobs` (first app + database pair)

Two containers: `jobs-web` and `postgres:16` on the named volume `jobs_jobs_pg`. This is the first task that moves database state, and it establishes the dump/restore sequence Tasks 4-8 reuse.

**Files:**
- Copy: `/opt/jobs` (Hetzner) → `/root/migration/jobs` (MacBook)
- Create: `/root/migration/jobs.sql` (temporary dump, deleted after restore)

**Interfaces:**
- Consumes: the sequence from Task 1.
- Produces: the `pg_dump` → `pg_restore` sequence reused by `bidpulse`, `surrey-tracker`, `firmgraph`, `proppulse` and `outline`.

- [ ] **Step 1: Copy the source tree and its env file**

```bash
ssh root@personal-projectsjl 'tar czf - -C /opt jobs' | ssh root@my-linux-mint 'tar xzf - -C /root/migration'
ssh root@my-linux-mint 'ls -a /root/migration/jobs | head'
```

Expected: compose file plus any `.env` present.

- [ ] **Step 2: Dump the database from the running Hetzner container**

```bash
ssh root@personal-projectsjl 'docker exec jobs-db-1 pg_dumpall -U postgres --clean --if-exists' > /tmp/jobs.sql
wc -c /tmp/jobs.sql
```

Expected: a non-trivial byte count. `pg_dumpall` captures roles as well as data, which matters because the app's role must exist before restore.

- [ ] **Step 3: Start only the database on the MacBook**

```bash
ssh root@my-linux-mint 'cd /root/migration/jobs && docker compose up -d db && sleep 10 && docker ps --filter name=jobs-db --format "{{.Names}} {{.Status}}"'
```

Expected: healthy/running. The service key may be `db` or `jobs-db` — read the compose file.

- [ ] **Step 4: Restore into it**

```bash
cat /tmp/jobs.sql | ssh root@my-linux-mint 'docker exec -i jobs-db-1 psql -U postgres -v ON_ERROR_STOP=0 -q' 2>&1 | tail -5
ssh root@my-linux-mint 'docker exec jobs-db-1 psql -U postgres -tAc "select datname, pg_size_pretty(pg_database_size(datname)) from pg_database order by pg_database_size(datname) desc" | head -5'
```

Expected: the application database present with a size close to the Hetzner original. Compare against `ssh root@personal-projectsjl 'docker exec jobs-db-1 psql -U postgres -tAc "select datname, pg_size_pretty(pg_database_size(datname)) from pg_database order by pg_database_size(datname) desc" | head -5'`.

- [ ] **Step 5: Build and start the web container**

```bash
ssh root@my-linux-mint 'cd /root/migration/jobs && docker compose build 2>&1 | tail -2 && docker compose up -d && sleep 8 && docker ps --filter name=jobs --format "{{.Names}} {{.Status}} {{.Ports}}"'
```

Expected: both containers up, web published on `127.0.0.1:8210`.

- [ ] **Step 6: Run the project's tests**

```bash
ssh root@my-linux-mint 'ls /root/migration/jobs/*/tests /root/migration/jobs/tests 2>/dev/null | head'
```

Run whatever suite exists against the MacBook instance and require a pass before cutover. If there is none, treat Step 7 as the gate and say so in the commit.

- [ ] **Step 7: Smoke-test, then cut over and stop the old stack**

```bash
ssh root@my-linux-mint 'curl -s -o /dev/null -w "%{http_code}\n" http://127.0.0.1:8210/'
```

Then Task 1 Steps 6-9 with `hostname: jobs.joaovpl.uk`, `service: http://localhost:8210`, stopping `jobs-web-1` and `jobs-db-1` on Hetzner.

- [ ] **Step 8: Delete the dump and commit**

```bash
rm -f /tmp/jobs.sql
git commit -am "docs: mark jobs migrated"
```

The dump contains application data — do not leave it lying in `/tmp`.

---

### Task 4: Move `bidpulse` (4 containers + host watcher)

`bidpulse-api`, `bidpulse-web`, `bidpulse-db` (postgres:16), `bidpulse-edge` (caddy:2), plus the `bidpulse-watch.service` systemd unit on the host. Note this stack sets container memory limits (api 320 MB, db 256 MB, web 256 MB, edge 64 MB) — keep them, they matter more on the smaller host.

**Files:**
- Copy: `/root/bidpulse` → `/root/migration/bidpulse`, `/root/bidpulse-data` → `/root/bidpulse-data`
- Copy: `/etc/systemd/system/bidpulse-watch.service` → same path on MacBook

- [ ] **Step 1: Copy source, data and the systemd unit**

```bash
ssh root@personal-projectsjl 'tar czf - -C /root bidpulse bidpulse-data' | ssh root@my-linux-mint 'tar xzf - -C /root && mv /root/bidpulse /root/migration/bidpulse'
ssh root@personal-projectsjl 'cat /etc/systemd/system/bidpulse-watch.service' | ssh root@my-linux-mint 'cat > /etc/systemd/system/bidpulse-watch.service'
```

- [ ] **Step 2: Dump and restore the database**

Task 3 Steps 2-4, with container `bidpulse-db-1`.

- [ ] **Step 3: Build, start, and check memory headroom**

```bash
ssh root@my-linux-mint 'cd /root/migration/bidpulse && docker compose build 2>&1 | tail -2 && docker compose up -d && sleep 10 && docker stats --no-stream --format "{{.Name}} {{.MemUsage}}" | grep bidpulse && free -h | head -2'
```

Expected: four containers up, MacBook still showing available memory.

- [ ] **Step 4: Run the project's tests, then cut over**

Task 1 Steps 6-9 with `hostname: bidpulse.joaovpl.uk`, `service: http://localhost:8130`.

- [ ] **Step 5: Enable the watcher on the MacBook, disable it on Hetzner**

```bash
ssh root@my-linux-mint 'systemctl daemon-reload && systemctl enable --now bidpulse-watch.service && systemctl is-active bidpulse-watch.service'
ssh root@personal-projectsjl 'systemctl disable --now bidpulse-watch.service'
```

Expected: `active` on the MacBook, stopped on Hetzner. Two copies watching the same queue would double-notify, so do these in one sitting.

- [ ] **Step 6: Commit**

```bash
git commit -am "docs: mark bidpulse migrated"
```

---

### Task 5: Move `surrey-tracker` (`trackmytax.joaovpl.uk`)

Three containers: `surrey_web` (published on 3000), `surrey_api`, `surrey_db` (postgres:16 on volume `surrey-tracker_db_data`).

**Files:**
- Copy: `/opt/surrey-tracker` → `/root/migration/surrey-tracker`

- [ ] **Step 1: Copy source and env files**

```bash
ssh root@personal-projectsjl 'tar czf - -C /opt surrey-tracker' | ssh root@my-linux-mint 'tar xzf - -C /root/migration'
```

- [ ] **Step 2: Dump and restore the database**

Task 3 Steps 2-4, with container `surrey_db`.

- [ ] **Step 3: Build and start**

```bash
ssh root@my-linux-mint 'cd /root/migration/surrey-tracker && docker compose build 2>&1 | tail -2 && docker compose up -d && sleep 10 && docker ps --filter name=surrey --format "{{.Names}} {{.Status}} {{.Ports}}"'
```

Note the web container publishes on `127.0.0.1:3000` — check nothing else on the MacBook already holds port 3000 with `ss -lnt | grep :3000` before starting.

- [ ] **Step 4: Run the project's tests, then cut over**

Task 1 Steps 6-9 with `hostname: trackmytax.joaovpl.uk`, `service: http://localhost:3000`.

- [ ] **Step 5: Commit**

```bash
git commit -am "docs: mark surrey-tracker migrated"
```

---

### Task 6: Move `firmgraph` (`company-check.joaovpl.uk`, 5 containers + watcher)

`firmgraph-api`, `firmgraph-web`, `firmgraph-scheduler`, `firmgraph-db` (postgres:16-alpine on `firmgraph_pg_data`), `firmgraph-caddy` (on `firmgraph_caddy_data`/`firmgraph_caddy_config`), plus `firmgraph-watcher.service` with `WorkingDirectory=/opt/firmgraph`.

**Files:**
- Copy: `/opt/firmgraph` → `/root/migration/firmgraph` (the compose file lives in `/opt/firmgraph/infra`)
- Copy: `/etc/systemd/system/firmgraph-watcher.service` → same path on MacBook

- [ ] **Step 1: Copy the whole tree, including `infra/`**

```bash
ssh root@personal-projectsjl 'tar czf - -C /opt firmgraph' | ssh root@my-linux-mint 'tar xzf - -C /root/migration'
ssh root@personal-projectsjl 'cat /etc/systemd/system/firmgraph-watcher.service' | ssh root@my-linux-mint 'cat > /etc/systemd/system/firmgraph-watcher.service'
```

The watcher's `WorkingDirectory` is `/opt/firmgraph`, so either symlink `/opt/firmgraph -> /root/migration/firmgraph` on the MacBook or edit the unit. Prefer the symlink — it keeps the unit file identical.

- [ ] **Step 2: Dump and restore the database**

Task 3 Steps 2-4, with container `firmgraph-db-1`.

- [ ] **Step 3: Copy the Caddy volumes**

```bash
ssh root@personal-projectsjl 'docker run --rm -v firmgraph_caddy_data:/d -v firmgraph_caddy_config:/c alpine tar czf - -C / d c' | ssh root@my-linux-mint 'docker volume create firmgraph_caddy_data >/dev/null; docker volume create firmgraph_caddy_config >/dev/null; docker run --rm -i -v firmgraph_caddy_data:/d -v firmgraph_caddy_config:/c alpine tar xzf - -C /'
```

Caddy's data volume holds issued certificates; copying it avoids a fresh ACME run on first boot.

- [ ] **Step 4: Build, start, run tests, cut over**

Compose lives in `infra/`, so build with `cd /root/migration/firmgraph/infra && docker compose build`. Then Task 1 Steps 6-9 with `hostname: company-check.joaovpl.uk`, `service: http://localhost:8110`.

- [ ] **Step 5: Move the watcher**

Task 4 Step 5, substituting `firmgraph-watcher.service`.

- [ ] **Step 6: Commit**

```bash
git commit -am "docs: mark firmgraph migrated"
```

---

### Task 7: Move `proppulse` (4 containers, PostGIS)

`proppulse-api`, `proppulse-web`, `proppulse-scheduler`, `proppulse-db` on `postgis/postgis:16-3.4` with a bind mount at `/opt/proppulse-pgdata`.

**Files:**
- Copy: `/opt/proppulse` → `/root/migration/proppulse` (compose in `infra/`)
- Copy: `/opt/proppulse-pgdata` → `/opt/proppulse-pgdata` (MacBook) — but see Step 2

- [ ] **Step 1: Copy the source tree**

```bash
ssh root@personal-projectsjl 'tar czf - -C /opt proppulse' | ssh root@my-linux-mint 'tar xzf - -C /root/migration'
```

- [ ] **Step 2: Move the database by dump, not by copying the bind mount**

```bash
ssh root@personal-projectsjl 'docker exec proppulse-db-1 pg_dumpall -U postgres --clean --if-exists' > /tmp/proppulse.sql
ssh root@my-linux-mint 'mkdir -p /opt/proppulse-pgdata && cd /root/migration/proppulse/infra && docker compose up -d db && sleep 15'
cat /tmp/proppulse.sql | ssh root@my-linux-mint 'docker exec -i proppulse-db-1 psql -U postgres -q' 2>&1 | tail -3
rm -f /tmp/proppulse.sql
```

Copying `/opt/proppulse-pgdata` byte-for-byte works only if both hosts run the identical Postgres build; the dump avoids that trap, and PostGIS extensions are recreated by the dump's own `CREATE EXTENSION` statements.

- [ ] **Step 3: Verify PostGIS came across**

```bash
ssh root@my-linux-mint 'docker exec proppulse-db-1 psql -U postgres -tAc "select extname, extversion from pg_extension where extname like \"%postgis%\""'
```

Expected: a `postgis` row. If it is missing, the app will fail on the first spatial query — fix before cutover.

- [ ] **Step 4: Build, start, run tests, cut over**

Task 1 Steps 6-9 with `hostname: proppulse.joaovpl.uk`, `service: http://localhost:8120`.

- [ ] **Step 5: Commit**

```bash
git commit -am "docs: mark proppulse migrated"
```

---

### Task 8: Move the `olimpus` group — `avionics`, `harvester`, `yousummary`

`olimpus` is the node router on `127.0.0.1:8180` that fronts three hostnames, so all four move together in one cutover. This group also carries `bgutil-pot`, the `yousummary-gate`, and two systemd workers (`avionics-agent.service`, `yousummary-analyst.service`), plus `claude@avionics.service`.

**Files:**
- Copy: `/opt/olimpus`, `/opt/harvester`, `/root/avionics`, `/opt/yousummary` → `/root/migration/`
- Copy: `/root/avionics-data`, `/root/harvester-data`, `/root/yousummary-data` → same paths on MacBook
- Copy: `/root/yousummary.env` → same path on MacBook
- Copy: `/etc/systemd/system/{avionics-agent,yousummary-analyst}.service` → same paths

- [ ] **Step 1: Copy all four source trees, their data and secrets**

```bash
ssh root@personal-projectsjl 'tar czf - -C /opt olimpus harvester yousummary' | ssh root@my-linux-mint 'tar xzf - -C /root/migration'
ssh root@personal-projectsjl 'tar czf - -C /root avionics avionics-data harvester-data yousummary-data yousummary.env' | ssh root@my-linux-mint 'tar xzf - -C /root && mv /root/avionics /root/migration/avionics'
ssh root@my-linux-mint 'ls /root/migration | head'
```

- [ ] **Step 2: Copy the Claude CLI credentials the analyst and the summarizer both need**

```bash
ssh root@personal-projectsjl 'tar czf - -C /root .claude' | ssh root@my-linux-mint 'tar xzf - -C /root'
ssh root@my-linux-mint 'timeout 90 env -u ANTHROPIC_API_KEY HOME=/root claude -p --model sonnet --output-format text <<< "reply with exactly: ok"'
```

Expected: `ok`. If it reports an expired OAuth session, re-run the headless login described in `docs/video-analysis.md` against the MacBook instead.

- [ ] **Step 3: Drop the SOCKS proxy configuration — it is not needed at home**

```bash
ssh root@my-linux-mint 'sed -i "/^YT_PROXY=/d" /root/yousummary.env && grep -c "^YT_PROXY=" /root/yousummary.env'
```

Expected: `0`. The MacBook is already on the home IP that YouTube accepts, so `YT_PROXY`, the socat bridge and the `ssh -R` tunnel all become unnecessary.

- [ ] **Step 4: Start the PO-token provider**

```bash
ssh root@my-linux-mint 'docker run -d --name bgutil-pot --restart unless-stopped -p 127.0.0.1:4416:4416 brainicism/bgutil-ytdlp-pot-provider:latest && sleep 5 && docker ps --filter name=bgutil --format "{{.Names}} {{.Status}}"'
```

Expected: running. Then connect it to the yousummary compose network once that stack is up: `docker network connect deploy_default bgutil-pot`.

- [ ] **Step 5: Build and test yousummary — it has a real suite, so run it**

```bash
ssh root@my-linux-mint 'cd /root/migration/yousummary && cargo test 2>&1 | grep "test result"'
ssh root@my-linux-mint 'cd /root/migration/yousummary/deploy/gate && npm ci && npx playwright install chromium && npx playwright test 2>&1 | tail -3'
```

Expected: `16 passed` from cargo, `6 passed` from Playwright. Both must pass before cutover.

- [ ] **Step 6: Start the yousummary stack and confirm a real summary end to end**

```bash
ssh root@my-linux-mint 'cd /root/migration/yousummary/deploy && docker compose build 2>&1 | tail -2 && docker compose up -d && sleep 10'
ssh root@my-linux-mint 'IP=$(docker inspect yousummary -f "{{range .NetworkSettings.Networks}}{{.IPAddress}} {{end}}" | awk "{print \$1}"); curl -s -m 400 -X POST http://$IP:8000/api/summarize -H "content-type: application/json" -d "{\"url\":\"https://youtu.be/OYhGxfP37us\",\"length\":\"short\"}" | head -c 200'
```

Expected: `{"success":true,"title":"Joe Rogan SHOCKED By Hitler Conspiracy Theory"...`. This single check proves transcripts (yt-dlp without a proxy), metadata, and the Claude CLI all work on the new host.

- [ ] **Step 7: Start `avionics`, `harvester` and `olimpus`**

```bash
ssh root@my-linux-mint 'cd /root/migration/harvester && docker compose up -d 2>/dev/null || docker run -d --name harvester --restart unless-stopped -p 127.0.0.1:8150:3000 -v /root/harvester-data:/data harvester:latest'
ssh root@my-linux-mint 'cd /root/migration/avionics && docker compose up -d 2>/dev/null || docker run -d --name avionics --restart unless-stopped -v /root/avionics-data:/data avionics:latest'
ssh root@my-linux-mint 'cd /root/migration/olimpus && docker compose up -d && sleep 5 && curl -s -o /dev/null -w "%{http_code}\n" http://127.0.0.1:8180/'
```

Read each project's compose file first and prefer it; the `docker run` fallbacks mirror the mounts and ports the Hetzner containers use.

- [ ] **Step 8: Cut over all three hostnames at once**

Task 1 Steps 6-9, adding three ingress entries — `avionics.joaovpl.uk`, `harvester.joaovpl.uk` and `yousummary.joaovpl.uk`, all pointing at `http://localhost:8180` — then routing all three DNS names and stopping `olimpus`, `avionics`, `harvester`, `yousummary`, `yousummary-gate` and `bgutil-pot` on Hetzner.

- [ ] **Step 9: Move the two workers**

```bash
ssh root@personal-projectsjl 'cat /etc/systemd/system/avionics-agent.service' | ssh root@my-linux-mint 'cat > /etc/systemd/system/avionics-agent.service'
ssh root@personal-projectsjl 'cat /etc/systemd/system/yousummary-analyst.service' | ssh root@my-linux-mint 'cat > /etc/systemd/system/yousummary-analyst.service'
ssh root@my-linux-mint 'ln -sfn /root/migration/yousummary /opt/yousummary && systemctl daemon-reload && systemctl enable --now avionics-agent yousummary-analyst && systemctl is-active avionics-agent yousummary-analyst'
ssh root@personal-projectsjl 'systemctl disable --now avionics-agent yousummary-analyst'
```

Expected: both `active` on the MacBook, stopped on Hetzner. The `yousummary-analyst` unit expects `/opt/yousummary/ops`, hence the symlink.

Then move the templated Claude session unit the avionics project uses:

```bash
ssh root@personal-projectsjl 'cat /etc/systemd/system/claude@.service' | ssh root@my-linux-mint 'cat > /etc/systemd/system/claude@.service'
ssh root@my-linux-mint 'systemctl daemon-reload && systemctl enable --now claude@avionics.service && systemctl is-active claude@avionics.service'
ssh root@personal-projectsjl 'systemctl disable --now claude@avionics.service'
```

Expected: `active` on the MacBook. This unit runs Claude Code inside tmux for the avionics project and depends on the `/root/.claude` credentials copied in Step 2.

- [ ] **Step 10: Tear down the SOCKS plumbing on Hetzner**

```bash
ssh root@personal-projectsjl 'systemctl disable --now yousummary-ytproxy-bridge.service && systemctl is-active yousummary-ytproxy-bridge.service'
```

Expected: `inactive`. Also stop the `ssh -R 8899` tunnel running on the Windows machine — nothing needs it now.

- [ ] **Step 11: Commit**

```bash
git commit -am "docs: mark the olimpus group migrated"
```

---

### Task 9: Move `outline` (`notes.joaovpl.uk`, `wiki.joaovpl.uk`)

Heaviest stack and therefore last: `outline` (636 MB RSS), `wikijs` (293 MB), `postgres:16-alpine`, `redis:7-alpine`, `caddy`, across five volumes. Two hostnames share the same Caddy on port 8200.

**Files:**
- Copy: `/opt/outline` → `/root/migration/outline`

- [ ] **Step 1: Check the MacBook can actually hold it**

```bash
ssh root@my-linux-mint 'free -h | head -2 && df -h / | tail -1'
```

Expected: at least 1.5 GB available memory before starting, since this stack adds roughly 1 GB. If it is tight, this is the service to leave on Hetzner or retire.

- [ ] **Step 2: Copy source and env**

```bash
ssh root@personal-projectsjl 'tar czf - -C /opt outline' | ssh root@my-linux-mint 'tar xzf - -C /root/migration'
```

- [ ] **Step 3: Dump and restore Postgres**

Task 3 Steps 2-4, with container `outline-postgres-1`. Outline stores documents in Postgres, so this dump is the important one — verify the restored database size matches before going further.

- [ ] **Step 4: Copy the Outline data and Caddy volumes**

```bash
ssh root@personal-projectsjl 'docker run --rm -v outline_outline_data:/od -v outline_caddy_data:/cd -v outline_caddy_config:/cc alpine tar czf - -C / od cd cc' | ssh root@my-linux-mint 'for v in outline_outline_data outline_caddy_data outline_caddy_config; do docker volume create $v >/dev/null; done; docker run --rm -i -v outline_outline_data:/od -v outline_caddy_data:/cd -v outline_caddy_config:/cc alpine tar xzf - -C /'
```

`outline_outline_data` holds uploaded attachments — losing it means broken images in every document. `outline_redis_data` is a cache and does not need copying.

- [ ] **Step 5: Start the stack and check both apps**

```bash
ssh root@my-linux-mint 'cd /root/migration/outline && docker compose up -d && sleep 30 && docker ps --filter name=outline --format "{{.Names}} {{.Status}}" && curl -s -o /dev/null -w "caddy:%{http_code}\n" http://127.0.0.1:8200/'
```

Expected: five containers up. Outline takes 20-30 seconds to become ready — do not judge it before then.

- [ ] **Step 6: Cut over both hostnames**

Task 1 Steps 6-9, adding `notes.joaovpl.uk` and `wiki.joaovpl.uk`, both to `http://localhost:8200`, then stopping all five Hetzner containers.

- [ ] **Step 7: Log into Outline and open a document with an attachment**

An HTTP 200 from Caddy does not prove the attachments volume survived. Open one real document containing an image before declaring this done.

- [ ] **Step 8: Commit**

```bash
git commit -am "docs: mark outline migrated"
```

---

### Task 10: Decommission

- [ ] **Step 1: Confirm every hostname is served by the MacBook**

```bash
for h in avionics bidpulse company-check flight harvester index jobs lyrics notes proppulse riteschool trackmytax weather wiki yousummary; do printf "%-14s %s\n" "$h" "$(curl -s -o /dev/null -w "%{http_code}" https://$h.joaovpl.uk/)"; done
```

Expected: no 5xx. Run this with the Hetzner containers stopped but the box still alive, so a failure is one `docker start` away from recovery.

- [ ] **Step 2: Let it soak**

Leave the Hetzner box stopped-but-alive for at least a week of normal use. Cron jobs, schedulers and watchers only reveal themselves when they miss a run.

- [ ] **Step 3: Take a final backup before cancelling**

```bash
ssh root@personal-projectsjl 'tar czf - /root /opt --exclude="*/node_modules" --exclude="*/target" 2>/dev/null' > ~/hetzner-final-backup.tar.gz
ls -lh ~/hetzner-final-backup.tar.gz
```

Keep this off the MacBook — an external disk or cloud storage. It is the last copy of anything overlooked.

- [ ] **Step 4: Cancel the server**

Only after Steps 1-3. Cancellation is irreversible and the disk is wiped.
