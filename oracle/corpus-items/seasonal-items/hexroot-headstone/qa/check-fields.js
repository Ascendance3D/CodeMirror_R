#!/usr/bin/env node
// VRML97 field-value legality for "Hexroot Headstone".
//
// Walks WRLForge's own parse tree (no second parser) and checks what a node
// name scan cannot: numeric ranges, interpolator key/keyValue agreement,
// IndexedFaceSet index ranges, DEF-before-USE order, ROUTE field types and
// event loops.
//
//   node qa/check-fields.js hexroot-headstone.wrl      (gzip or text)
'use strict';
const fs = require('fs');
const path = require('path');
const zlib = require('zlib');
const WF = path.resolve(__dirname, '../../../../../wrlforge');
const { parse } = require(path.join(WF, 'src/vrml'));

const buf = fs.readFileSync(process.argv[2]);
const text = buf[0] === 0x1f && buf[1] === 0x8b ? zlib.gunzipSync(buf).toString('utf8') : buf.toString('utf8');
const doc = parse(text);
let fails = 0;
const out = (ok, label, detail = '') => {
  if (!ok) fails++;
  console.log(`${ok ? 'PASS' : 'FAIL'}  ${label}${detail ? '  :: ' + detail : ''}`);
};

const nums = v => (v && v.type === 'Numbers') ? v.values : (v && v.type === 'Number') ? [v] : (v && v.type === 'Array') ? v.items.flatMap(nums) : [];
const vals = v => nums(v).map(n => n.value);
const field = (node, name) => (node.fields || []).find(f => f.name === name);

// ---- collect nodes in document order, tracking DEF/USE order
const nodes = [];
const defsSeen = new Set();
const forwardUse = [];
const defs = new Map();
(function walk(o) {
  if (!o || typeof o !== 'object') return;
  if (Array.isArray(o)) return o.forEach(walk);
  if (o.type === 'Node') {
    if (o.def) { defsSeen.add(o.def); defs.set(o.def, o); }
    nodes.push(o);
  }
  if (o.type === 'Use' && !defsSeen.has(o.name)) forwardUse.push(o.name);
  for (const [k, v] of Object.entries(o)) if (!k.endsWith('Range') && k !== 'range') walk(v);
})(doc.tree);

// ---- numbers
const all = [];
(function walk(o) {
  if (!o || typeof o !== 'object') return;
  if (Array.isArray(o)) return o.forEach(walk);
  if (o.type === 'Number') all.push(o);
  Object.values(o).forEach(walk);
})(doc.tree);
out(all.every(n => n.valid && Number.isFinite(n.value)), `all ${all.length} numbers finite and lexically valid`);

// ---- per node type
const in01 = a => a.every(x => x >= 0 && x <= 1);
let ifsCount = 0, faces = 0, tris = 0;
for (const n of nodes) {
  const id = n.def || n.nodeType;
  switch (n.nodeType) {
    case 'Transform': {
      const s = field(n, 'scale');
      if (s) out(vals(s.value).every(x => x > 0), `${id}.scale > 0`, vals(s.value).join(' '));
      const r = field(n, 'rotation');
      if (r) { const [x, y, z] = vals(r.value); out(Math.hypot(x, y, z) > 0.999 && Math.hypot(x, y, z) < 1.001, `${id}.rotation axis is unit`, vals(r.value).join(' ')); }
      break;
    }
    case 'Material': {
      for (const f of n.fields) {
        const v = vals(f.value);
        out(in01(v), `${id}.${f.name} in [0,1]`, v.join(' '));
      }
      break;
    }
    case 'TimeSensor': {
      const c = vals(field(n, 'cycleInterval').value)[0];
      out(c > 0, `${id}.cycleInterval > 0`, c);
      const st = vals(field(n, 'startTime').value)[0], sp = vals(field(n, 'stopTime').value)[0];
      out(sp > st, `${id} inactive at load (stopTime > startTime, both past)`, `start ${st} stop ${sp}`);
      break;
    }
    case 'ColorInterpolator': case 'ScalarInterpolator': case 'PositionInterpolator': {
      const k = vals(field(n, 'key').value), kv = vals(field(n, 'keyValue').value);
      const ar = n.nodeType === 'ScalarInterpolator' ? 1 : 3;
      out(k.every((x, i) => i === 0 || x >= k[i - 1]) && k[0] === 0 && k[k.length - 1] === 1,
        `${id}.key non-decreasing 0..1`, `${k.length} keys`);
      out(kv.length === k.length * ar, `${id}.keyValue count matches key`, `${kv.length} / ${k.length}x${ar}`);
      if (n.nodeType === 'ColorInterpolator') out(in01(kv), `${id}.keyValue colours in [0,1]`);
      break;
    }
    case 'IndexedFaceSet': {
      ifsCount++;
      const coord = field(n, 'coord').value, pts = vals(field(coord, 'point').value).length / 3;
      const ci = vals(field(n, 'coordIndex').value);
      const tcF = field(n, 'texCoord');
      const tci = field(n, 'texCoordIndex') ? vals(field(n, 'texCoordIndex').value) : null;
      const tpts = tcF ? vals(field(tcF.value, 'point').value).length / 2 : 0;
      let ok = true, cur = 0, okLen = true;
      for (const i of ci) {
        if (i === -1) { if (cur < 3) okLen = false; faces++; tris += cur - 2; cur = 0; continue; }
        if (!(Number.isInteger(i) && i >= 0 && i < pts)) ok = false;
        cur++;
      }
      out(ok && okLen && ci[ci.length - 1] === -1, `IFS#${ifsCount} coordIndex in range, faces >= 3 verts`, `${pts} points`);
      if (tci) {
        const same = tci.length === ci.length && tci.every((t, j) => (t === -1) === (ci[j] === -1));
        out(same && tci.every(t => t === -1 || (t >= 0 && t < tpts)), `IFS#${ifsCount} texCoordIndex mirrors coordIndex, in range`, `${tpts} texcoords`);
      }
      const ca = field(n, 'creaseAngle');
      if (ca) out(vals(ca.value)[0] >= 0, `IFS#${ifsCount} creaseAngle >= 0`);
      const solid = field(n, 'solid');
      out(!solid || solid.value.value !== false, `IFS#${ifsCount} not solid FALSE`);
      break;
    }
  }
}
console.log(`      (${ifsCount} IndexedFaceSets, ${faces} faces, ${tris} triangles)`);

// ---- DEF/USE
out(doc.duplicateDefs.length === 0, 'no duplicate DEF', doc.duplicateDefs.map(d => d.name || d).join(','));
out(forwardUse.length === 0, 'every USE follows its DEF in document order', forwardUse.join(','));
const unresolved = doc.uses.filter(u => !defs.has(u.name));
out(unresolved.length === 0, 'every USE resolves', `${doc.uses.length} USE`);

// ---- ROUTE types (VRML97 field types of the nodes used here)
const T = {
  TouchSensor: { touchTime: 'SFTime', isActive: 'SFBool', isOver: 'SFBool' },
  TimeSensor: { set_startTime: 'SFTime', set_stopTime: 'SFTime', fraction_changed: 'SFFloat', isActive: 'SFBool', cycleTime: 'SFTime' },
  ColorInterpolator: { set_fraction: 'SFFloat', value_changed: 'SFColor' },
  ScalarInterpolator: { set_fraction: 'SFFloat', value_changed: 'SFFloat' },
  PositionInterpolator: { set_fraction: 'SFFloat', value_changed: 'SFVec3f' },
  Material: { set_emissiveColor: 'SFColor', set_diffuseColor: 'SFColor' },
  Transform: { set_translation: 'SFVec3f', set_rotation: 'SFRotation', set_scale: 'SFVec3f' },
};
const outputs = new Set(['touchTime', 'isActive', 'isOver', 'fraction_changed', 'cycleTime', 'value_changed']);
const graph = new Map();
let routeOk = true;
for (const r of doc.routes) {
  const a = defs.get(r.from.node), b = defs.get(r.to.node);
  const ta = a && T[a.nodeType] && T[a.nodeType][r.from.event];
  const tb = b && T[b.nodeType] && T[b.nodeType][r.to.event];
  const good = ta && tb && ta === tb && outputs.has(r.from.event) && r.to.event.startsWith('set_');
  if (!good) { routeOk = false; console.log(`      bad ROUTE ${r.from.node}.${r.from.event} -> ${r.to.node}.${r.to.event}`); }
  if (!graph.has(r.from.node)) graph.set(r.from.node, new Set());
  graph.get(r.from.node).add(r.to.node);
}
out(routeOk, `all ${doc.routes.length} ROUTEs resolve, eventOut -> eventIn, types match`);
// event loop: a cycle in node->node routing
const state = new Map();
let cyc = null;
const dfs = u => { state.set(u, 1); for (const v of graph.get(u) || []) {
  if (state.get(v) === 1) cyc = `${u}->${v}`; else if (!state.has(v)) dfs(v); } state.set(u, 2); };
for (const u of graph.keys()) if (!state.has(u)) dfs(u);
out(!cyc, 'ROUTE graph acyclic (no event loops)', cyc || '');
// every animation node is both driven and driving
for (const [name, n] of defs) {
  if (/Interpolator|TimeSensor/.test(n.nodeType)) {
    const driven = doc.routes.some(r => r.to.node === name), drives = doc.routes.some(r => r.from.node === name);
    out(driven && drives, `${name} is routed in and out`);
  }
}
console.log(`\nparser diagnostics: ${doc.diagnostics.length}`);
console.log(fails ? `${fails} FIELD CHECK(S) FAILED` : 'ALL FIELD CHECKS PASSED');
process.exit(fails ? 1 : 0);
