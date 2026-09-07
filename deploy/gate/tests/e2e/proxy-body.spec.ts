import { test, expect } from "@playwright/test";
import http from "node:http";
import type { Server } from "node:http";
import { unlock } from "./helpers";

// Stand-in for the yousummary Rust service (the gate boots with
// UPSTREAM=http://127.0.0.1:8198). It echoes the request body back so the test
// can prove the passthrough proxy forwards bodies instead of hanging.
let upstream: Server;

test.beforeAll(async () => {
  upstream = http.createServer((req, res) => {
    const chunks: Buffer[] = [];
    req.on("data", (c) => chunks.push(c as Buffer));
    req.on("end", () => {
      res.writeHead(200, { "content-type": "application/json" });
      res.end(
        JSON.stringify({
          path: req.url,
          method: req.method,
          received: Buffer.concat(chunks).toString("utf8"),
        }),
      );
    });
  });
  await new Promise<void>((resolve) => upstream.listen(8198, "127.0.0.1", resolve));
  // Self-check, and it warms the loopback path: without a first connection here
  // the gate's very first proxy attempt can fail to connect on Windows.
  const probe = await fetch("http://127.0.0.1:8198/probe", { method: "POST", body: "x" });
  if (probe.status !== 200) throw new Error(`upstream fixture not ready: ${probe.status}`);
});

test.afterAll(async () => {
  await new Promise<void>((resolve) => upstream.close(() => resolve()));
});

test.describe("gate passthrough proxy", () => {
  test("forwards JSON POST bodies to upstream", async ({ page }) => {
    await unlock(page);

    const res = await page.request.post("/api/summarize", {
      headers: { "content-type": "application/json" },
      data: { url: "https://youtu.be/OYhGxfP37us", length: "medium" },
      timeout: 15_000,
    });

    expect(res.status()).toBe(200);
    const body = await res.json();
    expect(body.path).toBe("/api/summarize");
    expect(JSON.parse(body.received)).toEqual({
      url: "https://youtu.be/OYhGxfP37us",
      length: "medium",
    });
  });
});
