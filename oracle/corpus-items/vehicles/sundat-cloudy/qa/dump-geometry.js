#!/usr/bin/env node
// Flatten the shipped VRML into world-space geometry, using WRLForge's parser.
//
//   node qa/dump-geometry.js <file.edit.wrl> > work/geometry.json
//
// AGENTS.md forbids writing another regex VRML analyser, and it is right to:
// this file has to resolve a DEF/USE graph and a Transform stack, which is
// exactly what regex cannot do. So the PARSE comes from WRLForge's `src/vrml`
// and this script only walks the tree it returns, applying transforms and
// expanding `USE` into placed copies.
//
// The output is deliberately dumb - one record per runtime drawable, with its
// points already in world space - so `qa/geometry-gates.py` can do arithmetic
// without knowing anything about VRML.
'use strict';
const fs = require('fs');
const path = require('path');
const FORGE = '/home/ryan/Projects/cybertown/wrlforge';
const { parse } = require(path.join(FORGE, 'src', 'vrml'));

const file = process.argv[2];
const text = fs.readFileSync(file, 'utf8');
const res = parse(text);
if (res.diagnostics.length) {
  console.error('parser diagnostics: %d', res.diagnostics.length);
  process.exit(2);
}

// ---- value helpers ---------------------------------------------------------
// WRLForge wraps numeric fields two ways: `Array`/`items` and `Numbers`/`values`.
// Handling only the first silently yields [] for `creaseAngle`, which then
// disappears from the JSON entirely because JSON.stringify drops undefined -
// a missing gate rather than a failing one.
function nums(v) {
  if (!v) return [];
  if (v.type === 'Array') return v.items.flatMap(nums);
  if (v.type === 'Numbers') return v.values.flatMap(nums);
  if (typeof v.value === 'number') return [v.value];
  if (typeof v.value === 'string' && /^-?[\d.eE+]+$/.test(v.value)) return [Number(v.value)];
  return [];
}
function field(node, name) {
  const f = (node.fields || []).find((x) => x.name === name);
  return f ? f.value : null;
}
function nodesOf(v) {
  if (!v) return [];
  if (v.type === 'Array') return v.items.filter((i) => i && (i.type === 'Node' || i.type === 'Use'));
  if (v.type === 'Node' || v.type === 'Use') return [v];
  return [];
}

// ---- matrices (row-major 4x4, column vectors) ------------------------------
const I = () => [1,0,0,0, 0,1,0,0, 0,0,1,0, 0,0,0,1];
function mul(a, b) {
  const o = new Array(16).fill(0);
  for (let r = 0; r < 4; r++) for (let c = 0; c < 4; c++)
    for (let k = 0; k < 4; k++) o[r*4+c] += a[r*4+k] * b[k*4+c];
  return o;
}
function xform(m, p) {
  return [m[0]*p[0]+m[1]*p[1]+m[2]*p[2]+m[3],
          m[4]*p[0]+m[5]*p[1]+m[6]*p[2]+m[7],
          m[8]*p[0]+m[9]*p[1]+m[10]*p[2]+m[11]];
}
function trans(t) { const m = I(); m[3]=t[0]; m[7]=t[1]; m[11]=t[2]; return m; }
function scale(s) { const m = I(); m[0]=s[0]; m[5]=s[1]; m[10]=s[2]; return m; }
function rot(r) {
  const [x,y,z,a] = r; const L = Math.hypot(x,y,z);
  if (!L || !a) return I();
  const ux=x/L, uy=y/L, uz=z/L, c=Math.cos(a), s=Math.sin(a), t=1-c;
  return [t*ux*ux+c,    t*ux*uy-s*uz, t*ux*uz+s*uy, 0,
          t*ux*uy+s*uz, t*uy*uy+c,    t*uy*uz-s*ux, 0,
          t*ux*uz-s*uy, t*uy*uz+s*ux, t*uz*uz+c,    0,
          0,0,0,1];
}

// ---- walk ------------------------------------------------------------------
const defs = new Map();          // DEF name -> node
const out = [];

function register(node) {
  if (node && node.def) defs.set(node.def, node);
}

function materialName(shape) {
  const app = field(shape, 'appearance');
  if (!app || app.type !== 'Node') return null;
  const mat = field(app, 'material');
  if (!mat) return null;
  if (mat.type === 'Use') return mat.name || mat.value;
  return mat.def || mat.nodeType;
}
function textureUrl(shape) {
  const app = field(shape, 'appearance');
  if (!app || app.type !== 'Node') return null;
  const tex = field(app, 'texture');
  if (!tex || tex.type !== 'Node') return null;
  const u = field(tex, 'url');
  if (!u) return null;
  const items = u.type === 'Array' ? u.items : [u];
  return items.map((i) => i.value).join(',');
}

function emitShape(shape, M, label) {
  const geo = field(shape, 'geometry');
  if (!geo || geo.type !== 'Node' || geo.nodeType !== 'IndexedFaceSet') return;
  const coord = field(geo, 'coord');
  const pts = [];
  const raw = nums(field(coord, 'point'));
  for (let i = 0; i + 2 < raw.length; i += 3) pts.push(xform(M, [raw[i], raw[i+1], raw[i+2]]));
  const idx = nums(field(geo, 'coordIndex')).map((n) => Math.round(n));
  const faces = [];
  let cur = [];
  for (const i of idx) { if (i < 0) { if (cur.length) faces.push(cur); cur = []; } else cur.push(i); }
  if (cur.length) faces.push(cur);
  const solidF = field(geo, 'solid');
  const normalF = field(geo, 'normal');
  const creaseF = field(geo, 'creaseAngle');
  out.push({
    label,
    def: shape.def || null,
    material: materialName(shape),
    texture: textureUrl(shape),
    solid: solidF ? String(solidF.value).toUpperCase() !== 'FALSE' : true,
    hasNormals: !!normalF,
    creaseAngle: creaseF && nums(creaseF).length ? nums(creaseF)[0] : null,
    points: pts.map((p) => p.map((c) => +c.toFixed(6))),
    faces,
  });
}

function walk(node, M, trail) {
  if (!node) return;
  if (node.type === 'Use') {
    const target = defs.get(node.name || node.value);
    if (!target) { console.error('unresolved USE %s', node.name || node.value); process.exit(2); }
    walk(target, M, trail + '/USE:' + (node.name || node.value));
    return;
  }
  register(node);
  const t = node.nodeType;
  if (t === 'Transform') {
    const T = nums(field(node, 'translation'));
    const R = nums(field(node, 'rotation'));
    const S = nums(field(node, 'scale'));
    let m = M;
    if (T.length === 3) m = mul(m, trans(T));
    if (R.length === 4) m = mul(m, rot(R));
    if (S.length === 3) m = mul(m, scale(S));
    for (const c of nodesOf(field(node, 'children')))
      walk(c, m, trail + '/' + (node.def || 'Transform'));
    return;
  }
  if (t === 'Group' || t === 'Switch') {
    for (const c of nodesOf(field(node, 'children'))) walk(c, M, trail + '/' + t);
    return;
  }
  if (t === 'Shape') {
    // register the geometry/appearance DEFs inside before emitting
    const app = field(node, 'appearance');
    if (app && app.type === 'Node') { register(app); const mm = field(app, 'material'); if (mm && mm.type === 'Node') register(mm); }
    const geo = field(node, 'geometry');
    if (geo && geo.type === 'Node') register(geo);
    emitShape(node, M, trail + '/Shape');
    return;
  }
}

for (const s of res.tree.statements) walk(s, I(), '');
process.stdout.write(JSON.stringify({ file: path.basename(file), drawables: out }));
