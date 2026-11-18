import AxeBuilder from "@axe-core/playwright";
import { expect, test, type Page } from "@playwright/test";
import { execFileSync } from "node:child_process";
import { cpSync } from "node:fs";
import { resolve } from "node:path";
import { pathToFileURL } from "node:url";
import { binary, OUT } from "./setup";

const VIEWS = ["Overview", "Data", "Explore", "Relationships", "Equations", "Target", "Next experiments", "Reliability"];

function watchErrors(page: Page): string[] {
  const errors: string[] = [];
  page.on("pageerror", (e) => errors.push(String(e)));
  page.on("console", (m) => {
    if (m.type() === "error") errors.push(m.text());
  });
  return errors;
}

test("static report.html opens offline and every view renders", async ({ page, context }) => {
  // No network: every request other than the file itself must fail.
  await context.route(/^https?:/, (r) => r.abort());
  const errors = watchErrors(page);
  await page.goto(pathToFileURL(resolve(OUT, "report", "report.html")).href);
  await expect(page.getByRole("tablist", { name: "Report views" })).toBeVisible();
  for (const v of VIEWS) {
    await page.getByRole("tab", { name: v }).click();
    await expect(page.getByRole("tab", { name: v })).toHaveAttribute("aria-selected", "true");
    await expect(page.locator("#panel")).not.toBeEmpty();
  }
  expect(errors).toEqual([]);
});

test("grade and evidence appear next to the equation and candidates", async ({ page }) => {
  await page.goto(pathToFileURL(resolve(OUT, "report", "report.html")).href);
  await page.getByRole("tab", { name: "Equations" }).click();
  await expect(page.locator(".equation-card .badge").first()).toBeVisible();
  await expect(page.locator(".equation-card")).toContainText("evidence");
  await page.getByRole("tab", { name: "Target" }).click();
  await expect(page.getByRole("heading", { name: /Candidates/ })).toContainText("evidence");
  // Clicking another front equation updates the card.
  await page.getByRole("tab", { name: "Equations" }).click();
  const options = page.getByRole("listbox", { name: "Equations on the front" }).getByRole("option");
  await options.first().click();
  await expect(page.locator(".equation-card")).toContainText("#0");
});

test("keyboard navigation between views", async ({ page }) => {
  await page.goto(pathToFileURL(resolve(OUT, "report", "report.html")).href);
  await page.getByRole("tab", { name: "Overview" }).focus();
  await page.keyboard.press("ArrowRight");
  await expect(page.getByRole("tab", { name: "Explore" })).toHaveAttribute("aria-selected", "true");
  await expect(page.getByRole("tab", { name: "Explore" })).toBeFocused();
});

test("no serious accessibility violations in any view", async ({ page }) => {
  await page.goto(pathToFileURL(resolve(OUT, "report", "report.html")).href);
  for (const v of VIEWS) {
    await page.getByRole("tab", { name: v }).click();
    const r = await new AxeBuilder({ page }).analyze();
    const bad = r.violations.filter((x) => x.impact === "serious" || x.impact === "critical");
    expect(
      bad.map((x) => `${v}: ${x.id} (${x.nodes.length}) ${x.nodes[0]?.target.join(" ")}`),
    ).toEqual([]);
  }
});

test("brushing a 50k-point scatter stays above 30 frames per second", async ({ page }) => {
  const big = resolve(OUT, "big");
  const report = resolve(OUT, "big-report");
  execFileSync(binary(), ["synth", "business-revenue", "--rows", "50000", "--out", big]);
  cpSync(resolve(OUT, "report"), report, { recursive: true });
  execFileSync(binary(), ["report", report, "--data", resolve(big, "data.csv")]);
  await page.goto(pathToFileURL(resolve(report, "report.html")).href);
  await page.getByRole("tab", { name: "Explore" }).click();
  await expect(page.getByText("50,000 sampled rows").or(page.locator(".scatter").first())).toBeVisible();
  const box = (await page.locator(".scatter").first().boundingBox())!;
  const x0 = box.x + 80;
  const y0 = box.y + 30;
  await page.mouse.move(x0, y0);
  await page.mouse.down();
  // Count animation frames the page renders while the brush is dragged continuously.
  await page.evaluate(() => {
    const w = window as unknown as { __frames: number; __stop: boolean };
    w.__frames = 0;
    w.__stop = false;
    const tick = () => {
      w.__frames++;
      if (!w.__stop) requestAnimationFrame(tick);
    };
    requestAnimationFrame(tick);
  });
  const t0 = Date.now();
  for (let i = 1; i <= 40; i++) {
    await page.mouse.move(x0 + i * 9, y0 + i * 6);
  }
  const elapsed = (Date.now() - t0) / 1000;
  const frames = await page.evaluate(() => {
    const w = window as unknown as { __frames: number; __stop: boolean };
    w.__stop = true;
    return w.__frames;
  });
  await page.mouse.up();
  await expect(page.getByText(/rows selected/)).toBeVisible();
  const fps = frames / Math.max(elapsed, 1e-3);
  console.log(`brushing 50k points: ${fps.toFixed(1)} frames per second over ${elapsed.toFixed(2)} s`);
  expect(fps).toBeGreaterThan(30);
});
