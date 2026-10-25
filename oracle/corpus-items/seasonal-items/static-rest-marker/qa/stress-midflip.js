// Mid-transition stress for "Static Rest Marker" -- run through
// `playwright -s=<session> run-code --filename qa/stress-midflip.js` after
// `ctqa load` + `ctqa camlook` (see qa/run-stress.sh).
//
// Every click is a real CDP mouse event (page.mouse), timed on the Node side
// so extra clicks land at exact offsets inside a flip. A passive in-page
// recorder samples, every frame: the three tile rotations (raw SFRotation and
// Rodrigues flip angle), both clocks' isActive/startTime, both touchTimes,
// both gate depths and the live scene bbox. A capture-phase canvas listener
// timestamps every mousedown X_ITE actually received. Nothing writes to the
// model.
async page => {
  const PI = 3.141593;
  await page.evaluate(() => {
    const s = window.QA.scene, n = x => s.getNamedNode(x);
    const ang = q => { const k = [q.x, q.y, q.z], a = q.angle, c = Math.cos(a), si = Math.sin(a);
      const v = [0, 1, 0], kv = k[1], cr = [k[1]*v[2]-k[2]*v[1], k[2]*v[0]-k[0]*v[2], k[0]*v[1]-k[1]*v[0]];
      const r = [0, 1, 2].map(i => v[i]*c + cr[i]*si + k[i]*kv*(1-c));
      let t = Math.atan2(r[2], r[1]) * 180 / Math.PI; if (t < -179.99) t += 360; return t; };
    const canvas = document.querySelector('X3DCanvas');
    window.__st = { rows: [], downs: [], on: false };
    canvas.addEventListener('mousedown', () => { if (window.__st.on) window.__st.downs.push(performance.now()); }, true);
    window.__stSnap = () => {
      const T = [1, 2, 3].map(i => n('Tile' + i).rotation);
      const b = window.computeSceneBBox(s);
      return { t: performance.now(),
        deg: T.map(ang), raw: T.map(q => [q.x, q.y, q.z, q.angle]),
        fA: n('FwdClock').isActive, rA: n('RevClock').isActive,
        fS: Number(n('FwdClock').startTime), rS: Number(n('RevClock').startTime),
        fT: Number(n('FwdTouch').touchTime), rT: Number(n('RevTouch').touchTime),
        fg: n('FwdGate').translation.z, rg: n('RevGate').translation.z,
        bmin: b ? b.min : null, bmax: b ? b.max : null };
    };
    window.__stStart = () => { window.__st.rows = []; window.__st.downs = []; window.__st.on = true;
      const f = () => { if (!window.__st.on) return; window.__st.rows.push(window.__stSnap()); requestAnimationFrame(f); }; f(); };
    window.__stStop = () => { window.__st.on = false; return window.__st; };
  });
  const p = await page.evaluate(() => QA.project([0, -0.92, 0.19]));
  const sleep = ms => page.waitForTimeout(ms);   // run-code sandbox has no setTimeout
  const click = async () => { await page.mouse.move(p.x, p.y); await page.mouse.down(); await page.mouse.up(); };

  const stateOf = r => {
    const at = v => r.deg.every(d => Math.abs(d - v) < 1e-4);
    if (at(0) && !r.fA && !r.rA && Math.abs(r.fg) < 1e-6 && Math.abs(r.rg + 0.284) < 1e-6) return 'A';
    if (at(180) && !r.fA && !r.rA && Math.abs(r.fg + 0.284) < 1e-6 && Math.abs(r.rg) < 1e-6) return 'B';
    return 'INVALID';
  };

  // One trigger + extra clicks at the given ms offsets (after the trigger's
  // mouseup), then 2.4 s settle. Returns the evidence and its verdicts.
  const run = async (name, offsets) => {
    const pre = stateOf(await page.evaluate(() => window.__stSnap()));
    await page.evaluate(() => window.__stStart());
    await click();
    const t0 = Date.now(); const sent = [];
    for (const off of offsets) {
      const w = off - (Date.now() - t0); if (w > 0) await sleep(w);
      sent.push(Date.now() - t0); await click();
    }
    const w = 2400 - (Date.now() - t0); if (w > 0) await sleep(w);
    const st = await page.evaluate(() => window.__stStop());
    const rows = st.rows, last = rows[rows.length - 1], first = rows[0];
    const post = stateOf(last);
    const trig = st.downs[0];
    const extraRel = st.downs.slice(1).map(d => +((d - trig) / 1000).toFixed(3));
    // touch acceptances / clock activations seen by the recorder
    let touches = 0, acts = 0, starts = 0;
    for (let i = 1; i < rows.length; i++) {
      const a = rows[i - 1], b = rows[i];
      if (b.fT !== a.fT) touches++; if (b.rT !== a.rT) touches++;
      if (b.fA && !a.fA) acts++; if (b.rA && !a.rA) acts++;
      if (b.fS !== a.fS) starts++; if (b.rS !== a.rS) starts++;
    }
    if (first.fA || first.rA) acts = -1;
    const fwd = pre === 'A', from = fwd ? 0 : 180, to = fwd ? 180 : 0;
    const mono = [0, 1, 2].map(k => rows.every((r, i) => i === 0 ||
      (fwd ? r.deg[k] >= rows[i - 1].deg[k] - 1e-6 : r.deg[k] <= rows[i - 1].deg[k] + 1e-6)));
    const inRange = rows.every(r => r.deg.every(d => d > -0.01 && d < 180.01) &&
      r.raw.every(q => q[3] >= -1e-6 && q[3] <= PI + 1e-5));
    // landing time per tile, relative to the trigger mousedown
    const land = [0, 1, 2].map(k => { const r = rows.find(r => Math.abs(r.deg[k] - to) < 1e-4);
      return r ? +((r.t - trig) / 1000).toFixed(3) : null; });
    // mid-transition: any frame where a tile has moved but not landed, after all extra clicks
    const lastExtra = st.downs.length > 1 ? st.downs[st.downs.length - 1] : trig;
    const movingAfter = rows.some(r => r.t > lastExtra && r.deg.some(d => Math.abs(d - from) > 0.5 && Math.abs(d - to) > 0.5));
    const extraWhileMoving = st.downs.slice(1).map(d => {
      const r = rows.reduce((b, r) => (Math.abs(r.t - d) < Math.abs(b.t - d) ? r : b));
      return r.fA || r.rA; });
    const zmax = Math.max(...rows.map(r => r.bmax ? r.bmax[2] : -Infinity));
    const bmin = [0, 1, 2].map(i => Math.min(...rows.map(r => r.bmin[i])));
    const bmax = [0, 1, 2].map(i => Math.max(...rows.map(r => r.bmax[i])));
    const checks = {
      'pre/post states': `${pre}->${post}`,
      'extra clicks sent / delivered to canvas': `${offsets.length}/${extraRel.length}`,
      'every extra click landed while a clock was active': extraWhileMoving.every(Boolean),
      'touches accepted == 1': touches === 1,
      'clock activations == 1': acts === 1,
      'clock startTime writes == 1': starts === 1,
      'exactly one flip, to the opposite state': (pre === 'A' && post === 'B') || (pre === 'B' && post === 'A'),
      'monotonic every tile (no stall reversal / restart)': mono.every(Boolean),
      'raw angle within [0, pi] every frame (no 2pi accumulation)': inRange,
      'tiles landed in staggered order ~0.117 s apart': land.every(x => x !== null) &&
        (fwd ? [land[1] - land[0], land[2] - land[1]] : [land[1] - land[2], land[0] - land[1]]).every(d => d > 0.08 && d < 0.16),
      'tiles still moving after last extra click (click was truly mid-flip)': offsets.length === 0 || movingAfter,
    };
    const ok = ['every extra click landed while a clock was active', 'touches accepted == 1', 'clock activations == 1',
      'clock startTime writes == 1', 'exactly one flip, to the opposite state', 'monotonic every tile (no stall reversal / restart)',
      'raw angle within [0, pi] every frame (no 2pi accumulation)', 'tiles landed in staggered order ~0.117 s apart',
      'tiles still moving after last extra click (click was truly mid-flip)'].every(k => checks[k] === true) &&
      extraRel.length === offsets.length;
    return { name, dir: fwd ? 'A->B' : 'B->A', ok, extraClickRel_s: extraRel, sentAt_ms: sent, land_s: land,
      frames: rows.length, finalRaw: last.raw, bboxMin: bmin, bboxMax: bmax, zmax, checks };
  };

  const out = { pixel: p, cases: [], endurance: [] };
  const burst = Array.from({ length: 13 }, (_, i) => 100 + i * 95);   // 100 ... 1240 ms
  const plan = [
    ['A burst forward', burst], ['B burst reverse', burst],
    ['C early ~100 ms forward', [100]], ['C early ~100 ms reverse', [100]],
    ['D middle ~750 ms forward', [750]], ['D middle ~750 ms reverse', [750]],
    ['E late ~1250 ms forward', [1250]], ['E late ~1250 ms reverse', [1250]],
  ];
  for (const [name, offs] of plan) out.cases.push(await run(name, offs));
  for (let i = 1; i <= 24; i++) out.endurance.push(await run('toggle ' + i, []));
  return JSON.stringify(out);
}
