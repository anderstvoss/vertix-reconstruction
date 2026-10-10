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
