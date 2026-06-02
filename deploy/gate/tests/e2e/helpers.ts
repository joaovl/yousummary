import { authenticator } from "otplib";
import type { APIRequestContext, Page } from "@playwright/test";
import { expect } from "@playwright/test";

const TOTP_SECRET = "JBSWY3DPEHPK3PXP";
const AGENT_TOKEN = "testtoken";

/**
 * Unlock the gate so the browser context carries a valid `m_session` cookie.
 *
 * We POST /api/unlock through `page.request`, which shares its cookie jar with
 * the page. The gate sets m_session (httpOnly, sameSite=strict, secure=false
 * because NODE_ENV=test). After this, page.goto('/analyze') and the in-page
 * fetch() polling to /api/analyze/:id are both authenticated.
 */
export async function unlock(page: Page): Promise<void> {
  authenticator.options = { window: 1 };
  const code = authenticator.generate(TOTP_SECRET);
  const res = await page.request.post("/api/unlock", {
    headers: { "content-type": "application/json" },
    data: { code },
  });
  expect(res.ok(), `unlock failed: ${res.status()} ${await res.text()}`).toBeTruthy();

  // Confirm the cookie actually landed in the context's jar.
  const cookies = await page.context().cookies();
  expect(
    cookies.some((c) => c.name === "m_session" && c.value.includes(".")),
    "m_session cookie not set after unlock",
  ).toBeTruthy();
}

type AgentJob = {
  id: number;
  mode: string;
  depth: string;
  custom: string | null;
  intent: string | null;
  transcript: string | null;
  urls: string[];
};

/** Fetch the pending analysis jobs (token-gated agent endpoint), newest last. */
export async function listAgentJobs(request: APIRequestContext): Promise<AgentJob[]> {
  const res = await request.get(`/api/agent/analysis-jobs?token=${AGENT_TOKEN}`);
  expect(res.ok(), `agent list failed: ${res.status()}`).toBeTruthy();
  const body = await res.json();
  return body.jobs as AgentJob[];
}

/** Return the most-recently-created pending job (highest id). */
export async function latestAgentJob(request: APIRequestContext): Promise<AgentJob> {
  const jobs = await listAgentJobs(request);
  expect(jobs.length, "expected at least one pending analysis job").toBeGreaterThan(0);
  return jobs.reduce((a, b) => (b.id > a.id ? b : a));
}

/**
 * Act as the worker: grab the newest pending job and resolve it `done` with the
 * supplied canned HTML. Mirrors what the box worker would POST back.
 */
export async function resolveLatestJob(
  request: APIRequestContext,
  html: string,
): Promise<number> {
  const job = await latestAgentJob(request);
  const res = await request.post(
    `/api/agent/analysis-jobs/${job.id}/resolve?token=${AGENT_TOKEN}`,
    {
      headers: { "content-type": "application/json" },
      data: { status: "done", result_html: html, result_md: html },
    },
  );
  expect(res.ok(), `resolve failed: ${res.status()} ${await res.text()}`).toBeTruthy();
  return job.id;
}
