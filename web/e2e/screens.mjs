// Dev helper: screenshot every view of a report.html. Usage: node e2e/screens.mjs <report.html> <out-dir>
import { chromium } from "@playwright/test";
import { mkdirSync } from "node:fs";
import { resolve } from "node:path";
import { pathToFileURL } from "node:url";

const [file, out = "test-results/screens"] = process.argv.slice(2);
mkdirSync(out, { recursive: true });
const browser = await chromium.launch({ channel: process.env.PW_CHANNEL || undefined });
const page = await browser.newPage({ viewport: { width: 1400, height: 1000 } });
page.on("pageerror", (e) => console.log("pageerror:", String(e)));
await page.goto(pathToFileURL(resolve(file)).href);
const tabs = ["Overview", "Data", "Explore", "Relationships", "Equations", "Target", "Next experiments", "Reliability"];
for (const [i, v] of tabs.entries()) {
  await page.getByRole("tab", { name: v }).click();
  await page.waitForTimeout(300);
  await page.screenshot({ path: resolve(out, `${i}-${v.replace(/ /g, "-")}.png`), fullPage: true });
}
await browser.close();
console.log("screens in", out);
