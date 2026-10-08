#!/usr/bin/env node
"use strict";
/**
 * Execute the archived Vertix 2016-08-06 setupMap/canPlaceFlag functions
 * against a synthetic mapData produced by a10_original_map_fixture.py.
 *
 * Usage: node analyses/a11_original_setupmap_probe.cjs APP_JS MAPDATA_JSON TILE_SCALE
 *
 * APP_JS is supplied externally from preservation; do not commit original app.js.
 * This tests tile decoding, NOT a complete browser boot or server behavior.
 */
const fs = require("node:fs");
const vm = require("node:vm");
const assert = require("node:assert/strict");
const crypto = require("node:crypto");
const [sourcePath, dataPath, scaleText] = process.argv.slice(2);
if (!sourcePath || !dataPath || !scaleText) {
  console.error("Usage: node a11_original_setupmap_probe.cjs APP_JS MAPDATA_JSON TILE_SCALE");
  process.exit(2);
}
const source = fs.readFileSync(sourcePath, "utf8");
const map = JSON.parse(fs.readFileSync(dataPath, "utf8"));
const scale = Number(scaleText);
assert(Number.isSafeInteger(scale) && scale > 0);
assert(map && map.genData && map.gameMode && Array.isArray(map.tiles));
assert(Array.isArray(map.genData.data.data));

function extractFunction(name) {
  const match = new RegExp("\\bfunction\\s+" + name + "\\s*\\(").exec(source);
  assert(match, "Function " + name + " not found in source");
  const start = match.index;
  const open = source.indexOf("{", start);
  let depth = 0, quote = null, escapeNext = false;
  // Sufficient for these specifically audited functions (no braces in regex
  // literals or backtick template literals).
  for (let i = open; i < source.length; i++) {
    const c = source[i];
    if (quote) {
      if (escapeNext) escapeNext = false;
      else if (c === "\\") escapeNext = true;
      else if (c === quote) quote = null;
      continue;
    }
    if (c === '"' || c === "'") { quote = c; continue; }
    if (c === "{") depth++;
    if (c === "}") {
      depth--;
      if (depth === 0) return source.slice(start, i + 1);
    }
  }
  throw new Error("Unbalanced function body for " + name);
}
function normalizedFnv(text) {
  let hash = 2166136261;
  for (const character of text.replace(/\s/g, "")) {
    hash ^= character.charCodeAt(0);
    hash = Math.imul(hash, 16777619);
  }
  return (hash >>> 0).toString(16);
}
// Fingerprints derived from preserved 2016-08-06 beautified JS source.
// They guard accidental mismatches but are NOT secure authenticity evidence.
const expected = {setupMap:"21ea5eca", canPlaceFlag:"2d34a3e5"};
const functions = {};
for (const [name, fingerprint] of Object.entries(expected)) {
  functions[name] = extractFunction(name);
  assert.equal(normalizedFnv(functions[name]), fingerprint,
    "Original function " + name + " fingerprint mismatch");
}
const sandbox = {
  mapTileScale: scale,
  gameObjects: [],
  randomInt: (low, high) => high, // deterministic cosmetic variants
  tmpY: null, tmpShad: null,
};
vm.createContext(sandbox);
vm.runInContext(functions.canPlaceFlag + "\n" + functions.setupMap,
  sandbox, {timeout: 5000});
sandbox.__map = map;
vm.runInContext("setupMap(__map)", sandbox, {timeout: 5000});
const width = map.genData.width;
const height = map.genData.height;
const pixels = map.genData.data.data;
assert.equal(pixels.length, width*height*4);
let black = 0, green = 0, yellow = 0;
for (let y = 0; y < height; y++) for (let x = 0; x < width; x++) {
  const start = (y * width + x) * 4;
  const pixel = pixels.slice(start, start + 3).join(",");
  if (pixel === "0,0,0" || (x === 0 && y === 0)) black++;
  else if (pixel === "0,255,0") green++;
  else if (pixel === "255,255,0") yellow++;
}
const tiles = map.tiles;
assert.equal(tiles.length, width*height);
assert.equal(tiles.filter(t => t.wall).length, black);
assert.equal(map.width, (width - 4)*scale);
assert.equal(map.height, (height - 4)*scale);
assert.equal(map.tilePerCol, height);
const hardpoints = ["Hardpoint", "Zone War"].includes(map.gameMode.name) ? yellow : 0;
assert.equal(tiles.filter(t => t.hardPoint).length, hardpoints);
assert.equal(tiles.filter(t => t.spriteIndex === 2).length, green);
console.log(JSON.stringify({
  source_sha256: crypto.createHash("sha256").update(source).digest("hex"),
  source_function_fingerprints: expected,
  fixture_dimensions: [width,height],
  mode: map.gameMode.name,
  tiles: tiles.length,
  walls: black,
  green_tiles: green,
  yellow_tiles: yellow,
  hardpoints,
  flags: sandbox.gameObjects.filter(o => o.type === "flag").length,
  world_dimensions: [map.width,map.height],
  nested_rgba_length: pixels.length,
  pass: true,
}));
