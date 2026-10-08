"use strict";
const { test } = require("node:test");
const assert = require("node:assert/strict");
const fs = require("node:fs");
const vm = require("node:vm");
const path = require("node:path");

function rig() {
  const listeners = Object.create(null);
  const documentListeners = Object.create(null);
  const canvas = {};
  const win = {
    c: canvas,
    keys: { u: 0, d: 0, l: 0, r: 0 },
    keyMap: [],
    keysList: { upKey: 87, downKey: 83, leftKey: 65, rightKey: 68 },
    keyToChange: null,
    document: {
      activeElement: canvas,
      hidden: false,
      addEventListener(type, fn) { (documentListeners[type] ??= []).push(fn); }
    },
    addEventListener(type, fn) { (listeners[type] ??= []).push(fn); }
  };
  // Model original handler registration order, including its documented
  // deletion of opposite key state (August 2016 app.js, lines 451-477).
  win.addEventListener("keydown", (event) => {
    if (win.document.activeElement !== canvas) return;
    const key = event.keyCode;
    win.keyMap[key] = true;
    for (const [a, b, fa, fb] of [
      [win.keysList.upKey, win.keysList.downKey, "u", "d"],
      [win.keysList.leftKey, win.keysList.rightKey, "l", "r"]
    ]) {
      if (key === a) {
        win.keys[fa] = 1; win.keys[fb] = 0; win.keyMap[b] = false;
      } else if (key === b) {
        win.keys[fb] = 1; win.keys[fa] = 0; win.keyMap[a] = false;
      }
    }
  });
  win.addEventListener("keyup", event => {
    win.keyMap[event.keyCode] = false;
    if (event.keyCode === win.keysList.upKey) win.keys.u = 0;
    if (event.keyCode === win.keysList.downKey) win.keys.d = 0;
    if (event.keyCode === win.keysList.leftKey) win.keys.l = 0;
    if (event.keyCode === win.keysList.rightKey) win.keys.r = 0;
  });
  const source = fs.readFileSync(path.join(__dirname, "../../web/input-opposites.js"), "utf8");
  vm.runInNewContext(source, { window: win });
  const emit = (type, code) => {
    for (const handler of (listeners[type] || [])) handler({ keyCode: code });
  };
  return { win, emit, visibility(hidden) {
    win.document.hidden = hidden;
    for (const fn of (documentListeners.visibilitychange || [])) fn();
  } };
}

test("W + S cancels and releasing S resumes still-held W", () => {
  const { win, emit } = rig();
  emit("keydown", 87);
  assert.deepEqual([win.keys.u, win.keys.d], [1, 0]);
  emit("keydown", 83);
  assert.deepEqual([win.keys.u, win.keys.d], [0, 0]);
  assert.equal(win.keyMap[87], true);
  assert.equal(win.keyMap[83], true);
  emit("keyup", 83);
  assert.deepEqual([win.keys.u, win.keys.d], [1, 0]);
  emit("keyup", 87);
  assert.deepEqual([win.keys.u, win.keys.d], [0, 0]);
});

test("opposite key order is symmetric and auto-repeat is inert", () => {
  const { win, emit } = rig();
  emit("keydown", 83);
  emit("keydown", 87);
  emit("keydown", 87);
  assert.deepEqual([win.keys.u, win.keys.d], [0, 0]);
  emit("keyup", 87);
  assert.deepEqual([win.keys.u, win.keys.d], [0, 1]);
});

test("A + D cancels; custom bindings are used", () => {
  const { win, emit } = rig();
  win.keysList.leftKey = 74;
  win.keysList.rightKey = 76;
  emit("keydown", 74);
  emit("keydown", 76);
  assert.deepEqual([win.keys.l, win.keys.r], [0, 0]);
  emit("keyup", 74);
  assert.deepEqual([win.keys.l, win.keys.r], [0, 1]);
});

test("blur and hidden document release every movement direction", () => {
  const { win, emit, visibility } = rig();
  emit("keydown", 87);
  emit("blur");
  assert.deepEqual([win.keys.u, win.keys.d], [0, 0]);
  emit("keydown", 65);
  visibility(true);
  assert.deepEqual([win.keys.l, win.keys.r], [0, 0]);
});

test("typing into chat does not trigger movement", () => {
  const { win, emit } = rig();
  win.document.activeElement = {};
  emit("keydown", 87);
  assert.deepEqual([win.keys.u, win.keys.d], [0, 0]);
});
