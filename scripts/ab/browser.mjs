// Runs one browser client for scripts/ab_compare.py and prints its metrics
// as JSON. Called with a single JSON argument:
//
//   { url, inject, shell, duration, screenshot, shotAt, width, height,
//     dpr, channel, headless }
//
// `inject` is desktop/krp/inject.js for KRP's client (with `shell` as its
// window.__vertixShell); the Rust client reads its options from `url` and
// reports on its own. Both set window.vertixMetrics when `duration` is up.
//
// Needs Playwright: `npm install --no-save playwright` in this folder, then
// `npx playwright install chromium` (or pass a `channel` such as "chrome"
// to use an installed browser).
import { readFileSync } from "node:fs";
import { chromium } from "playwright";

const o = JSON.parse(process.argv[2]);
const browser = await chromium.launch({
	headless: o.headless ?? false,
	channel: o.channel || undefined,
});
const context = await browser.newContext({
	viewport: { width: o.width ?? 1280, height: o.height ?? 720 },
	deviceScaleFactor: o.dpr ?? undefined,
});
if (o.inject) {
	const shell = JSON.stringify(o.shell ?? {});
	await context.addInitScript({
		content: `window.__vertixShell = ${shell};\n${readFileSync(o.inject, "utf8")}`,
	});
}
const page = await context.newPage();
page.on("pageerror", (e) => console.error(`page error: ${e.message}`));
await page.goto(o.url);
if (o.screenshot) {
	await page.waitForTimeout((o.shotAt ?? 5) * 1000);
	await page.screenshot({ path: o.screenshot });
}
// Polls on a timer: polling on animation frames would add frames of its
// own to the page being measured.
await page.waitForFunction(() => window.vertixMetrics, null, {
	timeout: ((o.duration ?? 30) + 60) * 1000,
	polling: 500,
});
console.log(JSON.stringify(await page.evaluate(() => window.vertixMetrics)));
await browser.close();
