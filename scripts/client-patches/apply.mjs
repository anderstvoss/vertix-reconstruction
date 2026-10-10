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
