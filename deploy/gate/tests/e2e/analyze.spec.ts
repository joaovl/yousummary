import { test, expect } from "@playwright/test";
import { unlock, latestAgentJob, resolveLatestJob } from "./helpers";

// Poll interval in the page is 3s; allow several cycles for the round-trip.
const POLL_TIMEOUT = 20_000;

test.describe("yousummary /analyze data-quality gate", () => {
  test("1. unauthenticated GET /analyze redirects to /unlock", async ({ page }) => {
    // No unlock(): the context has no m_session cookie.
    await page.goto("/analyze");
    // Server replies 302 -> /unlock?next=...; the browser follows it.
    await expect(page).toHaveURL(/\/unlock(\?|$)/);
    await expect(page.locator("h1")).toContainText(/unlock/i);
  });

  test("2. unlock + summary round-trip renders structured HTML", async ({ page }) => {
    await unlock(page);
    await page.goto("/analyze");
    await expect(page.locator("h1")).toContainText(/analyze/i);

    await page.fill("#urls", "https://www.youtube.com/watch?v=dQw4w9WgXcQ");
    await page.fill("#transcript", "This is a canned transcript for the summary test.");
    await page.selectOption("#mode", "summary");
    await page.click("#go");

    // The submit creates the job; act as the worker and resolve it.
    await expect
      .poll(async () => (await latestAgentJob(page.request)).id, { timeout: 10_000 })
      .toBeGreaterThan(0);
    const job = await latestAgentJob(page.request);
    expect(job.mode).toBe("summary");

    await resolveLatestJob(
      page.request,
      "<h2>Summary</h2><ul><li>point one</li><li>point two</li></ul>",
    );

    // The page polls /api/analyze/:id and renders result_html into #out.
    const out = page.locator("#out");
    await expect(out.locator("h2")).toContainText("Summary", { timeout: POLL_TIMEOUT });
    await expect(out.locator("li")).toHaveCount(2);
    await expect(out.locator("li").first()).toContainText("point one");

    // No error/empty state.
    await expect(page.locator("#status")).toContainText(/done/i);
    await expect(page.locator("#out .err")).toHaveCount(0);
  });

  test("3. multiple URLs auto-switch to rank mode and render a table", async ({ page }) => {
    await unlock(page);
    await page.goto("/analyze");

    // Two URLs, one per line, Mode left on Auto -> server forces rank.
    await page.fill(
      "#urls",
      "https://www.youtube.com/watch?v=aaaaaaaaaaa\nhttps://www.youtube.com/watch?v=bbbbbbbbbbb",
    );
    await page.click("#go");

    await expect
      .poll(async () => (await latestAgentJob(page.request)).id, { timeout: 10_000 })
      .toBeGreaterThan(0);
    const job = await latestAgentJob(page.request);
    expect(job.mode).toBe("rank");
    expect(job.urls.length).toBe(2);

    const tableHtml =
      "<table><thead><tr><th>Video</th><th>Score</th><th>Why</th></tr></thead>" +
      "<tbody>" +
      "<tr><td>Video A</td><td>9</td><td>clear, doc-verified</td></tr>" +
      "<tr><td>Video B</td><td>4</td><td>outdated commands</td></tr>" +
      "</tbody></table>";
    await resolveLatestJob(page.request, tableHtml);

    const out = page.locator("#out");
    await expect(out.locator("table")).toHaveCount(1, { timeout: POLL_TIMEOUT });
    // Header row + >=1 data row.
    await expect(out.locator("table tr")).toHaveCount(3);
    await expect(out.locator("table thead th")).toHaveCount(3);
    await expect(out.locator("table tbody tr")).toHaveCount(2);
  });

  test("4. custom-bypass instructions are sent to the worker", async ({ page }) => {
    await unlock(page);
    await page.goto("/analyze");

    const customText = "Ignore the mode; just list every CLI flag mentioned.";
    await page.fill("#urls", "https://www.youtube.com/watch?v=ccccccccccc");
    await page.fill("#custom", customText);
    await page.click("#go");

    await expect
      .poll(async () => (await latestAgentJob(page.request)).id, { timeout: 10_000 })
      .toBeGreaterThan(0);
    const job = await latestAgentJob(page.request);
    expect(job.custom).toBeTruthy();
    expect(job.custom).toBe(customText);

    // Clean up the queue so it doesn't dangle.
    await resolveLatestJob(page.request, "<p>done</p>");
  });

  test("5. data-quality: done result is non-empty and has no error text", async ({ page }) => {
    await unlock(page);
    await page.goto("/analyze");

    await page.fill("#transcript", "Another canned transcript for the quality check.");
    await page.selectOption("#mode", "summary");
    await page.click("#go");

    await expect
      .poll(async () => (await latestAgentJob(page.request)).id, { timeout: 10_000 })
      .toBeGreaterThan(0);
    await resolveLatestJob(
      page.request,
      "<h2>Summary</h2><p>A complete, useful summary with substance.</p>",
    );

    const out = page.locator("#out");
    await expect(out.locator("h2")).toContainText("Summary", { timeout: POLL_TIMEOUT });

    const text = (await out.innerText()).trim();
    expect(text.length).toBeGreaterThan(0);
    expect(text.toLowerCase()).not.toContain("error");
    expect(text.toLowerCase()).not.toContain("failed");
    await expect(page.locator("#status")).not.toContainText(/error|failed/i);
  });
});

test.describe("@live", () => {
  test.skip(!process.env.LIVE, "live smoke test — run with LIVE=1 (hits real worker)");

  test("real end-to-end: status eventually leaves pending", async ({ page }) => {
    await unlock(page);
    await page.goto("/analyze");
    await page.fill("#urls", "https://www.youtube.com/watch?v=dQw4w9WgXcQ");
    await page.selectOption("#mode", "summary");
    await page.click("#go");

    // With a real worker running, the job should leave "pending" within a few minutes.
    const job = await latestAgentJob(page.request);
    await expect
      .poll(
        async () => {
          const r = await page.request.get(`/api/analyze/${job.id}`);
          const j = await r.json();
          return j.status as string;
        },
        { timeout: 240_000, intervals: [5_000] },
      )
      .not.toBe("pending");
  });
});
