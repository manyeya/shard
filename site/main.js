// Shard's site: the sliced headline, a simulated download drawn the way Shard's own window draws
// one, and links to the latest release.
"use strict";

const still = matchMedia("(prefers-reduced-motion: reduce)").matches;
const $ = (id) => document.getElementById(id);
// Only touch the page when the text changes.
function set(el, text) {
  if (el && el.textContent !== text) el.textContent = text;
}

// The headline, cut into three slices per line like the logo's S.
for (const line of document.querySelectorAll(".sliced .line")) {
  const text = line.textContent.trim();
  line.textContent = "";
  for (let i = 0; i < 3; i++) {
    const band = document.createElement("span");
    band.className = "band";
    band.textContent = text;
    line.append(band);
  }
}
if (!still) {
  // The slices come in from alternating sides, out of order, and lock into place.
  const order = [3, 0, 5, 1, 4, 2];
  document.querySelectorAll(".sliced .band").forEach((band, i) => {
    const from = (i % 2 ? 1 : -1) * (80 + Math.random() * 80);
    band.animate([{ transform: `translateX(${from}px)`, opacity: 0 }, { transform: "none", opacity: 1 }], {
      duration: 1000,
      delay: 120 + order[i] * 110,
      easing: "cubic-bezier(0.16, 1, 0.3, 1)",
      fill: "backwards",
    });
  });
}

// A download the way Shard does one: the first connection takes the whole file; each one after
// it, and each one that finishes, takes the back half of whichever part has the most left, until
// the parts are too small to split.
class Demo {
  count() {
    return 8;
  }
  speedFor(i) {
    // One connection is slow, as there usually is one, so the others end up taking its work.
    return (i === 5 ? 2 + Math.random() * 2 : 8 + Math.random() * 11) * 1e6;
  }
  fresh() {
    const n = this.count();
    const size = 2.69e9;
    const whole = { s: 0, e: size, d: 0, c: 0, by: 0 };
    const conns = [];
    for (let i = 0; i < n; i++) conns.push({ part: i ? null : whole, opens: i * 0.35, v: 0, target: this.speedFor(i), re: 1 + Math.random() * 2 });
    const keep = this.m && this.m.n === n;
    return {
      n, size, min: size / 100, parts: [whole], conns, t: 0, done: null, smooth: 0, sample: 0, events: [],
      peak: keep ? this.m.peak : 0,
      hist: keep ? this.m.hist : new Array(96).fill(0),
      hist2: keep ? this.m.hist2 : Array.from({ length: 96 }, () => new Array(n).fill(0)),
    };
  }
  seed() {
    const m = this.fresh();
    for (let i = 0; i < 140; i++) this.step(m, 0.08);
    return m;
  }
  take(m, i) {
    let best = null;
    let most = 2 * m.min;
    for (const p of m.parts) {
      const left = p.e - p.s - p.d;
      if (p.c !== null && left >= most) { best = p; most = left; }
    }
    if (!best) return null;
    const mid = best.s + best.d + most / 2;
    const part = { s: mid, e: best.e, d: 0, c: i, by: i, born: m.t };
    best.e = mid;
    m.parts.push(part);
    m.events.push({ x: mid / m.size, t: m.t, i });
    if (m.events.length > 24) m.events.shift();
    return part;
  }
  step(m, dt) {
    m.t += dt;
    m.conns.forEach((c, i) => {
      if (c.opens !== null && m.t >= c.opens) { c.opens = null; if (!c.part) c.part = this.take(m, i); }
      if (!c.part) { c.v = 0; return; }
      c.re -= dt;
      if (c.re <= 0) { c.target = this.speedFor(i); c.re = 1 + Math.random() * 2; }
      c.v += (c.target - c.v) * Math.min(1, dt * 3);
      const p = c.part;
      p.d = Math.min(p.e - p.s, p.d + c.v * dt);
      if (p.d >= p.e - p.s) { p.c = null; c.part = this.take(m, i); }
    });
    const total = m.conns.reduce((a, c) => a + c.v, 0);
    m.smooth += (total - m.smooth) * Math.min(1, dt * 4);
    m.peak = Math.max(m.peak, m.smooth);
    m.sample -= dt;
    if (m.sample <= 0) {
      m.sample += 0.2;
      m.hist.push(m.smooth); m.hist.shift();
      m.hist2.push(m.conns.map((c) => c.v)); m.hist2.shift();
    }
    if (m.done === null && m.conns.every((c) => !c.part && c.opens === null)) m.done = m.t;
  }
  sim() {
    if (!this.m || this.m.n !== this.count()) { this.m = this.seed(); this.fx = null; }
    return this.m;
  }
  start() {
    this.clock = 9;
    this.sim();
    if (still) {
      const once = () => { this.draw(0); this.paint(true); };
      once();
      document.fonts.ready.then(once);
      addEventListener("resize", once);
      return;
    }
    this.last = performance.now();
    this.painted = 0;
    const frame = (now) => {
      requestAnimationFrame(frame);
      const dt = Math.min(0.05, Math.max(0, (now - this.last) / 1000));
      this.last = now;
      if (this.m.done !== null && this.m.t - this.m.done > 4) this.m = this.fresh();
      this.step(this.m, dt);
      this.clock += dt;
      this.draw(dt);
      const text = now - this.painted > 110;
      if (text) this.painted = now;
      this.paint(text);
    };
    requestAnimationFrame(frame);
  }

  // Drawing: each canvas is drawn only while it's on screen.
  prep(el) {
    const w = el.clientWidth;
    const h = el.clientHeight;
    if (!w || !h) return null;
    const r = el.getBoundingClientRect();
    if (r.bottom < -40 || r.top > (window.innerHeight || 4000) + 40) return null;
    const dpr = Math.min(2, window.devicePixelRatio || 1);
    const bw = Math.round(w * dpr);
    const bh = Math.round(h * dpr);
    if (el.width !== bw || el.height !== bh) { el.width = bw; el.height = bh; }
    const ctx = el.getContext("2d");
    ctx.setTransform(dpr, 0, 0, dpr, 0, 0);
    ctx.clearRect(0, 0, w, h);
    ctx.globalCompositeOperation = "source-over";
    ctx.shadowBlur = 0;
    return { ctx, w, h };
  }
  draw(dt) {
    if (!this.fx) this.fx = { flow: [], due: [], sparks: [], lim: [], limDue: [], nets: [], netDue: [0, 0] };
    const jobs = [
      ["au-gauge", this.drawGauge], ["au-flow", this.drawFlow], ["au-chart", this.drawChart], ["au-spectro", this.drawSpectro], ["au-map", this.drawMap],
      ["au-limit", this.drawLimit], ["au-resume", this.drawResume], ["au-nets", this.drawNets], ["au-dial", this.drawDial],
    ];
    for (const [id, fn] of jobs) {
      const el = $(id);
      if (!el) continue;
      const g = this.prep(el);
      if (g) fn.call(this, g, dt);
    }
  }
  // Dark olive to pale lime.
  mix(x) {
    const stops = [[0, [16, 18, 15]], [0.32, [54, 83, 20]], [0.68, [163, 230, 53]], [1, [240, 253, 214]]];
    const v = Math.max(0, Math.min(1, x));
    for (let k = 1; k < stops.length; k++) {
      if (v <= stops[k][0]) {
        const a = stops[k - 1];
        const b = stops[k];
        const u = (v - a[0]) / (b[0] - a[0]);
        return "rgb(" + a[1].map((c, j) => Math.round(c + (b[1][j] - c) * u)).join(",") + ")";
      }
    }
    return "rgb(240,253,214)";
  }
  curve(ctx, pts) {
    ctx.moveTo(pts[0][0], pts[0][1]);
    for (let i = 1; i < pts.length - 1; i++) {
      ctx.quadraticCurveTo(pts[i][0], pts[i][1], (pts[i][0] + pts[i + 1][0]) / 2, (pts[i][1] + pts[i + 1][1]) / 2);
    }
    const l = pts[pts.length - 1];
    ctx.lineTo(l[0], l[1]);
  }
  bez(a, b, c, d, u) {
    const v = 1 - u;
    return v * v * v * a + 3 * v * v * u * b + 3 * v * u * u * c + u * u * u * d;
  }
  round(ctx, x, y, w, h, r) {
    const rr = Math.max(0, Math.min(r, w / 2, h / 2));
    ctx.moveTo(x + rr, y);
    ctx.arcTo(x + w, y, x + w, y + h, rr);
    ctx.arcTo(x + w, y + h, x, y + h, rr);
    ctx.arcTo(x, y + h, x, y, rr);
    ctx.arcTo(x, y, x + w, y, rr);
    ctx.closePath();
  }

  drawGauge(g) {
    const { ctx, w, h } = g;
    const m = this.m;
    const cx = w / 2;
    const cy = h / 2 + 10;
    const R = Math.min(w, h) * 0.33;
    const a0 = Math.PI * 0.75;
    const sweep = Math.PI * 1.5;
    const top = 200e6;
    const f = Math.min(1, m.smooth / top);
    const glow = ctx.createRadialGradient(cx, cy, 0, cx, cy, R);
    glow.addColorStop(0, "rgba(163,230,53," + (0.05 + f * 0.12).toFixed(3) + ")");
    glow.addColorStop(1, "rgba(163,230,53,0)");
    ctx.fillStyle = glow;
    ctx.beginPath(); ctx.arc(cx, cy, R, 0, Math.PI * 2); ctx.fill();
    for (let k = 0; k <= 40; k++) {
      const a = a0 + (sweep * k) / 40;
      const major = k % 10 === 0;
      const r1 = R + 14;
      const r2 = R + (major ? 28 : k % 5 === 0 ? 23 : 19);
      ctx.strokeStyle = k / 40 <= f ? "rgba(190,242,100,0.95)" : "rgba(255,255,255,0.16)";
      ctx.lineWidth = major ? 2 : 1.2;
      ctx.beginPath(); ctx.moveTo(cx + Math.cos(a) * r1, cy + Math.sin(a) * r1); ctx.lineTo(cx + Math.cos(a) * r2, cy + Math.sin(a) * r2); ctx.stroke();
      if (major) {
        ctx.fillStyle = "#7c8378";
        ctx.font = '500 11px "Geist Mono", ui-monospace, monospace';
        ctx.textAlign = "center"; ctx.textBaseline = "middle";
        ctx.fillText(String((top / 1e6) * (k / 40)), cx + Math.cos(a) * (R + 42), cy + Math.sin(a) * (R + 42));
      }
    }
    ctx.lineCap = "round";
    ctx.lineWidth = 12;
    ctx.strokeStyle = "rgba(255,255,255,0.06)";
    ctx.beginPath(); ctx.arc(cx, cy, R, a0, a0 + sweep); ctx.stroke();
    const grad = ctx.createLinearGradient(cx - R, cy + R, cx + R, cy - R);
    grad.addColorStop(0, "#3f6212"); grad.addColorStop(0.55, "#a3e635"); grad.addColorStop(1, "#f7fee7");
    ctx.save();
    ctx.shadowColor = "rgba(163,230,53,0.85)"; ctx.shadowBlur = 22;
    ctx.strokeStyle = grad;
    ctx.beginPath(); ctx.arc(cx, cy, R, a0, a0 + sweep * Math.max(0.002, f)); ctx.stroke();
    ctx.restore();
    const ae = a0 + sweep * f;
    const ex = cx + Math.cos(ae) * R;
    const ey = cy + Math.sin(ae) * R;
    const tip = ctx.createRadialGradient(ex, ey, 0, ex, ey, 18);
    tip.addColorStop(0, "rgba(255,255,255,1)"); tip.addColorStop(0.3, "rgba(217,249,157,0.8)"); tip.addColorStop(1, "rgba(163,230,53,0)");
    ctx.fillStyle = tip; ctx.beginPath(); ctx.arc(ex, ey, 18, 0, Math.PI * 2); ctx.fill();
    const pa = a0 + sweep * Math.min(1, m.peak / top);
    ctx.fillStyle = "#d9f99d";
    ctx.beginPath();
    ctx.moveTo(cx + Math.cos(pa) * (R - 9), cy + Math.sin(pa) * (R - 9));
    ctx.lineTo(cx + Math.cos(pa - 0.035) * (R - 17), cy + Math.sin(pa - 0.035) * (R - 17));
    ctx.lineTo(cx + Math.cos(pa + 0.035) * (R - 17), cy + Math.sin(pa + 0.035) * (R - 17));
    ctx.fill();
    // One arc per connection, brighter the faster it is.
    const n = m.n;
    const gap = 0.045;
    const each = sweep / n;
    ctx.lineCap = "butt";
    ctx.lineWidth = 5;
    m.conns.forEach((c, i) => {
      const s = a0 + each * i + gap / 2;
      const q = Math.min(1, c.v / 19e6);
      ctx.save();
      if (c.part) { ctx.shadowColor = "rgba(163,230,53,0.7)"; ctx.shadowBlur = 10 * q; }
      ctx.strokeStyle = c.part ? "rgba(163,230,53," + (0.18 + 0.82 * q).toFixed(3) + ")" : "rgba(255,255,255,0.08)";
      ctx.beginPath(); ctx.arc(cx, cy, R - 26, s, s + each - gap); ctx.stroke();
      ctx.restore();
    });
  }

  lane(i, n, w, h) {
    const narrow = w < 480;
    const x0 = narrow ? 30 : 64;
    const x1 = w * 0.5;
    const x2 = w - (narrow ? 50 : 74);
    const y0 = h / 2;
    const yi = n === 1 ? y0 : 26 + (i * (h - 52)) / (n - 1);
    return { x0, x1, x2, y0, yi };
  }
  at(L, t) {
    if (t < 0.5) {
      const u = t * 2;
      const mx = (L.x0 + L.x1) / 2;
      return [this.bez(L.x0, mx, mx, L.x1, u), this.bez(L.y0, L.y0, L.yi, L.yi, u)];
    }
    const u = t * 2 - 1;
    const mx = (L.x1 + L.x2) / 2;
    return [this.bez(L.x1, mx, mx, L.x2, u), this.bez(L.yi, L.yi, L.y0, L.y0, u)];
  }
  drawFlow(g, dt) {
    const { ctx, w, h } = g;
    const m = this.m;
    const fx = this.fx;
    const n = m.n;
    const t = this.clock;
    // Lanes: wider and brighter the more the connection is carrying.
    ctx.globalCompositeOperation = "lighter";
    m.conns.forEach((c, i) => {
      const L = this.lane(i, n, w, h);
      const q = Math.min(1, c.v / 19e6);
      const mxa = (L.x0 + L.x1) / 2;
      const mxb = (L.x1 + L.x2) / 2;
      ctx.beginPath();
      ctx.moveTo(L.x0, L.y0);
      ctx.bezierCurveTo(mxa, L.y0, mxa, L.yi, L.x1, L.yi);
      ctx.bezierCurveTo(mxb, L.yi, mxb, L.y0, L.x2, L.y0);
      if (c.part) {
        const gr = ctx.createLinearGradient(L.x0, 0, L.x2, 0);
        gr.addColorStop(0, "rgba(163,230,53,0.05)");
        gr.addColorStop(0.5, "rgba(163,230,53," + (0.12 + q * 0.22).toFixed(3) + ")");
        gr.addColorStop(1, "rgba(217,249,157,0.1)");
        ctx.strokeStyle = gr;
        ctx.lineWidth = 2 + q * 9;
        ctx.stroke();
        ctx.strokeStyle = "rgba(217,249,157," + (0.15 + q * 0.25).toFixed(3) + ")";
        ctx.lineWidth = 1;
        ctx.stroke();
      } else {
        ctx.strokeStyle = "rgba(255,255,255,0.05)";
        ctx.lineWidth = 1;
        ctx.stroke();
      }
    });
    // Particles: one for every megabyte or so, moving as fast as their connection.
    m.conns.forEach((c, i) => {
      fx.due[i] = (fx.due[i] || 0) + (c.v / 1e6) * dt * 0.9;
      while (fx.due[i] >= 1 && fx.flow.length < 520) {
        fx.due[i] -= 1;
        fx.flow.push({ i, t: Math.random() * 0.02, sp: 0.28 + c.v / 45e6 });
      }
    });
    for (const p of fx.flow) p.t += p.sp * dt;
    fx.flow = fx.flow.filter((p) => p.t < 1);
    for (const p of fx.flow) {
      const L = this.lane(p.i, n, w, h);
      const a = this.at(L, p.t);
      const b = this.at(L, Math.max(0, p.t - 0.035));
      const gr = ctx.createLinearGradient(b[0], b[1], a[0], a[1]);
      gr.addColorStop(0, "rgba(217,249,157,0)");
      gr.addColorStop(1, "rgba(236,252,203,0.95)");
      ctx.strokeStyle = gr;
      ctx.lineWidth = 2;
      ctx.lineCap = "round";
      ctx.beginPath(); ctx.moveTo(b[0], b[1]); ctx.lineTo(a[0], a[1]); ctx.stroke();
    }
    ctx.globalCompositeOperation = "source-over";
    // A burst where a connection has just cut a part in two.
    for (const e of m.events) {
      const age = m.t - e.t;
      if (age < 0 || age > 0.9) continue;
      const L = this.lane(e.i, n, w, h);
      ctx.strokeStyle = "rgba(236,252,203," + (1 - age / 0.9).toFixed(3) + ")";
      ctx.lineWidth = 1.5;
      ctx.beginPath(); ctx.arc(L.x1, L.yi, 6 + age * 34, 0, Math.PI * 2); ctx.stroke();
    }
    // Connection nodes with their speed.
    ctx.font = '500 10px "Geist Mono", ui-monospace, monospace';
    ctx.textBaseline = "middle";
    m.conns.forEach((c, i) => {
      const L = this.lane(i, n, w, h);
      ctx.save();
      if (c.part) { ctx.shadowColor = "rgba(163,230,53,0.9)"; ctx.shadowBlur = 14; }
      ctx.fillStyle = c.part ? "#a3e635" : "#262a23";
      ctx.beginPath(); ctx.arc(L.x1, L.yi, 4.5, 0, Math.PI * 2); ctx.fill();
      ctx.restore();
      ctx.strokeStyle = c.part ? "rgba(217,249,157,0.6)" : "rgba(255,255,255,0.15)";
      ctx.lineWidth = 1;
      ctx.beginPath(); ctx.arc(L.x1, L.yi, 8, 0, Math.PI * 2); ctx.stroke();
      if (n <= 12 && w >= 480) {
        const text = c.part ? (c.v / 1e6).toFixed(1) : c.opens !== null ? "opening" : "done";
        const tw = ctx.measureText(text).width + 12;
        ctx.fillStyle = "rgba(10,11,9,0.92)";
        ctx.beginPath(); this.round(ctx, L.x1 + 13, L.yi - 8, tw, 16, 8); ctx.fill();
        ctx.strokeStyle = c.part ? "rgba(163,230,53,0.3)" : "rgba(255,255,255,0.08)";
        ctx.beginPath(); this.round(ctx, L.x1 + 13.5, L.yi - 7.5, tw - 1, 15, 7.5); ctx.stroke();
        ctx.fillStyle = c.part ? "#d9f99d" : "#4a5046";
        ctx.textAlign = "left";
        ctx.fillText(text, L.x1 + 19, L.yi + 0.5);
      }
    });
    // The server, pulsing.
    const L0 = this.lane(0, n, w, h);
    for (let k = 0; k < 3; k++) {
      const ph = (t * 0.55 + k / 3) % 1;
      ctx.strokeStyle = "rgba(163,230,53," + ((1 - ph) * 0.4).toFixed(3) + ")";
      ctx.lineWidth = 1;
      ctx.beginPath(); ctx.arc(L0.x0, L0.y0, 12 + ph * 34, 0, Math.PI * 2); ctx.stroke();
    }
    const orb = ctx.createRadialGradient(L0.x0, L0.y0, 0, L0.x0, L0.y0, 26);
    orb.addColorStop(0, "rgba(247,254,231,1)"); orb.addColorStop(0.25, "rgba(190,242,100,0.9)"); orb.addColorStop(1, "rgba(163,230,53,0)");
    ctx.fillStyle = orb; ctx.beginPath(); ctx.arc(L0.x0, L0.y0, 26, 0, Math.PI * 2); ctx.fill();
    ctx.fillStyle = "#7c8378"; ctx.textAlign = "center";
    if (w >= 480) ctx.fillText("releases.ubuntu.com", L0.x0, L0.y0 + 44);
    // The file, filling.
    const got = m.parts.reduce((a, p) => a + p.d, 0) / m.size;
    const fw = w < 480 ? 38 : 50;
    const fh = w < 480 ? 58 : 72;
    const fxp = L0.x2;
    const fyp = L0.y0 - fh / 2;
    ctx.save();
    ctx.shadowColor = "rgba(163,230,53,0.45)"; ctx.shadowBlur = 24;
    ctx.fillStyle = "#10130e";
    ctx.beginPath(); this.round(ctx, fxp, fyp, fw, fh, 10); ctx.fill();
    ctx.restore();
    ctx.save();
    ctx.beginPath(); this.round(ctx, fxp, fyp, fw, fh, 10); ctx.clip();
    const fg = ctx.createLinearGradient(0, fyp + fh, 0, fyp);
    fg.addColorStop(0, "#4d7c0f"); fg.addColorStop(1, "#bef264");
    ctx.fillStyle = fg;
    const fill = fh * got;
    ctx.fillRect(fxp, fyp + fh - fill, fw, fill);
    ctx.fillStyle = "rgba(255,255,255,0.7)";
    ctx.fillRect(fxp, fyp + fh - fill, fw, 1.5);
    ctx.restore();
    ctx.strokeStyle = "rgba(217,249,157,0.65)";
    ctx.lineWidth = 1;
    ctx.beginPath(); this.round(ctx, fxp + 0.5, fyp + 0.5, fw - 1, fh - 1, 10); ctx.stroke();
    ctx.fillStyle = "#d9f99d";
    ctx.font = '600 12px "Geist Mono", ui-monospace, monospace';
    ctx.fillText(Math.floor(got * 100) + "%", fxp + fw / 2, fyp + fh + 18);
    ctx.fillStyle = "#7c8378";
    ctx.font = '500 10px "Geist Mono", ui-monospace, monospace';
    ctx.fillText("ubuntu.iso", fxp + fw / 2, fyp - 14);
  }

  drawChart(g) {
    const { ctx, w, h } = g;
    const m = this.m;
    const H = m.hist2;
    const n = m.n;
    const padL = 6;
    const padR = 64;
    const padT = 14;
    const padB = 24;
    const pw = w - padL - padR;
    const ph = h - padT - padB;
    const steps = [50e6, 100e6, 150e6, 200e6, 250e6, 300e6, 400e6, 500e6];
    const want = Math.max(m.peak, ...m.hist) * 1.12;
    const top = steps.find((s) => s >= want) || want;
    ctx.font = '500 10px "Geist Mono", ui-monospace, monospace';
    ctx.textBaseline = "middle";
    ctx.textAlign = "left";
    const div = (top / 1e6) % 4 === 0 ? 4 : 3;
    for (let k = 0; k <= div; k++) {
      const y = padT + ph - (ph * k) / div;
      ctx.strokeStyle = k === 0 ? "rgba(255,255,255,0.14)" : "rgba(255,255,255,0.05)";
      ctx.lineWidth = 1;
      ctx.setLineDash(k === 0 ? [] : [2, 4]);
      ctx.beginPath(); ctx.moveTo(padL, y); ctx.lineTo(padL + pw, y); ctx.stroke();
      ctx.setLineDash([]);
      ctx.fillStyle = "#5d6359";
      ctx.fillText(Math.round((top / 1e6) * (k / div)) + (k === div ? " MB/s" : ""), padL + pw + 10, y);
    }
    const len = H.length;
    const phase = 1 - Math.max(0, Math.min(1, m.sample / 0.2));
    const X = (i) => padL + ((i - phase) / (len - 2)) * pw;
    const Y = (v) => padT + ph - (Math.min(v, top) / top) * ph;
    ctx.save();
    ctx.beginPath(); ctx.rect(padL, 0, pw, h); ctx.clip();
    // Stacked: each connection's share, the first at the bottom.
    const cum = H.map((row) => {
      const out = [];
      let s = 0;
      for (let k = 0; k < n; k++) { s += row[k] || 0; out.push(s); }
      return out;
    });
    for (let k = n - 1; k >= 0; k--) {
      const pts = cum.map((row, i) => [X(i), Y(row[k])]);
      ctx.beginPath();
      this.curve(ctx, pts);
      ctx.lineTo(X(len - 1), padT + ph);
      ctx.lineTo(X(0), padT + ph);
      ctx.closePath();
      const shade = n === 1 ? 0.7 : 0.36 + (0.5 * k) / (n - 1);
      const gr = ctx.createLinearGradient(0, padT, 0, padT + ph);
      gr.addColorStop(0, this.mix(shade + 0.08));
      gr.addColorStop(1, this.mix(shade - 0.16));
      ctx.fillStyle = gr;
      ctx.globalAlpha = 0.92;
      ctx.fill();
      ctx.globalAlpha = 1;
      ctx.strokeStyle = "rgba(8,9,7,0.55)";
      ctx.lineWidth = 1;
      ctx.beginPath(); this.curve(ctx, pts); ctx.stroke();
    }
    const tops = cum.map((row, i) => [X(i), Y(row[n - 1])]);
    ctx.save();
    ctx.shadowColor = "rgba(190,242,100,0.9)"; ctx.shadowBlur = 14;
    ctx.strokeStyle = "#ecfccb"; ctx.lineWidth = 2;
    ctx.beginPath(); this.curve(ctx, tops); ctx.stroke();
    ctx.restore();
    const fade = ctx.createLinearGradient(padL, 0, padL + 90, 0);
    fade.addColorStop(0, "rgba(13,15,12,1)"); fade.addColorStop(1, "rgba(13,15,12,0)");
    ctx.fillStyle = fade; ctx.fillRect(padL, 0, 90, h);
    ctx.restore();
    // Peak, dashed.
    const py = Y(m.peak);
    ctx.strokeStyle = "rgba(217,249,157,0.45)"; ctx.setLineDash([4, 4]);
    ctx.beginPath(); ctx.moveTo(padL, py); ctx.lineTo(padL + pw, py); ctx.stroke();
    ctx.setLineDash([]);
    ctx.fillStyle = "#a7ada3"; ctx.textAlign = "left";
    ctx.fillText("peak " + (m.peak / 1e6).toFixed(1), padL + 8, py - 9);
    // Now: a scanner at the right edge with the current total.
    const nowX = padL + pw;
    ctx.strokeStyle = "rgba(236,252,203,0.35)"; ctx.lineWidth = 1;
    ctx.beginPath(); ctx.moveTo(nowX, padT); ctx.lineTo(nowX, padT + ph); ctx.stroke();
    const dotY = Y(m.smooth);
    const halo = ctx.createRadialGradient(nowX, dotY, 0, nowX, dotY, 16);
    halo.addColorStop(0, "rgba(255,255,255,1)"); halo.addColorStop(0.35, "rgba(190,242,100,0.75)"); halo.addColorStop(1, "rgba(163,230,53,0)");
    ctx.fillStyle = halo; ctx.beginPath(); ctx.arc(nowX, dotY, 16, 0, Math.PI * 2); ctx.fill();
    const label = (m.smooth / 1e6).toFixed(1) + " MB/s";
    ctx.font = '600 11px "Geist Mono", ui-monospace, monospace';
    const tw = ctx.measureText(label).width + 16;
    const lx = nowX - tw - 14;
    const ly = Math.max(padT + 2, Math.min(padT + ph - 22, dotY - 11));
    ctx.fillStyle = "#a3e635";
    ctx.beginPath(); this.round(ctx, lx, ly, tw, 22, 11); ctx.fill();
    ctx.fillStyle = "#0a0c06"; ctx.textAlign = "center";
    ctx.fillText(label, lx + tw / 2, ly + 11.5);
    ctx.font = '500 10px "Geist Mono", ui-monospace, monospace';
    ctx.fillStyle = "#5d6359";
    ctx.textAlign = "left"; ctx.fillText("-19 s", padL, h - 8);
    ctx.textAlign = "center"; ctx.fillText("-10 s", padL + pw / 2, h - 8);
    ctx.textAlign = "right"; ctx.fillText("now", padL + pw, h - 8);
  }

  drawSpectro(g) {
    const { ctx, w, h } = g;
    const m = this.m;
    const H = m.hist2;
    const n = m.n;
    const padL = 30;
    const padR = 58;
    const rowH = (h - 6) / n;
    const pw = w - padL - padR;
    const len = H.length;
    const phase = 1 - Math.max(0, Math.min(1, m.sample / 0.2));
    const cw = pw / (len - 2);
    ctx.save();
    ctx.beginPath(); ctx.rect(padL, 0, pw, h); ctx.clip();
    for (let i = 0; i < len; i++) {
      const x = padL + (i - phase) * cw;
      for (let k = 0; k < n; k++) {
        const v = H[i][k] || 0;
        const prev = i > 0 ? H[i - 1][k] || 0 : v;
        ctx.fillStyle = this.mix((v + prev) / 2 / 23e6);
        ctx.fillRect(x, 3 + k * rowH, cw + 0.6, rowH - 3);
      }
    }
    const fade = ctx.createLinearGradient(padL, 0, padL + 60, 0);
    fade.addColorStop(0, "rgba(13,15,12,1)"); fade.addColorStop(1, "rgba(13,15,12,0)");
    ctx.fillStyle = fade; ctx.fillRect(padL, 0, 60, h);
    ctx.restore();
    ctx.font = '500 10px "Geist Mono", ui-monospace, monospace';
    ctx.textBaseline = "middle";
    m.conns.forEach((c, k) => {
      const y = 3 + k * rowH + rowH / 2 - 1;
      ctx.fillStyle = c.part ? "#a7ada3" : "#4a5046";
      ctx.textAlign = "left";
      ctx.fillText("#" + (k + 1), 0, y);
      ctx.fillStyle = c.part ? "#d9f99d" : "#4a5046";
      ctx.textAlign = "right";
      ctx.fillText(c.part ? (c.v / 1e6).toFixed(1) : "·", w, y);
    });
  }

  drawMap(g, dt) {
    const { ctx, w, h } = g;
    const m = this.m;
    const fx = this.fx;
    const y = 6;
    const bh = h - 22;
    ctx.fillStyle = "rgba(255,255,255,0.025)";
    ctx.beginPath(); this.round(ctx, 0, y - 3, w, bh + 6, 10); ctx.fill();
    for (const p of m.parts) {
      const x = (p.s / m.size) * w + 1;
      const pw = Math.max(1, ((p.e - p.s) / m.size) * w - 2);
      ctx.fillStyle = "#161914";
      ctx.beginPath(); this.round(ctx, x, y, pw, bh, 5); ctx.fill();
      const fw = pw * (p.d / (p.e - p.s));
      if (fw > 0.5) {
        ctx.save();
        ctx.beginPath(); this.round(ctx, x, y, pw, bh, 5); ctx.clip();
        const gr = ctx.createLinearGradient(x, 0, x + Math.max(fw, 40), 0);
        gr.addColorStop(0, "#3f6212"); gr.addColorStop(0.7, "#a3e635"); gr.addColorStop(1, "#d9f99d");
        ctx.fillStyle = gr;
        ctx.fillRect(x, y, fw, bh);
        const sheen = ctx.createLinearGradient(0, y, 0, y + bh);
        sheen.addColorStop(0, "rgba(255,255,255,0.22)"); sheen.addColorStop(0.45, "rgba(255,255,255,0)"); sheen.addColorStop(1, "rgba(0,0,0,0.18)");
        ctx.fillStyle = sheen;
        ctx.fillRect(x, y, fw, bh);
        ctx.restore();
      }
      if (p.c !== null) {
        const hx = x + fw;
        ctx.save();
        ctx.shadowColor = "rgba(236,252,203,1)"; ctx.shadowBlur = 16;
        ctx.fillStyle = "#ffffff";
        ctx.fillRect(hx - 1, y - 2, 2, bh + 4);
        ctx.restore();
        if (dt && Math.random() < 0.5) fx.sparks.push({ x: hx, y: y + Math.random() * bh, vx: -20 - Math.random() * 40, vy: (Math.random() - 0.5) * 30, life: 0.5 + Math.random() * 0.4, age: 0 });
      }
      // Which connection fetched it, as a thin strip underneath.
      ctx.fillStyle = this.mix(0.4 + (0.5 * (p.by % m.n)) / Math.max(1, m.n - 1));
      ctx.fillRect(x, y + bh + 6, pw, 3);
    }
    ctx.globalCompositeOperation = "lighter";
    for (const s of fx.sparks) {
      s.age += dt; s.x += s.vx * dt; s.y += s.vy * dt;
      const a = Math.max(0, 1 - s.age / s.life);
      ctx.fillStyle = "rgba(217,249,157," + (a * 0.8).toFixed(3) + ")";
      ctx.fillRect(s.x, s.y, 1.6, 1.6);
    }
    fx.sparks = fx.sparks.filter((s) => s.age < s.life).slice(-300);
    // A flash where a part was just cut in two.
    for (const e of m.events) {
      const age = m.t - e.t;
      if (age < 0 || age > 0.9) continue;
      const x = e.x * w;
      const a = 1 - age / 0.9;
      const gr = ctx.createRadialGradient(x, y + bh / 2, 0, x, y + bh / 2, 30 + age * 40);
      gr.addColorStop(0, "rgba(236,252,203," + (a * 0.9).toFixed(3) + ")"); gr.addColorStop(1, "rgba(163,230,53,0)");
      ctx.fillStyle = gr;
      ctx.fillRect(x - 80, y - 10, 160, bh + 20);
      ctx.fillStyle = "rgba(255,255,255," + a.toFixed(3) + ")";
      ctx.fillRect(x - 0.75, y - 6, 1.5, bh + 12);
    }
    ctx.globalCompositeOperation = "source-over";
  }

  limitState(t) {
    const steps = [[0, 8], [2, 3], [5.5, 4], [8.5, 5], [9.6, 4]];
    const tt = t % 13;
    let open = 8;
    for (const s of steps) if (tt >= s[0]) open = s[1];
    return { tt, open };
  }
  drawLimit(g, dt) {
    const { ctx, w, h } = g;
    const fx = this.fx;
    const { tt, open } = this.limitState(this.clock);
    const rows = 8;
    const x0 = 40;
    const x1 = w - 16;
    const rowH = (h - 16) / rows;
    // When each lane last changed, for the flashes.
    const changes = [[2, [3, 4, 5, 6, 7], "out"], [5.5, [3], "in"], [8.5, [4], "in"], [9.6, [4], "out"]];
    ctx.font = '500 10px "Geist Mono", ui-monospace, monospace';
    ctx.textBaseline = "middle";
    for (let i = 0; i < rows; i++) {
      const y = 8 + rowH * i + rowH / 2;
      const on = i < open;
      let flash = null;
      for (const [at, lanes, kind] of changes) if (lanes.includes(i) && tt >= at && tt - at < 0.9) flash = { kind, age: tt - at };
      ctx.fillStyle = on ? "#a7ada3" : "#5d4046";
      ctx.textAlign = "left";
      ctx.fillText("#" + (i + 1), 0, y);
      if (on) {
        ctx.strokeStyle = "rgba(163,230,53,0.22)"; ctx.lineWidth = 6; ctx.lineCap = "round";
        ctx.beginPath(); ctx.moveTo(x0, y); ctx.lineTo(x1, y); ctx.stroke();
        fx.limDue[i] = (fx.limDue[i] || 0) + dt * (2.5 + (i % 3));
        while (fx.limDue[i] >= 1) { fx.limDue[i] -= 1; fx.lim.push({ i, x: 0, sp: 0.45 + Math.random() * 0.25 }); }
      } else {
        ctx.strokeStyle = "rgba(251,113,133,0.28)"; ctx.lineWidth = 1; ctx.setLineDash([3, 5]);
        ctx.beginPath(); ctx.moveTo(x0 + 44, y); ctx.lineTo(x1, y); ctx.stroke();
        ctx.setLineDash([]);
        ctx.fillStyle = "rgba(251,113,133,0.14)";
        ctx.beginPath(); this.round(ctx, x0, y - 9, 36, 18, 9); ctx.fill();
        ctx.fillStyle = "#fb7185"; ctx.textAlign = "center";
        ctx.fillText("429", x0 + 18, y + 0.5);
      }
      if (flash) {
        const a = 1 - flash.age / 0.9;
        const col = flash.kind === "out" ? "251,113,133" : "217,249,157";
        const gr = ctx.createLinearGradient(x0, 0, x1, 0);
        const head = flash.kind === "in" ? Math.min(1, flash.age / 0.6) : 1;
        gr.addColorStop(0, "rgba(" + col + "," + (a * 0.7).toFixed(3) + ")");
        gr.addColorStop(Math.max(0.01, head), "rgba(" + col + "," + (a * 0.5).toFixed(3) + ")");
        gr.addColorStop(Math.min(1, head + 0.01), "rgba(" + col + ",0)");
        ctx.strokeStyle = gr; ctx.lineWidth = 8; ctx.lineCap = "round";
        ctx.beginPath(); ctx.moveTo(x0, y); ctx.lineTo(x1, y); ctx.stroke();
      }
    }
    ctx.globalCompositeOperation = "lighter";
    for (const p of fx.lim) {
      p.x += p.sp * dt;
      const y = 8 + rowH * p.i + rowH / 2;
      const px = x0 + p.x * (x1 - x0);
      const gr = ctx.createLinearGradient(px - 28, 0, px, 0);
      gr.addColorStop(0, "rgba(217,249,157,0)"); gr.addColorStop(1, "rgba(236,252,203,0.95)");
      ctx.strokeStyle = gr; ctx.lineWidth = 2.5; ctx.lineCap = "round";
      ctx.beginPath(); ctx.moveTo(px - 28, y); ctx.lineTo(px, y); ctx.stroke();
    }
    ctx.globalCompositeOperation = "source-over";
    fx.lim = fx.lim.filter((p) => p.x < 1 && p.i < open).slice(-200);
  }

  drawResume(g) {
    const { ctx, w, h } = g;
    const tt = this.clock % 9.5;
    let f = 1;
    let stopped = false;
    if (tt < 3.5) f = (tt / 3.5) * 0.6;
    else if (tt < 5) { f = 0.6; stopped = true; }
    else if (tt < 8) f = 0.6 + ((tt - 5) / 3) * 0.4;
    const cx = w / 2;
    const cy = h / 2;
    const R = Math.min(w, h) * 0.36;
    const a0 = -Math.PI / 2;
    ctx.lineCap = "round";
    ctx.lineWidth = 10;
    ctx.strokeStyle = "rgba(255,255,255,0.06)";
    ctx.beginPath(); ctx.arc(cx, cy, R, 0, Math.PI * 2); ctx.stroke();
    const flicker = stopped && !still ? 0.25 + Math.random() * 0.25 : stopped ? 0.4 : 1;
    ctx.save();
    if (!stopped) { ctx.shadowColor = "rgba(163,230,53,0.85)"; ctx.shadowBlur = 18; }
    const gr = ctx.createLinearGradient(cx - R, cy + R, cx + R, cy - R);
    gr.addColorStop(0, stopped ? "#4a5046" : "#4d7c0f"); gr.addColorStop(1, stopped ? "#8a9086" : "#ecfccb");
    ctx.strokeStyle = gr;
    ctx.globalAlpha = flicker;
    ctx.beginPath(); ctx.arc(cx, cy, R, a0, a0 + Math.PI * 2 * Math.max(0.005, f)); ctx.stroke();
    ctx.restore();
    if (tt >= 3.5) {
      const am = a0 + Math.PI * 2 * 0.6;
      ctx.strokeStyle = "#ffffff"; ctx.lineWidth = 2; ctx.lineCap = "butt";
      ctx.beginPath(); ctx.moveTo(cx + Math.cos(am) * (R - 12), cy + Math.sin(am) * (R - 12)); ctx.lineTo(cx + Math.cos(am) * (R + 12), cy + Math.sin(am) * (R + 12)); ctx.stroke();
    }
    ctx.fillStyle = stopped ? "#8a9086" : "#f1f3ee";
    ctx.font = '500 30px "Geist Mono", ui-monospace, monospace';
    ctx.textAlign = "center"; ctx.textBaseline = "middle";
    ctx.fillText(Math.floor(f * 100) + "%", cx, cy - 4);
    ctx.font = '500 10px "Geist Mono", ui-monospace, monospace';
    ctx.fillStyle = stopped ? "#fb7185" : "#7c8378";
    ctx.fillText(stopped ? "stopped" : f >= 1 ? "done" : tt >= 5 ? "resumed" : "downloading", cx, cy + 22);
  }

  drawNets(g, dt) {
    const { ctx, w, h } = g;
    const fx = this.fx;
    const t = this.clock;
    const a = 31.2 + 3 * Math.sin(t * 0.8) + Math.sin(t * 2.3);
    const b = 14.6 + 2 * Math.sin(t * 1.1 + 1) + 0.7 * Math.sin(t * 3.7);
    const src = [{ y: h * 0.26, v: a, col: "163,230,53", name: "Wi-Fi" }, { y: h * 0.74, v: b, col: "236,240,232", name: "iPhone USB" }];
    const x0 = 110;
    const x2 = w - 70;
    const y2 = h / 2;
    ctx.globalCompositeOperation = "lighter";
    src.forEach((s, k) => {
      const mx = (x0 + x2) / 2;
      ctx.strokeStyle = "rgba(" + s.col + ",0.22)";
      ctx.lineWidth = s.v * 0.55;
      ctx.lineCap = "butt";
      ctx.beginPath(); ctx.moveTo(x0, s.y); ctx.bezierCurveTo(mx, s.y, mx, y2 + (k ? 6 : -6), x2, y2 + (k ? 6 : -6)); ctx.stroke();
      fx.netDue[k] += dt * s.v * 0.35;
      while (fx.netDue[k] >= 1) { fx.netDue[k] -= 1; fx.nets.push({ k, u: 0, off: (Math.random() - 0.5) * s.v * 0.45, sp: 0.42 + Math.random() * 0.12 }); }
    });
    for (const p of fx.nets) {
      p.u += p.sp * dt;
      const s = src[p.k];
      const mx = (x0 + x2) / 2;
      const ty = y2 + (p.k ? 6 : -6);
      const px = this.bez(x0, mx, mx, x2, p.u);
      const py = this.bez(s.y, s.y, ty, ty, p.u) + p.off * (1 - p.u);
      ctx.fillStyle = "rgba(" + s.col + ",0.9)";
      ctx.fillRect(px - 1.5, py - 1, 3, 2);
    }
    fx.nets = fx.nets.filter((p) => p.u < 1).slice(-400);
    ctx.globalCompositeOperation = "source-over";
    ctx.font = '500 11px "Geist Mono", ui-monospace, monospace';
    ctx.textBaseline = "middle";
    src.forEach((s) => {
      ctx.fillStyle = "rgba(" + s.col + ",1)";
      ctx.beginPath(); ctx.arc(x0, s.y, 5, 0, Math.PI * 2); ctx.fill();
      ctx.textAlign = "right";
      ctx.fillStyle = "#f1f3ee"; ctx.fillText(s.name, x0 - 14, s.y - 8);
      ctx.fillStyle = "#7c8378"; ctx.fillText(s.v.toFixed(1) + " MB/s", x0 - 14, s.y + 8);
    });
    ctx.save();
    ctx.shadowColor = "rgba(163,230,53,0.6)"; ctx.shadowBlur = 20;
    ctx.fillStyle = "#10130e";
    ctx.beginPath(); this.round(ctx, x2, y2 - 34, 44, 68, 9); ctx.fill();
    ctx.restore();
    ctx.strokeStyle = "rgba(217,249,157,0.6)";
    ctx.beginPath(); this.round(ctx, x2 + 0.5, y2 - 33.5, 43, 67, 9); ctx.stroke();
    ctx.fillStyle = "#d9f99d"; ctx.textAlign = "center";
    ctx.font = '600 12px "Geist Mono", ui-monospace, monospace';
    ctx.fillText((a + b).toFixed(1), x2 + 22, y2 - 6);
    ctx.font = '500 9px "Geist Mono", ui-monospace, monospace';
    ctx.fillStyle = "#7c8378";
    ctx.fillText("MB/s", x2 + 22, y2 + 10);
  }

  drawDial(g) {
    const { ctx, w, h } = g;
    const hour = (this.clock * 1.5) % 24;
    const cx = w / 2;
    const cy = h / 2;
    const R = Math.min(w, h) * 0.4;
    const ang = (x) => -Math.PI / 2 + (x / 24) * Math.PI * 2;
    for (let k = 0; k < 24; k++) {
      const a = ang(k);
      const major = k % 6 === 0;
      ctx.strokeStyle = major ? "rgba(255,255,255,0.4)" : "rgba(255,255,255,0.14)";
      ctx.lineWidth = major ? 2 : 1;
      ctx.beginPath(); ctx.moveTo(cx + Math.cos(a) * (R + 6), cy + Math.sin(a) * (R + 6)); ctx.lineTo(cx + Math.cos(a) * (R + (major ? 14 : 10)), cy + Math.sin(a) * (R + (major ? 14 : 10))); ctx.stroke();
    }
    ctx.lineWidth = 8; ctx.lineCap = "round";
    ctx.strokeStyle = "rgba(255,255,255,0.06)";
    ctx.beginPath(); ctx.arc(cx, cy, R, 0, Math.PI * 2); ctx.stroke();
    // The window the queue runs in: 01:00 to 07:00.
    const inside = hour >= 1 && hour < 7;
    ctx.save();
    ctx.shadowColor = "rgba(163,230,53," + (inside ? 0.95 : 0.4) + ")"; ctx.shadowBlur = inside ? 20 : 8;
    ctx.strokeStyle = inside ? "#bef264" : "rgba(163,230,53,0.55)";
    ctx.beginPath(); ctx.arc(cx, cy, R, ang(1), ang(7)); ctx.stroke();
    ctx.restore();
    if (ctx.createConicGradient) {
      const trail = ctx.createConicGradient(ang(hour) - 1.2, cx, cy);
      trail.addColorStop(0, "rgba(236,252,203,0)");
      trail.addColorStop(0.19, "rgba(236,252,203,0.18)");
      trail.addColorStop(0.191, "rgba(236,252,203,0)");
      ctx.fillStyle = trail;
      ctx.beginPath(); ctx.moveTo(cx, cy); ctx.arc(cx, cy, R - 8, ang(hour) - 1.2, ang(hour)); ctx.closePath(); ctx.fill();
    }
    const a = ang(hour);
    ctx.strokeStyle = "#f1f3ee"; ctx.lineWidth = 2; ctx.lineCap = "round";
    ctx.beginPath(); ctx.moveTo(cx, cy); ctx.lineTo(cx + Math.cos(a) * (R - 10), cy + Math.sin(a) * (R - 10)); ctx.stroke();
    ctx.fillStyle = "#f1f3ee"; ctx.beginPath(); ctx.arc(cx, cy, 4, 0, Math.PI * 2); ctx.fill();
    ctx.font = '500 9px "Geist Mono", ui-monospace, monospace';
    ctx.fillStyle = "#5d6359"; ctx.textAlign = "center"; ctx.textBaseline = "middle";
    ctx.fillText("00", cx, cy - R + 16); ctx.fillText("12", cx, cy + R - 16);
    ctx.fillText("06", cx + R - 16, cy); ctx.fillText("18", cx - R + 16, cy);
  }

  fmt(n) {
    if (n >= 1e9) return (n / 1e9).toFixed(2) + " GB";
    if (n >= 1e6) return (n / 1e6).toFixed(n >= 1e8 ? 0 : 1) + " MB";
    return Math.round(n / 1e3) + " KB";
  }
  view() {
    const m = this.m;
    const got = m.parts.reduce((a, p) => a + p.d, 0);
    const live = m.conns.filter((c) => c.part).length;
    const pct = Math.min(100, (got / m.size) * 100);
    const left = m.size - got;
    return {
      n: String(m.n),
      live: String(live),
      speed: (m.smooth / 1e6).toFixed(1),
      peak: (m.peak / 1e6).toFixed(1),
      partCount: String(m.parts.length),
      file: this.fmt(got) + " of " + this.fmt(m.size),
      pct: Math.floor(pct) + "%",
      pctCss: pct.toFixed(1) + "%",
      eta: m.done !== null ? "Done" : m.smooth > 1e6 ? Math.ceil(left / m.smooth) + " s" : "Starting",
      state: m.done !== null ? "Downloaded" : "Downloading",
      caption: m.done !== null
        ? "Done: " + this.fmt(m.size) + " in " + Math.round(m.done) + " s, in " + m.parts.length + " parts over " + m.n + " connections."
        : "Each flash is a connection cutting the slowest part in two and taking the back half.",
    };
  }
  limitNote(t) {
    const notes = [
      [0, "Starting with 8 connections"],
      [2, "The server refused 5 with “too many requests”. Carrying on with 3."],
      [5.5, "Half a minute later it tries one more. Allowed: 4 connections."],
      [8.5, "Later it tries a fifth…"],
      [9.6, "…refused, so it waits twice as long before the next try."],
    ];
    const tt = t % 13;
    let out = notes[0][1];
    for (const s of notes) if (tt >= s[0]) out = s[1];
    return out;
  }
  resumeNote(t) {
    const tt = t % 9.5;
    if (tt < 3.5) return "Downloading";
    if (tt < 5) return "Shard quit at 60%. Nothing is lost.";
    if (tt < 8) return "Opened again: it carries on from 60%";
    return "Done";
  }
  handDemo(t) {
    const tt = t % 6.5;
    const u = Math.max(0, Math.min(1, (tt - 1.2) / 0.8));
    const e = u * u * (3 - 2 * u);
    const pct = Math.round(Math.max(0, Math.min(1, (tt - 2) / 3.2)) * 100);
    return {
      chip: "translate(" + (e * 40).toFixed(1) + "px, " + (e * 72 - Math.sin(e * Math.PI) * 26).toFixed(1) + "px) scale(" + (1 - e * 0.25).toFixed(3) + ")",
      chipOp: (1 - e).toFixed(2), cardOp: (0.3 + 0.7 * e).toFixed(2), glow: (e * (1 - pct / 140)).toFixed(2),
      pct: pct === 100 ? "Done" : pct + "%", bar: pct + "%",
    };
  }
  scheduleDemo(t) {
    const hour = (t * 1.5) % 24;
    const h = Math.floor(hour);
    const mins = Math.floor((hour - h) * 60);
    const inside = hour >= 1 && hour < 7;
    return { time: String(h).padStart(2, "0") + ":" + String(mins).padStart(2, "0"), state: inside ? "Downloading. Stops at 07:00, then the computer sleeps." : "Queued. Starts at 01:00." };
  }
  refreshDemo(t) {
    const tt = t % 8;
    const fresh = "https://cdn.example.com/build.zip?token=8f3a…";
    if (tt < 2) return { url: "Link expired: the server says 403", bar: "53%", color: "#fb7185", note: "1.42 GB of 2.69 GB kept", urlColor: "#fb7185" };
    if (tt < 3.6) return { url: fresh.slice(0, Math.ceil(((tt - 2) / 1.6) * fresh.length)), bar: "53%", color: "#5d6359", note: "Pasting a fresh link", urlColor: "#d9f99d" };
    if (tt < 4.4) return { url: fresh, bar: "53%", color: "#a3e635", note: "Same size: keeping 1.42 GB", urlColor: "#d9f99d" };
    const p = Math.min(100, 53 + ((tt - 4.4) / 3) * 47);
    return { url: fresh, bar: p.toFixed(0) + "%", color: "#a3e635", note: p >= 100 ? "Done" : "Carrying on: " + p.toFixed(0) + "%", urlColor: "#d9f99d" };
  }
  updateDemo(t) {
    const tt = t % 10;
    const at = tt < 1.5 ? 0 : tt < 2.6 ? 1 : tt < 6 ? 2 : tt < 7.4 ? 3 : 4;
    const got = Math.max(0, Math.min(1, (tt - 2.6) / 3.4));
    const texts = ["Reading the signed list of releases", "Ed25519 signature checks out: 0.1.1 is newer", "Downloading quietly: " + Math.round(got * 100) + "%", "SHA-256 matches the signed list", "Shard 0.1.1 is ready"];
    return { at, text: texts[at], bar: at < 2 ? "0%" : at > 2 ? "100%" : (got * 100).toFixed(0) + "%" };
  }
  trayDemo(t) {
    const a = ((t / 18) % 1) * 100;
    const b = (((t + 7) / 26) % 1) * 100;
    const tail = this.m.hist.slice(-40);
    const top = Math.max(1, ...tail);
    const spark = "M" + tail.map((x, i) => ((i / (tail.length - 1)) * 120).toFixed(1) + "," + (28 - (x / top) * 24).toFixed(1)).join(" L");
    return { spark, rows: [[Math.floor(a) + "%", a.toFixed(1) + "%"], [Math.floor(b) + "%", b.toFixed(1) + "%"]] };
  }

  // The page's text and small animations; text only every 110 ms so the numbers stay readable.
  paint(text) {
    const t = this.clock;
    const v = this.view();
    if (text) {
      set($("g-speed"), v.speed); set($("g-peak"), v.peak); set($("c-speed"), v.speed);
      set($("f-live"), v.live); set($("f-n"), v.n); set($("c-state"), v.state);
      set($("c-file"), v.file); set($("c-eta"), v.eta); set($("c-parts"), v.partCount); set($("c-pct"), v.pct);
      set($("c-caption"), v.caption);
      set($("lim-note"), this.limitNote(t)); set($("res-note"), this.resumeNote(t));
      for (const el of document.querySelectorAll(".t-speed")) set(el, v.speed);
      const tray = this.trayDemo(t);
      $("t-spark").setAttribute("d", tray.spark);
      set($("tr0"), v.pct); $("tb0").style.width = v.pctCss;
      tray.rows.forEach(([pct, bar], i) => { set($("tr" + (i + 1)), pct); $("tb" + (i + 1)).style.width = bar; });
      $("t-icon").style.height = v.pctCss;
      const sch = this.scheduleDemo(t);
      set($("s-time"), sch.time); set($("s-state"), sch.state);
    }
    const hand = this.handDemo(t);
    const chip = $("h-chip");
    chip.style.transform = hand.chip; chip.style.opacity = hand.chipOp;
    const card = $("h-card");
    card.style.opacity = hand.cardOp; card.style.boxShadow = "0 0 40px rgba(163, 230, 53, " + hand.glow + ")";
    set($("h-pct"), hand.pct); $("h-bar").style.width = hand.bar;
    const ref = this.refreshDemo(t);
    const url = $("r-url");
    set(url, ref.url); url.style.color = ref.urlColor;
    const rb = $("r-bar");
    rb.style.width = ref.bar; rb.style.background = ref.color; rb.style.boxShadow = "0 0 16px " + ref.color;
    set($("r-note"), ref.note);
    const upd = this.updateDemo(t);
    [...$("u-chain").children].forEach((el, i) => { el.className = i < upd.at ? "done" : i === upd.at ? "now" : ""; });
    $("u-bar").style.width = upd.bar;
    set($("u-text"), upd.text);
    $("u-ready").hidden = upd.at !== 4;
  }
}
new Demo().start();

// Downloads: the latest release's files, with the visitor's own platform first.
const RELEASES = "https://github.com/manyeya/shard/releases";
const LABELS = {
  "macos-universal.dmg": "Disk image",
  setup: "Installer",
  "windows-x64.zip": "Portable zip",
  "linux-x64.AppImage": "AppImage for x64",
  "linux-arm64.AppImage": "AppImage for arm64",
  "amd64.deb": ".deb for x64",
  "arm64.deb": ".deb for arm64",
  "x86_64.rpm": ".rpm for x64",
  "aarch64.rpm": ".rpm for arm64",
  "linux-x64.tar.gz": "Tarball for x64",
  "linux-arm64.tar.gz": "Tarball for arm64",
};
// The extension's pages in each store: the ids go here once each listing is live.
const STORES = [
  { name: "Chrome Web Store", url: null },
  { name: "Edge Add-ons", url: null },
  { name: "Firefox Add-ons", url: null },
];
const NAMES = { mac: "macOS", windows: "Windows", linux: "Linux" };

function visitor() {
  const ua = navigator.userAgent;
  const platform = navigator.userAgentData?.platform || navigator.platform || "";
  if (/android|iphone|ipad|ipod/i.test(ua)) return null;
  // iPads ask for the desktop site and say they're Macs; their touch screen gives them away.
  if (/mac/i.test(platform)) return navigator.maxTouchPoints > 1 ? null : "mac";
  if (/win/i.test(platform)) return "windows";
  if (/linux|x11/i.test(platform)) return "linux";
  return null;
}
function bestFile(os) {
  if (os === "mac") return "macos-universal.dmg";
  if (os === "windows") return "setup";
  if (os === "linux") return /aarch64|arm64/i.test(`${navigator.userAgent} ${navigator.platform}`) ? "linux-arm64.AppImage" : "linux-x64.AppImage";
  return null;
}
function fileLink(href, text, size, best) {
  const a = document.createElement("a");
  a.className = best ? "file best" : "file";
  a.href = href;
  a.append(text);
  if (size) {
    const small = document.createElement("small");
    small.textContent = size;
    a.append(small);
  }
  return a;
}
const megabytes = (n) => `${(n / 1e6).toFixed(1)} MB`;

const os = visitor();
const best = bestFile(os);
if (os) {
  set($("get-label"), `Download for ${NAMES[os]}`);
  set($("get2"), `Download for ${NAMES[os]}`);
  document.querySelector(`.plat[data-os="${os}"]`).classList.add("here");
} else {
  $("get").href = "#download";
}

function showRelease(release) {
  const assets = release.assets || [];
  const find = (hint) => assets.find((a) => (hint === "setup" ? a.name.startsWith("Shard-Setup-") : a.name.endsWith(hint)));
  const version = release.tag_name.replace(/^v/, "");
  const date = new Date(release.published_at).toLocaleDateString(undefined, { day: "numeric", month: "long", year: "numeric" });
  const line = $("release-line");
  line.textContent = `Version ${version}, released ${date}. `;
  const notes = document.createElement("a");
  notes.href = release.html_url;
  notes.textContent = "What's new";
  line.append(notes);

  for (const box of document.querySelectorAll(".files[data-files]")) {
    for (const hint of box.dataset.files.split(" ")) {
      const asset = find(hint);
      if (asset) box.append(fileLink(asset.browser_download_url, LABELS[hint], megabytes(asset.size), hint === best));
    }
    if (!box.children.length) box.append(fileLink(release.html_url, "On GitHub"));
  }

  const sums = find("SHA256SUMS");
  if (sums) {
    const checksums = $("checksums");
    checksums.append("To check a download, compare its SHA-256 with the one in ");
    const a = document.createElement("a");
    a.href = sums.browser_download_url;
    a.textContent = "SHA256SUMS";
    checksums.append(a, ".");
  }

  const asset = best && find(best);
  if (asset) {
    $("get").href = asset.browser_download_url;
    $("get2").href = asset.browser_download_url;
    set($("get-note"), `Version ${version}, ${megabytes(asset.size)}. Free.`);
  }
}

function noRelease(none) {
  set($("release-line"), none
    ? "There's no release yet. The first one will be listed here and on GitHub."
    : "GitHub didn't answer just now, so here are its release pages instead.");
  for (const box of document.querySelectorAll(".files[data-files]")) box.append(fileLink(none ? RELEASES : `${RELEASES}/latest`, "Releases on GitHub"));
  if (none) {
    set($("get-note"), "No release yet. Follow along on GitHub.");
    $("get").href = RELEASES;
    $("get2").href = RELEASES;
  }
}

// The stores: a link where the listing is live, and a note where it's still on its way.
for (const box of document.querySelectorAll("[data-stores]")) {
  for (const store of STORES) {
    if (store.url) box.append(fileLink(store.url, store.name));
    else {
      const soon = document.createElement("span");
      soon.className = "file soon";
      soon.append(store.name);
      const small = document.createElement("small");
      small.textContent = "On its way";
      soon.append(small);
      box.append(soon);
    }
  }
}

fetch("https://api.github.com/repos/manyeya/shard/releases/latest", { headers: { Accept: "application/vnd.github+json" } })
  .then((r) => (r.ok ? r.json() : Promise.reject(r.status)))
  .then(showRelease, (status) => noRelease(status === 404));
