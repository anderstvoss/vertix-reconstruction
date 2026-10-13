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
		// Sprays: placed by their centre, so an image that is not square
		// (see cacheSpray below) is still centred where it was sprayed.
		file: "core/src/app.tsx",
		find: "\ttmpSpray.xPos = x - tmpSpray.scale! / 2;\n\ttmpSpray.yPos = y - tmpSpray.scale! / 2;",
		replace: "\t// The spray's centre; cacheSpray sizes it.\n\ttmpSpray.xPos = x;\n\ttmpSpray.yPos = y;",
	},
	{
		file: "core/src/app.tsx",
		find: "function cacheSpray(img: Sprite) {\n\tconst tmpIndex = `${img.src}`;\n\tlet tmpSpray = cachedSprays[tmpIndex];\n\tif (tmpSpray || img.width === 0) return;\n\n\tlet initialCanvas = document.createElement(\"canvas\");\n\tlet initialCtx = initialCanvas.getContext(\"2d\")!;\n\tinitialCanvas.width = img.resolution!;\n\tinitialCanvas.height = img.resolution!;\n\tinitialCtx.drawImage(img, 0, 0, img.resolution!, img.resolution!);\n\tlet finalCanvas = document.createElement(\"canvas\");\n\tlet finalCtx = finalCanvas.getContext(\"2d\")!;\n\tfinalCanvas.width = img.scale!;\n\tfinalCanvas.height = img.scale!;\n\tfinalCtx.imageSmoothingEnabled = false;\n\tfinalCtx.globalAlpha = img.alpha!;\n\tfinalCtx.drawImage(initialCanvas, 0, 0, img.scale!, img.scale!);\n\ttmpSpray = finalCanvas;\n\tcachedSprays[tmpIndex] = tmpSpray;\n}\nfunction drawSprays() {\n\tif (!st.settings.showSprays) return;\n\tfor (const sp of userSprays) {\n\t\tif (!sp.active) continue;\n\t\tlet tmpSpray = cachedSprays[`${sp.src}`];\n\t\tif (!tmpSpray) continue;\n\t\tgraph.drawImage(tmpSpray, sp.xPos! - st.startX, sp.yPos! - st.startY);\n\t}\n}",
		replace: "// Sprays are sized as in the 2016 client and KRP: the image is shrunk to\n// the spray's `resolution` (30 px) and blown up to its `scale` (64 px)\n// with hard pixels. The one change: an image that is not square keeps its\n// shape inside that square instead of being stretched to fill it.\nconst sprayDrawSize: Record<string, [number, number]> = {};\nfunction cacheSpray(img: Sprite) {\n\tconst tmpIndex = `${img.src}`;\n\tif (cachedSprays[tmpIndex] || img.naturalWidth === 0) return;\n\tconst w = img.naturalWidth;\n\tconst h = img.naturalHeight;\n\tconst k = Math.max(w, h);\n\tconst res = img.resolution || 30;\n\tconst scale = img.scale || 64;\n\tconst small = document.createElement(\"canvas\");\n\tsmall.width = Math.max(1, Math.round((res * w) / k));\n\tsmall.height = Math.max(1, Math.round((res * h) / k));\n\tsmall.getContext(\"2d\")!.drawImage(img, 0, 0, small.width, small.height);\n\tconst canvas = document.createElement(\"canvas\");\n\tconst ctx = canvas.getContext(\"2d\")!;\n\tcanvas.width = Math.max(1, Math.round((scale * w) / k));\n\tcanvas.height = Math.max(1, Math.round((scale * h) / k));\n\tctx.imageSmoothingEnabled = false;\n\tctx.globalAlpha = img.alpha ?? 1;\n\tctx.drawImage(small, 0, 0, canvas.width, canvas.height);\n\tcachedSprays[tmpIndex] = canvas;\n\tsprayDrawSize[tmpIndex] = [canvas.width, canvas.height];\n}\nfunction drawSprays() {\n\tif (!st.settings.showSprays) return;\n\tfor (const sp of userSprays) {\n\t\tif (!sp.active) continue;\n\t\tconst tmpSpray = cachedSprays[`${sp.src}`];\n\t\tconst size = sprayDrawSize[`${sp.src}`];\n\t\tif (!tmpSpray || !size) continue;\n\t\tconst [dw, dh] = size;\n\t\tgraph.drawImage(tmpSpray, sp.xPos! - dw / 2 - st.startX, sp.yPos! - dh / 2 - st.startY);\n\t}\n}",
	},
	{
		// Sprays added on the server (`[content] sprays_dir`) join the
		// spray list.
		file: "core/src/state.svelte.ts",
		find: "window.st = st;",
		replace: "window.st = st;\n\n// Sprays the server adds (PNG files in its sprays folder). The saved choice\n// is read now, before the loadout tab clears a spray it does not know yet.\nconst savedSpray = localStorage.getItem(\"prevSpray\");\nfetch(\"/sprays/index.json\")\n\t.then((r) => (r.ok ? r.json() : { sprays: [] }))\n\t.then((d: { sprays: (typeof sprays)[number][] }) => {\n\t\tfor (const spray of d.sprays) {\n\t\t\tif (!st.sprays.some((s) => s.id === spray.id)) st.sprays.push(spray);\n\t\t}\n\t\tif (!st.loadout.spray && savedSpray) {\n\t\t\tst.loadout.spray = st.sprays.find((s) => String(s.id) === savedSpray) ?? null;\n\t\t}\n\t})\n\t.catch(() => {});",
	},
	{
		// Several sprays per player (`sprays_per_player` on the server).
		file: "core/src/app.tsx",
		find: "function createSpray(plrIdx: number, x: number, y: number) {\n\tlet tmpPlayer = findUserByIndex(plrIdx);\n\tif (!tmpPlayer) return;\n\tlet tmpSpray = userSprays.find((s) => s.owner === plrIdx);\n\tif (!tmpSpray) {\n\t\tconst img = new Image() as Sprite;\n\t\timg.owner = plrIdx;",
		replace: "// `slot` (from this server) lets a player keep several sprays on the map:\n// the server cycles it through `sprays_per_player` slots.\nfunction createSpray(plrIdx: number, x: number, y: number, slot = 0) {\n\tlet tmpPlayer = findUserByIndex(plrIdx);\n\tif (!tmpPlayer) return;\n\tlet tmpSpray = userSprays.find(\n\t\t(s) => s.owner === plrIdx && ((s as Sprite & { spraySlot?: number }).spraySlot ?? 0) === slot,\n\t);\n\tif (!tmpSpray) {\n\t\tconst img = new Image() as Sprite & { spraySlot?: number };\n\t\t// Not `slot`: every element already has a string `slot` property.\n\t\timg.spraySlot = slot;\n\t\timg.owner = plrIdx;",
	},
	{
		// KRP ignores `add` for the player themself, so after changing spray in
		// game they kept placing the old one (others saw the new one). Take
		// the spray from it; nothing else of their own is replaced.
		file: "core/src/app.tsx",
		find: "\t\t\tst.players[st.players.indexOf(existingUser)] = parsed;\n\t\t}\n\t}\n}",
		replace: "\t\t\tst.players[st.players.indexOf(existingUser)] = parsed;\n\t\t}\n\t} else if (parsed.spray) {\n\t\tst.player.spray = parsed.spray;\n\t\tconst me = findUserByIndex(parsed.index);\n\t\tif (me) me.spray = parsed.spray;\n\t}\n}",
	},
	{
		// Hidden tabs: drop shots that arrived while frames were stalled
		file: "core/src/app.tsx",
		find: "function someoneShot(evt: ShootEvent) {\n\tif (evt.i !== st.player.index) {\n\t\tconst tmpPlayer = findUserByIndex(evt.i);\n\t\tconst bullet = findServerBullet(evt.si);\n\t\tif (tmpPlayer && bullet) {\n\t\t\tshootNextBullet(evt, tmpPlayer, target.d, currentTime, bullet);\n\t\t}\n\t}\n}",
		replace: "// Hidden tabs: the browser stops drawing frames but shots keep arriving.\n// KRP armed them all with the frozen frame clock and fired them together\n// when the tab came back. Here a shot remembers when it arrived and is\n// dropped if frames did not run soon after (Projectile.update), shots that\n// arrive while frames are stalled play no sound, and one frame never runs\n// a longer step than MAX_FRAME_MS. localStorage `vertix.hiddenTab` = \"krp\"\n// restores KRP's behaviour.\nconst KRP_HIDDEN_TAB = (() => {\n\ttry {\n\t\treturn localStorage.getItem(\"vertix.hiddenTab\") === \"krp\";\n\t} catch {\n\t\treturn false;\n\t}\n})();\nconst STALE_SHOT_MS = 200;\nconst MAX_FRAME_MS = 100;\nfunction someoneShot(evt: ShootEvent) {\n\tif (evt.i !== st.player.index) {\n\t\tconst tmpPlayer = findUserByIndex(evt.i);\n\t\tconst bullet = findServerBullet(evt.si);\n\t\tif (tmpPlayer && bullet) {\n\t\t\tconst now = Date.now();\n\t\t\tif (!KRP_HIDDEN_TAB) {\n\t\t\t\tbullet.silent = document.hidden || now - currentTime > STALE_SHOT_MS;\n\t\t\t}\n\t\t\tshootNextBullet(evt, tmpPlayer, target.d, currentTime, bullet);\n\t\t\tif (!KRP_HIDDEN_TAB) bullet.arrivedAt = now;\n\t\t}\n\t}\n}",
	},
	{
		file: "core/src/app.tsx",
		find: "\tdelta = currentTime - oldTime;\n",
		replace: "\tdelta = currentTime - oldTime;\n\t// After a hidden tab one frame would run the whole gap as one step.\n\tif (!KRP_HIDDEN_TAB) delta = Math.min(delta, MAX_FRAME_MS);\n",
	},
	{
		file: "core/src/logic/projectile.ts",
		find: "\tselfDamage = false;\n\tupdate(",
		replace: "\tselfDamage = false;\n\t// Set by someoneShot (hidden-tab handling, see app.tsx).\n\tarrivedAt = 0;\n\tsilent = false;\n\tupdate(",
	},
	{
		file: "core/src/logic/projectile.ts",
		find: "\t\t\tif (this.skipMove) {\n\t\t\t\tlifetime = 0;\n\t\t\t\tthis.startTime = currentTime;\n\t\t\t}",
		replace: "\t\t\tif (this.skipMove) {\n\t\t\t\t// A shot that arrived while frames were not running is in\n\t\t\t\t// the past: drop it instead of firing it late.\n\t\t\t\tconst late = this.arrivedAt ? currentTime - this.arrivedAt : 0;\n\t\t\t\tthis.arrivedAt = 0;\n\t\t\t\tif (late > 200) {\n\t\t\t\t\tthis.active = false;\n\t\t\t\t\tthis.trailAlpha = 0;\n\t\t\t\t\tthis.skipMove = false;\n\t\t\t\t\treturn;\n\t\t\t\t}\n\t\t\t\tlifetime = 0;\n\t\t\t\tthis.startTime = currentTime;\n\t\t\t}",
	},
	{
		file: "core/src/logic/projectile.ts",
		find: "\t\tthis.active = true;\n\t\tif (typeof window !== \"undefined\") playSound(`shot${this.weaponIndex}`, this.x, this.y);",
		replace: "\t\tthis.active = true;\n\t\tthis.arrivedAt = 0;\n\t\tif (typeof window !== \"undefined\" && !this.silent) {\n\t\t\tplaySound(`shot${this.weaponIndex}`, this.x, this.y);\n\t\t}\n\t\tthis.silent = false;",
	},
	{
		// Flag waving: the client steps a flag's animation once every five
		// drawn frames. The 2016 client drew at 30 fps, so a flag changed
		// picture every 167 ms; KRP draws every display refresh, so flags
		// wave 2x faster at 60 Hz and almost 5x at 144 Hz. The countdown now
		// runs on elapsed time measured in 30 fps frames, which keeps the 2016
		// speed at any refresh rate. localStorage `vertix.flagSpeed` = "krp"
		// restores KRP's per-frame stepping.
		file: "core/src/app.tsx",
		find: "function drawFlag(flg: FlagObject) {\n\tflg.ac--;\n\tif (flg.ac <= 0) {\n\t\tflg.ac = 5;\n",
		replace: "const KRP_FLAG_SPEED = (() => {\n\ttry {\n\t\treturn localStorage.getItem(\"vertix.flagSpeed\") === \"krp\";\n\t} catch {\n\t\treturn false;\n\t}\n})();\nconst FLAG_FRAME_MS = 1000 / 30;\nfunction drawFlag(flg: FlagObject) {\n\tflg.ac -= KRP_FLAG_SPEED ? 1 : delta / FLAG_FRAME_MS;\n\tif (flg.ac <= 0) {\n\t\t// Keep the leftover so the average is exactly one picture per\n\t\t// five 2016 frames (6 per second).\n\t\tflg.ac = KRP_FLAG_SPEED ? 5 : Math.max(flg.ac + 5, 1);\n",
	},
	{
		// The mod tab's link to a Reddit thread of (mostly dead) Dropbox
		// links now opens this server's list of restored packs.
		file: "core/src/components/tabs/ModTab.svelte",
		find: 'href="https://www.reddit.com/r/VertixOnline/comments/4vypx9/texture_mods_please_post_all_texture_mods_here/"',
		replace: 'href="/mods/"',
	},
	{
		// Effects persist off screen (docs/DEVIATIONS.md): KRP deactivates
		// a particle (blood, dust, bullet hole) as soon as it is off screen.
		// Now it keeps ageing there and is drawn when it is back in view.
		// localStorage "vertix.effects" = "krp" restores KRP's behaviour.
		file: "core/src/visual/particle.ts",
		find: [
			"\tfor (let i = 0; i < cachedParticles.length; ++i) {",
			"\t\tif (",
			"\t\t\t(st.settings.showParticles || cachedParticles[i].forceShow) &&",
			"\t\t\tcachedParticles[i].active &&",
			"\t\t\tcanSee(",
			"\t\t\t\tcachedParticles[i].x - st.startX,",
			"\t\t\t\tcachedParticles[i].y - st.startY,",
			"\t\t\t\tcachedParticles[i].scale,",
			"\t\t\t\tcachedParticles[i].scale,",
			"\t\t\t)",
			"\t\t) {",
			"\t\t\tif (layer === cachedParticles[i].layer) {",
			"\t\t\t\tcachedParticles[i].update(delta);",
			"\t\t\t\tcachedParticles[i].draw();",
			"\t\t\t}",
		].join("\n"),
		replace: [
			"\tconst persist = localStorage.getItem(\"vertix.effects\") !== \"krp\";",
			"\tfor (let i = 0; i < cachedParticles.length; ++i) {",
			"\t\tconst visible = canSee(",
			"\t\t\tcachedParticles[i].x - st.startX,",
			"\t\t\tcachedParticles[i].y - st.startY,",
			"\t\t\tcachedParticles[i].scale,",
			"\t\t\tcachedParticles[i].scale,",
			"\t\t);",
			"\t\tif (",
			"\t\t\t(st.settings.showParticles || cachedParticles[i].forceShow) &&",
			"\t\t\tcachedParticles[i].active &&",
			"\t\t\t(visible || persist)",
			"\t\t) {",
			"\t\t\tif (layer === cachedParticles[i].layer) {",
			"\t\t\t\tcachedParticles[i].update(delta);",
			"\t\t\t\tif (visible) cachedParticles[i].draw();",
			"\t\t\t}",
		].join("\n"),
	},
	{
		// Bullet holes follow the server (docs/DEVIATIONS.md): gameSetup
		// carries `bulletHoles`, and when it is false a bullet hitting a
		// wall leaves no hole sprite. A server that does not send it keeps
		// KRP's holes.
		file: "core/src/visual/particle.ts",
		find: "\t\tif (i === 0 && spriteIndex === 2 && addBulletHole) {",
		replace: "\t\tif (i === 0 && spriteIndex === 2 && addBulletHole && particleRules.bulletHoles) {",
	},
	{
		file: "core/src/visual/particle.ts",
		find: "export function updateParticles(delta: number, layer: number) {",
		replace: [
			"export const particleRules = { bulletHoles: true };",
			"",
			"export function updateParticles(delta: number, layer: number) {",
		].join("\n"),
	},
	{
		file: "core/src/app.tsx",
		find: "\tparticleCone,\n\tstillDustParticle,\n\tupdateParticles,\n} from \"./visual/particle.ts\";",
		replace: "\tparticleCone,\n\tparticleRules,\n\tstillDustParticle,\n\tupdateParticles,\n} from \"./visual/particle.ts\";",
	},
	{
		file: "core/src/app.tsx",
		find: "\t\t\tconst setupData = JSON.parse(setupJson);",
		replace: [
			"\t\t\tconst setupData = JSON.parse(setupJson);",
			"\t\t\tparticleRules.bulletHoles = setupData.bulletHoles !== false;",
		].join("\n"),
	},
	{
		// Blood for hits this client cannot see (docs/DEVIATIONS.md): KRP
		// only bleeds a player who is on screen, so a hit off screen leaves
		// nothing to find later. The hit message has no position, so the
		// blood goes where the bullet is.
		file: "core/src/app.tsx",
		find: [
			"\t\t\tlet serverBullet = findServerBullet(healthUpdate.bulletIndex);",
			"\t\t\tif (serverBullet && serverBullet.owner?.index !== st.player.index) {",
		].join("\n"),
		replace: [
			"\t\t\tlet serverBullet = findServerBullet(healthUpdate.bulletIndex);",
			"\t\t\tif (",
			"\t\t\t\tserverBullet &&",
			"\t\t\t\t!player?.onScreen &&",
			"\t\t\t\thealthDelta < 0 &&",
			"\t\t\t\tserverBullet.spriteIndex !== 2 &&",
			"\t\t\t\tlocalStorage.getItem(\"vertix.effects\") !== \"krp\"",
			"\t\t\t) {",
			"\t\t\t\tparticleCone(",
			"\t\t\t\t\t12,",
			"\t\t\t\t\tserverBullet.x,",
			"\t\t\t\t\tserverBullet.y,",
			"\t\t\t\t\tserverBullet.dir + Math.PI,",
			"\t\t\t\t\tMath.PI / randomInt(5, 7),",
			"\t\t\t\t\t0.5,",
			"\t\t\t\t\t16,",
			"\t\t\t\t\t0,",
			"\t\t\t\t\ttrue,",
			"\t\t\t\t);",
			"\t\t\t\tcreateLiquid(serverBullet.x, serverBullet.y, serverBullet.dir, 4);",
			"\t\t\t}",
			"\t\t\tif (serverBullet && serverBullet.owner?.index !== st.player.index) {",
		].join("\n"),
	},
	{
		// 2016 floor tiles (docs/DEVIATIONS.md): KRP's res.zip has the
		// plainer 2020 ground tiles. The 2016 ones (specks, cracks, and the
		// planks on ground2, which maps place on their yellow cells) come
		// from this server's original-2016 pack when it has one.
		// localStorage "vertix.floor" = "krp" keeps KRP's tiles.
		file: "core/src/app.tsx",
		find: [
			"\t\tspriteIndex = 0;",
			"\t\tloadPlayerSprites(\"sprites/\");",
			"\t\tloadDefaultSprites(\"sprites/\");",
		].join("\n"),
		replace: [
			"\t\tif (isBaseAssets && localStorage.getItem(\"vertix.floor\") !== \"krp\") {",
			"\t\t\ttry {",
			"\t\t\t\tconst floorReader = new zip.ZipReader(new zip.HttpReader(\"/mods/original-2016/vertixmod.zip\"));",
			"\t\t\t\tfor (const floorEntry of await floorReader.getEntries()) {",
			"\t\t\t\t\tif (floorEntry.directory) continue;",
			"\t\t\t\t\tconst name = floorEntry.filename.replace(\"vertixmod/\", \"\");",
			"\t\t\t\t\tif (!/^sprites\\/ground[123]\\.png$/.test(name)) continue;",
			"\t\t\t\t\tconst blob = await floorEntry.getData(new zip.BlobWriter(\"image/png\"));",
			"\t\t\t\t\tlocalStorage.setItem(name, URL.createObjectURL(blob));",
			"\t\t\t\t}",
			"\t\t\t\tawait floorReader.close();",
			"\t\t\t} catch (err) {",
			"\t\t\t\tconsole.warn(\"2016 floor tiles not loaded\", err);",
			"\t\t\t}",
			"\t\t}",
			"\t\tspriteIndex = 0;",
			"\t\tloadPlayerSprites(\"sprites/\");",
			"\t\tloadDefaultSprites(\"sprites/\");",
		].join("\n"),
	},
	{
		// Frame rate: sub-millisecond frame times and the helpers below (docs/DEVIATIONS.md).
		file: "core/src/app.tsx",
		find: "function updateGameLoop() {\n\tdelta = currentTime - oldTime;",
		replace: "// Frame rate (docs/DEVIATIONS.md): KRP moves the player by each frame's\n// length, rounding to whole pixels every frame, so the speed depends on the\n// display, and it times frames with Date.now() (whole milliseconds). Here\n// the player moves in fixed 60 Hz input steps (the server applies one step\n// per packet, rounding the same way) and is drawn between steps, other\n// players glide between the positions the server sends instead of jumping,\n// and the screen shake fades by time. localStorage `vertix.frameRate` =\n// \"krp\" restores KRP's.\nconst KRP_FRAME_RATE = (() => {\n\ttry {\n\t\treturn localStorage.getItem(\"vertix.frameRate\") === \"krp\";\n\t} catch {\n\t\treturn false;\n\t}\n})();\nconst INPUT_STEP_MS = 1000 / 60;\nconst MAX_STEPS_PER_FRAME = 6;\nconst GLIDE_SNAP = 150;\ntype VxGlide = { fromX: number; fromY: number; t0: number; dur: number; lastMove: number };\nlet vxFrameClock = performance.now();\nlet vxFrameNow = vxFrameClock;\nlet vxInputAccum = 0;\nlet vxTickAlpha = 1;\nconst vxTickFrom = new Map<number, [number, number, number]>();\nconst vxGlides = new Map<number, VxGlide>();\nfunction vxGlideAt(g: VxGlide, toX: number, toY: number, now: number): [number, number] {\n\tconst t = g.dur > 0 ? Math.min(1, Math.max(0, (now - g.t0) / g.dur)) : 1;\n\treturn [g.fromX + (toX - g.fromX) * t, g.fromY + (toY - g.fromY) * t];\n}\n// Another player moved: glide from where they are drawn now to the new\n// position, over the time since their last move (their own input rate).\nfunction vxGlideTo(p: Player, before: [number, number]) {\n\tconst now = performance.now();\n\tconst g = vxGlides.get(p.index);\n\tconst [dx, dy] = g ? vxGlideAt(g, before[0], before[1], now) : before;\n\tconst far = Math.hypot(p.x - dx, p.y - dy) > GLIDE_SNAP;\n\tvxGlides.set(p.index, {\n\t\tfromX: far ? p.x : dx,\n\t\tfromY: far ? p.y : dy,\n\t\tt0: now,\n\t\tdur: Math.min(100, Math.max(8, now - (g ? g.lastMove : now))),\n\t\tlastMove: now,\n\t});\n}\n// Moves everyone to where they are drawn this frame; vxRestorePositions\n// puts the simulated positions back afterwards.\nfunction vxPlaceForDrawing() {\n\tconst sim: [Player, number, number, number][] = [];\n\tconst own: [number, number] = [st.player.x, st.player.y];\n\tif (KRP_FRAME_RATE) return { sim, own };\n\tfor (const p of st.players) {\n\t\tsim.push([p, p.x, p.y, p.jumpY]);\n\t\tconst from = vxTickFrom.get(p.index);\n\t\tif (from) {\n\t\t\tp.jumpY = from[2] + (p.jumpY - from[2]) * vxTickAlpha;\n\t\t\tif (p.index === st.player.index && Math.hypot(p.x - from[0], p.y - from[1]) <= GLIDE_SNAP) {\n\t\t\t\tp.x = from[0] + (p.x - from[0]) * vxTickAlpha;\n\t\t\t\tp.y = from[1] + (p.y - from[1]) * vxTickAlpha;\n\t\t\t}\n\t\t}\n\t\tif (p.index === st.player.index) {\n\t\t\t// The camera follows st.player.\n\t\t\tst.player.x = p.x;\n\t\t\tst.player.y = p.y;\n\t\t} else {\n\t\t\tconst g = vxGlides.get(p.index);\n\t\t\tif (g) [p.x, p.y] = vxGlideAt(g, p.x, p.y, vxFrameNow);\n\t\t}\n\t}\n\treturn { sim, own };\n}\nfunction vxRestorePositions(saved: ReturnType<typeof vxPlaceForDrawing>) {\n\tfor (const [p, x, y, jumpY] of saved.sim) {\n\t\tp.x = x;\n\t\tp.y = y;\n\t\tp.jumpY = jumpY;\n\t}\n\tst.player.x = saved.own[0];\n\tst.player.y = saved.own[1];\n}\nfunction updateGameLoop() {\n\tvxFrameNow = performance.now();\n\tdelta = KRP_FRAME_RATE ? currentTime - oldTime : vxFrameNow - vxFrameClock;\n\tvxFrameClock = vxFrameNow;",
	},
	{
		// Frame rate: the player moves in whole input steps (docs/DEVIATIONS.md).
		file: "core/src/app.tsx",
		find: "\tconst clientPrediction = true;\n\tif (clientPrediction) {\n\t\tfor (const plr of st.players) {\n\t\t\tif (plr.index === st.player.index) {\n\t\t\t\tplr.oldX = plr.x;\n\t\t\t\tplr.oldY = plr.y;\n\t\t\t\tif (!plr.dead && !st.gameOver) {\n\t\t\t\t\tplr.x += b * plr.speed * delta;\n\t\t\t\t\tplr.y += d * plr.speed * delta;\n\t\t\t\t}\n\t\t\t\twallCol(plr, st.gameMap.tiles, clutter);\n\t\t\t\tplr.x = Math.round(plr.x);\n\t\t\t\tplr.y = Math.round(plr.y);\n\t\t\t\tplr.angle = ((target.f + Math.PI * 2) % (Math.PI * 2)) * (180 / Math.PI) + 90;\n\t\t\t\tconst currentWeapon = getCurrentWeapon(plr);\n\t\t\t\tif (currentWeapon) {\n\t\t\t\t\tcurrentWeapon.front = isWeaponFacingFront(snapAngleToCardinal(plr.angle));\n\t\t\t\t}\n\t\t\t\tif (plr.jumpCountdown > 0) {\n\t\t\t\t\tplr.jumpCountdown -= delta;\n\t\t\t\t}\n\t\t\t\tif (keys.s && plr.jumpCountdown <= 0 && !st.gameOver) {\n\t\t\t\t\tplayerJump(plr);\n\t\t\t\t\tdoJump = 1;\n\t\t\t\t}\n\t\t\t}\n\t\t\tif (plr.jumpY !== 0) {\n\t\t\t\tplr.jumpDelta -= plr.gravityStrength * delta;\n\t\t\t\tplr.jumpY += plr.jumpDelta * delta;\n\t\t\t\tif (plr.jumpY > 0) {\n\t\t\t\t\tplr.animIndex = 1;\n\t\t\t\t} else {\n\t\t\t\t\tplr.jumpY = 0;\n\t\t\t\t\tplr.jumpDelta = 0;\n\t\t\t\t\tplr.jumpCountdown = 250;\n\t\t\t\t}\n\t\t\t\tplr.jumpY = Math.round(plr.jumpY);\n\t\t\t}\n\t\t\tif (plr.index === st.player.index && !st.gameOver) {\n\t\t\t\tlet sendData = {\n\t\t\t\t\thdt: b,\n\t\t\t\t\tvdt: d,\n\t\t\t\t\tts: currentTime,\n\t\t\t\t\tisn: inputNumber,\n\t\t\t\t\ts: doJump,\n\t\t\t\t\tdelta,\n\t\t\t\t};\n\t\t\t\tinputNumber++;\n\t\t\t\tthisInput.push(sendData);\n\t\t\t\tsocket.emit(\"4\", sendData);\n",
		replace: "\t// Whole input steps this frame (KRP: one step of the frame's length).\n\tconst steps: number[] = [];\n\tif (KRP_FRAME_RATE) {\n\t\tsteps.push(delta);\n\t\tvxTickAlpha = 1;\n\t} else {\n\t\tvxInputAccum += delta;\n\t\twhile (vxInputAccum >= INPUT_STEP_MS) {\n\t\t\tvxInputAccum -= INPUT_STEP_MS;\n\t\t\tif (steps.length < MAX_STEPS_PER_FRAME) steps.push(INPUT_STEP_MS);\n\t\t}\n\t\tif (vxInputAccum >= INPUT_STEP_MS) vxInputAccum = 0;\n\t\tvxTickAlpha = vxInputAccum / INPUT_STEP_MS;\n\t}\n\tconst clientPrediction = true;\n\tif (clientPrediction) {\n\t\tfor (const plr of st.players) {\n\t\t\tconst isMe = plr.index === st.player.index;\n\t\t\tif (isMe) {\n\t\t\t\tplr.angle = ((target.f + Math.PI * 2) % (Math.PI * 2)) * (180 / Math.PI) + 90;\n\t\t\t\tconst currentWeapon = getCurrentWeapon(plr);\n\t\t\t\tif (currentWeapon) {\n\t\t\t\t\tcurrentWeapon.front = isWeaponFacingFront(snapAngleToCardinal(plr.angle));\n\t\t\t\t}\n\t\t\t\tif (plr.jumpCountdown > 0) {\n\t\t\t\t\tplr.jumpCountdown -= delta;\n\t\t\t\t}\n\t\t\t}\n\t\t\tfor (let n = 0; n < steps.length; n++) {\n\t\t\t\tconst step = steps[n];\n\t\t\t\tvxTickFrom.set(plr.index, [plr.x, plr.y, plr.jumpY]);\n\t\t\t\tlet stepJump = 0;\n\t\t\t\tif (isMe) {\n\t\t\t\t\tplr.oldX = plr.x;\n\t\t\t\t\tplr.oldY = plr.y;\n\t\t\t\t\tif (!plr.dead && !st.gameOver) {\n\t\t\t\t\t\tplr.x += b * plr.speed * step;\n\t\t\t\t\t\tplr.y += d * plr.speed * step;\n\t\t\t\t\t}\n\t\t\t\t\twallCol(plr, st.gameMap.tiles, clutter);\n\t\t\t\t\tplr.x = Math.round(plr.x);\n\t\t\t\t\tplr.y = Math.round(plr.y);\n\t\t\t\t\tif (n === 0 && keys.s && plr.jumpCountdown <= 0 && !st.gameOver) {\n\t\t\t\t\t\tplayerJump(plr);\n\t\t\t\t\t\tstepJump = 1;\n\t\t\t\t\t}\n\t\t\t\t}\n\t\t\t\tif (plr.jumpY !== 0) {\n\t\t\t\t\tplr.jumpDelta -= plr.gravityStrength * step;\n\t\t\t\t\tplr.jumpY += plr.jumpDelta * step;\n\t\t\t\t\tif (plr.jumpY > 0) {\n\t\t\t\t\t\tplr.animIndex = 1;\n\t\t\t\t\t} else {\n\t\t\t\t\t\tplr.jumpY = 0;\n\t\t\t\t\t\tplr.jumpDelta = 0;\n\t\t\t\t\t\tplr.jumpCountdown = 250;\n\t\t\t\t\t}\n\t\t\t\t\tplr.jumpY = Math.round(plr.jumpY);\n\t\t\t\t}\n\t\t\t\tif (isMe && !st.gameOver) {\n\t\t\t\t\tconst sendData = {\n\t\t\t\t\t\thdt: b,\n\t\t\t\t\t\tvdt: d,\n\t\t\t\t\t\tts: currentTime,\n\t\t\t\t\t\tisn: inputNumber,\n\t\t\t\t\t\ts: stepJump,\n\t\t\t\t\t\tdelta: step,\n\t\t\t\t\t};\n\t\t\t\t\tinputNumber++;\n\t\t\t\t\tthisInput.push(sendData);\n\t\t\t\t\tsocket.emit(\"4\", sendData);\n\t\t\t\t}\n\t\t\t}\n\t\t\tif (plr.index === st.player.index && !st.gameOver) {\n",
	},
	{
		// Frame rate: draw everyone between updates.
		file: "core/src/app.tsx",
		find: "\tst.players.sort(sortUsersByPosition);\n\tif (!st.kicked) {",
		replace: "\tst.players.sort(sortUsersByPosition);\n\tconst vxDrawn = vxPlaceForDrawing();\n\tif (!st.kicked) {",
	},
	{
		// Frame rate: put the simulated positions back.
		file: "core/src/app.tsx",
		find: "\tif (disconnected || st.kicked) {\n\t\tdrawOverlay(graph, false, false);",
		replace: "\tvxRestorePositions(vxDrawn);\n\tif (disconnected || st.kicked) {\n\t\tdrawOverlay(graph, false, false);",
	},
	{
		// Frame rate: remember where another player was before this update.
		file: "core/src/app.tsx",
		find: "\t\t\t} else if (tmpUser != null) {\n",
		replace: "\t\t\t} else if (tmpUser != null) {\n\t\t\t\tconst vxBefore: [number, number] = [tmpUser.x, tmpUser.y];\n",
	},
	{
		// Frame rate: other players glide to their new position.
		file: "core/src/app.tsx",
		find: "\t\t\t\t\ttmpUser.nameYOffset = data[5 + cursor];\n\t\t\t\t}\n",
		replace: "\t\t\t\t\ttmpUser.nameYOffset = data[5 + cursor];\n\t\t\t\t}\n\t\t\t\tif (!KRP_FRAME_RATE && (tmpUser.x !== vxBefore[0] || tmpUser.y !== vxBefore[1])) {\n\t\t\t\t\tvxGlideTo(tmpUser, vxBefore);\n\t\t\t\t}\n",
	},
	{
		// Frame rate: the screen shake fades by time (0: KRP's, per frame).
		file: "core/src/app.tsx",
		find: "\tupdateScreenShake(/*delta*/);",
		replace: "\tupdateScreenShake(KRP_FRAME_RATE ? 0 : delta);",
	},
	{
		// Frame rate: KRP halves the shake every frame, so it fades faster on
		// a faster display; given the frame time it halves every 60th of a
		// second (KRP at 60 Hz) instead.
		file: "core/src/visual/shake.ts",
		find: "export function updateScreenShake() {",
		replace: "export function updateScreenShake(delta = 0) {",
	},
	{
		file: "core/src/visual/shake.ts",
		find: "\t\tst.shake.scale = st.shake.scale * screenSkRed;",
		replace: "\t\tst.shake.scale = st.shake.scale * (delta > 0 ? Math.pow(screenSkRed, delta / (1000 / 60)) : screenSkRed);",
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
