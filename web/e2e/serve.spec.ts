import { expect, test } from "@playwright/test";
import { spawn, type ChildProcess } from "node:child_process";
import { resolve } from "node:path";
import { binary, OUT } from "./setup";

let proc: ChildProcess | null = null;
let url = "";

test.beforeAll(async () => {
  proc = spawn(binary(), ["serve", resolve(OUT, "report"), "--no-open"], { stdio: ["ignore", "pipe", "pipe"] });
  url = await new Promise<string>((ok, fail) => {
    let buf = "";
    const timer = setTimeout(() => fail(new Error(`no URL printed: ${buf}`)), 30_000);
    proc!.stdout!.on("data", (d: Buffer) => {
      buf += d.toString();
      const m = /(http:\/\/127\.0\.0\.1:\d+\/#token=[0-9a-f]+)/.exec(buf);
      if (m) {
        clearTimeout(timer);
        ok(m[1]!);
      }
    });
    proc!.on("exit", (c) => fail(new Error(`serve exited with ${c}: ${buf}`)));
  });
});

test.afterAll(() => {
  proc?.kill();
});

test("served UI loads with the token and re-runs goals and suggestions", async ({ page, context }) => {
  // Everything except the local server is blocked: the UI must not need the network.
  const origin = new URL(url).origin;
  await context.route(/^https?:/, (r) => (r.request().url().startsWith(origin) ? r.continue() : r.abort()));
  const errors: string[] = [];
  page.on("pageerror", (e) => errors.push(String(e)));
  await page.goto(url);
  await expect(page.getByRole("tablist", { name: "Report views" })).toBeVisible();
  // The token is removed from the address bar after it is read.
  expect(page.url()).not.toContain("token=");

  await page.getByRole("tab", { name: "Target" }).click();
  const form = page.getByRole("form", { name: "Goal builder" });
  await form.locator("textarea").first().fill("revenue>=30000");
  await form.locator("textarea").nth(1).fill("max:orders");
  await form.getByRole("button", { name: "Find inputs" }).click();
  await expect(page.getByRole("columnheader", { name: "revenue>=30000" })).toBeVisible({ timeout: 60_000 });

  await page.getByRole("tab", { name: "Next experiments" }).click();
  await page.getByLabel("Batch size").fill("4");
  await page.getByRole("button", { name: "Suggest (no goals)" }).click();
  await expect(page.getByText("Mode balanced:")).toBeVisible({ timeout: 60_000 });
  await expect(page.locator("table").first().locator("tbody tr")).toHaveCount(4);
  expect(errors).toEqual([]);
});

test("API refuses requests without the token", async ({ request }) => {
  const origin = new URL(url).origin;
  const r = await request.get(`${origin}/api/report`);
  expect(r.status()).toBe(401);
});
