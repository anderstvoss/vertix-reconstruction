/*
 * Independent compatibility shim for the unmodified 2016-08-06 Vertix client.
 *
 * The original keyDown() clears the opposite keyMap entry, losing track of
 * physically held keys. Register this AFTER app.js: the original event handler
 * runs first, and this listener then reconciles both keyMap and keys.
 * Do not copy or modify the archived first-party app.js.
 */
(function installOppositeKeyFix(win) {
  "use strict";
  if (win.__vertixOppositeKeyFixInstalled) return;
  win.__vertixOppositeKeyFixInstalled = true;

  const held = new Set();
  const directions = [
    ["upKey", "downKey", "u", "d"],
    ["leftKey", "rightKey", "l", "r"]
  ];

  function isMovementKey(code) {
    const bindings = win.keysList;
    return Boolean(bindings && directions.some(([a, b]) =>
      code === bindings[a] || code === bindings[b]));
  }

  function reconcile() {
    const bindings = win.keysList;
    const flags = win.keys;
    const originalKeyMap = win.keyMap;
    if (!bindings || !flags || !originalKeyMap) return;
    for (const [positive, negative, positiveFlag, negativeFlag] of directions) {
      const a = bindings[positive];
      const b = bindings[negative];
      const aDown = held.has(a);
      const bDown = held.has(b);
      // Preserve the physical state for repeated keydown and remapped keys.
      originalKeyMap[a] = aDown;
      originalKeyMap[b] = bDown;
      flags[positiveFlag] = Number(aDown && !bDown);
      flags[negativeFlag] = Number(bDown && !aDown);
    }
  }

  function onKeyDown(event) {
    if (win.keyToChange != null || !win.c ||
        win.document.activeElement !== win.c) return;
    const code = event.keyCode || event.which;
    if (!isMovementKey(code)) return;
    held.add(code); // keyboard auto-repeat must not change held state
    reconcile();
  }

  function onKeyUp(event) {
    const code = event.keyCode || event.which;
    if (!held.delete(code) && !isMovementKey(code)) return;
    reconcile();
  }

  function releaseAll() {
    held.clear();
    reconcile();
  }

  win.addEventListener("keydown", onKeyDown, false);
  // keyup bubbles from the canvas (the original listens on the canvas).
  win.addEventListener("keyup", onKeyUp, false);
  win.addEventListener("blur", releaseAll, false);
  win.document.addEventListener("visibilitychange", function () {
    if (win.document.hidden) releaseAll();
  });
})(window);
