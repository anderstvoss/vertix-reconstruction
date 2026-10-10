// Runs in the page before KRP's client loads, from the desktop wrapper
// (desktop/krp) or from tools/ab for KRP in a browser. It changes KRP's
// behaviour only through browser APIs the client calls (requestAnimationFrame,
// the canvas, WebSocket), so none of KRP's code is copied or patched.
//
// Options come from window.__vertixShell, set just before this script:
//   input     "frame" (KRP: one input per drawn frame) or a rate in Hz. KRP
//             draws and sends input in the same loop, so a rate caps that
//             loop: on a faster display it draws and sends at most `input`
//             times a second.
//   display   "sharp" (draw the game canvas at the device pixel ratio) or
//             "krp" (KRP's CSS-pixel canvas, unchanged).
//   name      player name to type into the start menu.
//   autoplay  press ENTER GAME once the room is joined.
//   script    scripted keys, the Rust client's format: "d:1500,s:800,a+w:1000,:500".
//   duration  seconds to run, then report metrics.
//   label     "desktop" or "browser", recorded in the metrics.
//
// Metrics match the Rust client's (crates/client): frame times, inputs and
// server updates per second, ping. They are set on window.vertixMetrics and,
// in the desktop wrapper, posted to it over window.ipc.
(() => {
	"use strict";
	const opts = Object.assign(
		{ input: "frame", display: "sharp", autoplay: false, label: "browser" },
		window.__vertixShell || {},
	);
	const now = () => performance.now();
	const started = now();

	// --- Loop rate (KRP's input rate) and frame times -----------------------
	const nativeRaf = window.requestAnimationFrame.bind(window);
	const hz = opts.input === "frame" ? 0 : Number.parseFloat(opts.input);
	const minGap = hz >= 10 && hz <= 1000 ? 1000 / hz : 0;
	let lastRun = 0;
	const frameMs = [];
	let lastFrame = 0;
	// Every callback of one display frame gets the same timestamp, so a frame
	// is let through, and counted, once per timestamp.
	window.requestAnimationFrame = (cb) =>
		nativeRaf(function gate(t) {
			if (t !== lastRun) {
				// Allow 1 ms of vsync jitter so a 60 Hz cap on a 60 Hz
				// display does not skip frames.
				if (minGap && lastRun && t - lastRun < minGap - 1) {
					nativeRaf(gate);
					return;
				}
				lastRun = t;
				const n = now();
				if (lastFrame) frameMs.push(n - lastFrame);
				lastFrame = n;
			}
			cb(t);
		});

	// --- Sharp display ------------------------------------------------------
	// KRP sizes its game canvas (#cvs) in CSS pixels, so on a high-DPI screen
	// the browser upscales it. Here the canvas keeps reporting CSS pixels to
	// KRP while its backing store is device pixels, and every transform KRP
	// sets on it is scaled by the device pixel ratio.
	if (opts.display === "sharp") {
		const dpr = () => window.devicePixelRatio || 1;
		const isGame = (c) => c && c.id === "cvs";
		const proto = HTMLCanvasElement.prototype;
		const ctxProto = CanvasRenderingContext2D.prototype;
		const nativeSetTransform = ctxProto.setTransform;
		for (const side of ["width", "height"]) {
			const desc = Object.getOwnPropertyDescriptor(proto, side);
			Object.defineProperty(proto, side, {
				configurable: true,
				enumerable: desc.enumerable,
				get() {
					const v = desc.get.call(this);
					return isGame(this) ? Math.round(v / dpr()) : v;
				},
				set(v) {
					if (!isGame(this)) {
						desc.set.call(this, v);
						return;
					}
					const r = dpr();
					desc.set.call(this, Math.round(v * r));
					this.style[side] = `${v}px`;
					// Resizing resets the context; start it at the device scale.
					const ctx = this.getContext("2d");
					nativeSetTransform.call(ctx, r, 0, 0, r, 0, 0);
				},
			});
		}
		ctxProto.setTransform = function (a, b, c, d, e, f) {
			if (!isGame(this.canvas) || typeof a !== "number") {
				return nativeSetTransform.apply(this, arguments);
			}
			const r = dpr();
			return nativeSetTransform.call(this, a * r, b * r, c * r, d * r, e * r, f * r);
		};
		ctxProto.resetTransform = function () {
			if (!isGame(this.canvas)) return nativeSetTransform.call(this, 1, 0, 0, 1, 0, 0);
			const r = dpr();
			return nativeSetTransform.call(this, r, 0, 0, r, 0, 0);
		};
		// A move to a screen with another pixel ratio: KRP resizes on "resize".
		let lastDpr = dpr();
		setInterval(() => {
			if (dpr() !== lastDpr) {
				lastDpr = dpr();
				window.dispatchEvent(new Event("resize"));
			}
		}, 500);
	}

	// --- Network counters -----------------------------------------------------
	// socket.io frames: 42/<room>,["event",...]
	const eventOf = (data) => {
		if (typeof data !== "string" || !data.startsWith("42")) return null;
		const m = /^42(?:\/[^,]*,)?\["([^"]*)"/.exec(data);
		return m ? m[1] : null;
	};
	let inputsSent = 0;
	let updatesReceived = 0;
	let ping = 0;
	let pingStart = 0;
	const NativeWS = window.WebSocket;
	const nativeSend = NativeWS.prototype.send;
	NativeWS.prototype.send = function (data) {
		const ev = eventOf(data);
		if (ev === "4") inputsSent++;
		else if (ev === "ping1") pingStart = now();
		return nativeSend.call(this, data);
	};
	window.WebSocket = class extends NativeWS {
		constructor(...args) {
			super(...args);
			this.addEventListener("message", (e) => {
				const ev = eventOf(e.data);
				if (ev === "rsd") updatesReceived++;
				else if (ev === "pong1" && pingStart) ping = Math.round(now() - pingStart);
			});
		}
	};

	// --- Start menu and scripted input ----------------------------------------
	const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
	const codes = { w: "KeyW", a: "KeyA", s: "KeyS", d: "KeyD", space: "Space", r: "KeyR" };
	const key = (type, code) => {
		const target = type === "keyup" ? document.getElementById("cvs") || window : window;
		target.dispatchEvent(new KeyboardEvent(type, { code, bubbles: true }));
	};
	async function play() {
		if (opts.name) {
			for (let i = 0; i < 100; i++) {
				const input = document.getElementById("playerNameInput");
				if (input) {
					input.value = opts.name;
					input.dispatchEvent(new Event("input", { bubbles: true }));
					break;
				}
				await sleep(100);
			}
		}
		if (!opts.autoplay) return;
		for (let i = 0; i < 300; i++) {
			const menu = document.getElementById("startMenuWrapper");
			if (menu && menu.style.display === "none") break;
			if (typeof window.startGame === "function") window.startGame();
			await sleep(250);
		}
		if (!opts.script) return;
		await sleep(1000);
		for (const step of String(opts.script).split(",")) {
			const [keys, ms] = step.split(":");
			const held = keys ? keys.split("+").map((k) => codes[k.trim()]).filter(Boolean) : [];
			for (const c of held) key("keydown", c);
			await sleep(Number.parseFloat(ms) || 0);
			for (const c of held) key("keyup", c);
		}
	}
	window.addEventListener("load", () => {
		play();
	});

	// --- Report -------------------------------------------------------------------
	function metrics() {
		const sorted = [...frameMs].sort((a, b) => a - b);
		const pct = (p) => (sorted.length ? sorted[Math.round((sorted.length - 1) * p)] : 0);
		const mean = sorted.length ? sorted.reduce((a, b) => a + b, 0) / sorted.length : 0;
		const secs = (now() - started) / 1000;
		return {
			client: "krp",
			target: opts.label,
			options: opts,
			seconds: secs,
			frames: frameMs.length,
			fps_mean: mean > 0 ? 1000 / mean : 0,
			frame_ms: { mean, p50: pct(0.5), p95: pct(0.95), p99: pct(0.99), max: pct(1) },
			inputs_sent: inputsSent,
			inputs_per_second: inputsSent / Math.max(secs, 0.001),
			updates_received: updatesReceived,
			updates_per_second: updatesReceived / Math.max(secs, 0.001),
			ping_ms: ping,
			dpi_scale: window.devicePixelRatio || 1,
			css_size: [window.innerWidth, window.innerHeight],
		};
	}
	window.vertixMetricsNow = metrics;
	const duration = Number.parseFloat(opts.duration);
	if (duration > 0) {
		setTimeout(() => {
			const m = metrics();
			window.vertixMetrics = m;
			if (window.ipc) window.ipc.postMessage(JSON.stringify({ metrics: m }));
		}, duration * 1000);
	}
})();
