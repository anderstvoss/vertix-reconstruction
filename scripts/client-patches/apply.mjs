// Patches the pinned KrunkerRevival client checkout before it is built.
// Each patch replaces one exact passage and fails loudly if the passage is
// not found, so a change of pin cannot silently drop a patch. The files are
// first restored from the pinned commit, so running this twice is safe.
//
//   node scripts/client-patches/apply.mjs CHECKOUT_DIR
import { execFileSync } from "node:child_process";
import { readFileSync, writeFileSync } from "node:fs";
import { join } from "node:path";

const checkout = process.argv[2];
if (!checkout) {
	console.error("usage: apply.mjs CHECKOUT_DIR");
	process.exit(2);
}

const patches = [
	{
		// Mod packs: KRP sends a Dropbox share key to Dropbox (the links are
		// dead) and turns a path such as /mods/x/vertixmod.zip into
		// http:///mods/..., which no browser fetches. A key now loads the
		// pack this server restored from the archive (/mods/<key>/), and a
		// path loads from this server; full URLs are unchanged.
		file: "core/src/app.tsx",
		find: [
			"\t\t\tif (url.includes(\".\")) {",
			"\t\t\t\tmodPath = url;",
			"\t\t\t\tif (!modPath.match(/^https?:\\/\\//i)) {",
			"\t\t\t\t\tmodPath = `http://${modPath}`;",
			"\t\t\t\t}",
			"\t\t\t} else {",
			"\t\t\t\tmodPath = `https://dl.dropboxusercontent.com/s/${url}/vertixmod.zip`;",
			"\t\t\t}",
		].join("\n"),
		replace: [
			"\t\t\turl = url.trim();",
			"\t\t\tif (/^(\\.{0,2}\\/|mods\\/)/i.test(url)) {",
			"\t\t\t\tmodPath = new URL(url.replace(/^mods\\//i, \"/mods/\"), location.href).href;",
			"\t\t\t} else if (url.includes(\".\")) {",
			"\t\t\t\tmodPath = url;",
			"\t\t\t\tif (!modPath.match(/^https?:\\/\\//i)) {",
			"\t\t\t\t\tmodPath = `http://${modPath}`;",
			"\t\t\t\t}",
			"\t\t\t} else {",
			"\t\t\t\tmodPath = `/mods/${encodeURIComponent(url)}/vertixmod.zip`;",
			"\t\t\t}",
		].join("\n"),
	},
	{
		// "No mods" (see the mod tab): remember the client's own menu title
		// and classes before any pack replaces them.
		file: "core/src/app.tsx",
		find: 'var linkedMod = location.hash.replace("#", "");',
		replace: [
			"const baseTitle = mainTitleText.innerHTML;",
			"const baseCharacterClasses = st.characterClasses;",
			'var linkedMod = location.hash.replace("#", "");',
		].join("\n"),
	},
	{
		// window.unloadModPack: back to the client's own art, title and
		// classes, without pack sounds (the stock game is silent).
		file: "core/src/app.tsx",
		find: [
			"\t\tloadModPack: typeof loadModPack;",
			"\t}",
			"}",
			"window.loadModPack = loadModPack;",
		].join("\n"),
		replace: [
			"\t\tloadModPack: typeof loadModPack;",
			"\t\tunloadModPack: typeof unloadModPack;",
			"\t}",
			"}",
			"window.loadModPack = loadModPack;",
			"window.unloadModPack = unloadModPack;",
			"async function unloadModPack() {",
			"\tif (loadingTexturePack) return;",
			"\tmainTitleText.innerHTML = baseTitle;",
			"\tst.characterClasses = baseCharacterClasses;",
			"\tst.loadout.class = st.characterClasses.find(",
			"\t\t(c) => c.folderName === st.loadout.class.folderName,",
			"\t)!;",
			'\tawait loadModPack("", true);',
			'\tsetModInfoText("No mod pack loaded");',
			"}",
		].join("\n"),
	},
	{
		// The mod tab lists every pack this server restored
		// (/mods/index.json) plus "No mods", instead of KRP's one Sonic
		// button. Without a pack list (mods turned off) the Sonic button
		// stays.
		file: "core/src/components/tabs/ModTab.svelte",
		find: "\tlet textureModInput: HTMLInputElement;",
		replace: [
			"\tlet textureModInput: HTMLInputElement;",
			"\tconst modPacks: Promise<{ key: string; name: string; bytes: number }[]> = fetch(\"/mods/index.json\")",
			"\t\t.then((r) => (r.ok ? r.json() : Promise.reject(r.status)))",
			"\t\t.then((d) => d.packs);",
			"\tconst packSize = (bytes: number) =>",
			"\t\tbytes < 1048576 ? `${Math.ceil(bytes / 1024)} kb` : `${Math.round(bytes / 1048576)} mb`;",
		].join("\n"),
	},
	{
		file: "core/src/components/tabs/ModTab.svelte",
		find: "\t<div class=\"modBtn\" onclick={() => window.loadModPack('13xlc5n3ipudqsn', false)}>Sonic Mod Pack</div>",
		replace: [
			"\t<div class=\"modBtn\" onclick={() => window.unloadModPack()}>No mods (default art)</div>",
			"\t{#await modPacks then packs}",
			"\t\t{#each packs as pack (pack.key)}",
			"\t\t\t<div class=\"modBtn\" onclick={() => window.loadModPack(pack.key, false)}>{pack.name} ({packSize(pack.bytes)})</div>",
			"\t\t{/each}",
			"\t{:catch}",
			"\t\t<div class=\"modBtn\" onclick={() => window.loadModPack('13xlc5n3ipudqsn', false)}>Sonic Mod Pack</div>",
			"\t{/await}",
		].join("\n"),
	},
	{
		// Sprays: sized from the image file alone instead of KRP's
		// per-spray scale and resolution (see cacheSpray below).
		file: "core/src/app.tsx",
		find: "\ttmpSpray.xPos = x - tmpSpray.scale! / 2;\n\ttmpSpray.yPos = y - tmpSpray.scale! / 2;",
		replace: "\t// The spray's centre; its size comes from its image (cacheSpray).\n\ttmpSpray.xPos = x;\n\ttmpSpray.yPos = y;",
	},
	{
		file: "core/src/app.tsx",
		find: "function cacheSpray(img: Sprite) {\n\tconst tmpIndex = `${img.src}`;\n\tlet tmpSpray = cachedSprays[tmpIndex];\n\tif (tmpSpray || img.width === 0) return;\n\n\tlet initialCanvas = document.createElement(\"canvas\");\n\tlet initialCtx = initialCanvas.getContext(\"2d\")!;\n\tinitialCanvas.width = img.resolution!;\n\tinitialCanvas.height = img.resolution!;\n\tinitialCtx.drawImage(img, 0, 0, img.resolution!, img.resolution!);\n\tlet finalCanvas = document.createElement(\"canvas\");\n\tlet finalCtx = finalCanvas.getContext(\"2d\")!;\n\tfinalCanvas.width = img.scale!;\n\tfinalCanvas.height = img.scale!;\n\tfinalCtx.imageSmoothingEnabled = false;\n\tfinalCtx.globalAlpha = img.alpha!;\n\tfinalCtx.drawImage(initialCanvas, 0, 0, img.scale!, img.scale!);\n\ttmpSpray = finalCanvas;\n\tcachedSprays[tmpIndex] = tmpSpray;\n}\nfunction drawSprays() {\n\tif (!st.settings.showSprays) return;\n\tfor (const sp of userSprays) {\n\t\tif (!sp.active) continue;\n\t\tlet tmpSpray = cachedSprays[`${sp.src}`];\n\t\tif (!tmpSpray) continue;\n\t\tgraph.drawImage(tmpSpray, sp.xPos! - st.startX, sp.yPos! - st.startY);\n\t}\n}",
		replace: "// A spray is just its image file: drawn at SPRAY_PX world pixels per\n// image pixel and at most SPRAY_MAX across. Small images scale up with\n// crisp pixels; larger ones are kept at full resolution and scaled down\n// when drawn, so they keep their detail.\nconst SPRAY_PX = 2;\nconst SPRAY_MAX = 64;\nconst sprayDrawSize: Record<string, [number, number]> = {};\nfunction cacheSpray(img: Sprite) {\n\tconst tmpIndex = `${img.src}`;\n\tif (cachedSprays[tmpIndex] || img.naturalWidth === 0) return;\n\tconst w = img.naturalWidth;\n\tconst h = img.naturalHeight;\n\tconst fit = Math.min(SPRAY_PX, SPRAY_MAX / Math.max(w, h));\n\tconst up = Math.max(1, fit);\n\tconst canvas = document.createElement(\"canvas\");\n\tconst ctx = canvas.getContext(\"2d\")!;\n\tcanvas.width = Math.round(w * up);\n\tcanvas.height = Math.round(h * up);\n\tctx.imageSmoothingEnabled = false;\n\tctx.globalAlpha = img.alpha ?? 1;\n\tctx.drawImage(img, 0, 0, canvas.width, canvas.height);\n\tcachedSprays[tmpIndex] = canvas;\n\tsprayDrawSize[tmpIndex] = [w * fit, h * fit];\n}\nfunction drawSprays() {\n\tif (!st.settings.showSprays) return;\n\tfor (const sp of userSprays) {\n\t\tif (!sp.active) continue;\n\t\tconst tmpSpray = cachedSprays[`${sp.src}`];\n\t\tconst size = sprayDrawSize[`${sp.src}`];\n\t\tif (!tmpSpray || !size) continue;\n\t\tconst [dw, dh] = size;\n\t\tconst down = tmpSpray.width > dw;\n\t\tif (down) graph.imageSmoothingEnabled = true;\n\t\tgraph.drawImage(tmpSpray, sp.xPos! - dw / 2 - st.startX, sp.yPos! - dh / 2 - st.startY, dw, dh);\n\t\tif (down) graph.imageSmoothingEnabled = false;\n\t}\n}",
	},
	{
		// Sprays added on the server (`[content] sprays_dir`) join the
		// spray list.
		file: "core/src/state.svelte.ts",
		find: "window.st = st;",
		replace: "window.st = st;\n\n// Sprays the server adds (PNG files in its sprays folder). The saved choice\n// is read now, before the loadout tab clears a spray it does not know yet.\nconst savedSpray = localStorage.getItem(\"prevSpray\");\nfetch(\"/sprays/index.json\")\n\t.then((r) => (r.ok ? r.json() : { sprays: [] }))\n\t.then((d: { sprays: (typeof sprays)[number][] }) => {\n\t\tfor (const spray of d.sprays) {\n\t\t\tif (!st.sprays.some((s) => s.id === spray.id)) st.sprays.push(spray);\n\t\t}\n\t\tif (!st.loadout.spray && savedSpray) {\n\t\t\tst.loadout.spray = st.sprays.find((s) => String(s.id) === savedSpray) ?? null;\n\t\t}\n\t})\n\t.catch(() => {});",
	},
	{
		// The mod tab's link to a Reddit thread of (mostly dead) Dropbox
		// links now opens this server's list of restored packs.
		file: "core/src/components/tabs/ModTab.svelte",
		find: 'href="https://www.reddit.com/r/VertixOnline/comments/4vypx9/texture_mods_please_post_all_texture_mods_here/"',
		replace: 'href="/mods/"',
	},
];

// Restore each file once, so several patches can apply to the same file.
for (const file of new Set(patches.map((p) => p.file))) {
	execFileSync("git", ["-C", checkout, "checkout", "--", file]);
}

for (const p of patches) {
	const path = join(checkout, p.file);
	// Git on Windows usually checks files out with CRLF line endings; match
	// on LF and write the file back with the endings it had.
	const raw = readFileSync(path, "utf8");
	const crlf = raw.includes("\r\n");
	const text = crlf ? raw.replaceAll("\r\n", "\n") : raw;
	const at = text.indexOf(p.find);
	if (at < 0 || text.indexOf(p.find, at + 1) >= 0) {
		console.error(`patch for ${p.file} does not apply: passage not found exactly once`);
		process.exit(1);
	}
	const out = text.slice(0, at) + p.replace + text.slice(at + p.find.length);
	writeFileSync(path, crlf ? out.replaceAll("\n", "\r\n") : out);
	console.log(`patched ${p.file}`);
}
