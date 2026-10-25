#!/usr/bin/env node
// Structural facts about ONE shipping artifact, read by WRLForge's own parser
// and validator (no regex parsing here). Reads the gzip bytes on disk, inflates
// them, and prints JSON for work/30_gates.py.
//
//   node qa/wrlforge-report.js <artifact.wrl>
//
// WRLForge location: ~/Projects/cybertown/wrlforge (sibling repository).
'use strict';
const fs = require('fs');
const zlib = require('zlib');
const path = require('path');

const WRLFORGE = process.env.WRLFORGE ||
  path.resolve(__dirname, '../../../../../wrlforge');
const { parse } = require(path.join(WRLFORGE, 'src/vrml'));
const { validate } = require(path.join(WRLFORGE, 'validator.js'));

const file = process.argv[2];
const bytes = fs.readFileSync(file);
const gz = bytes[0] === 0x1f && bytes[1] === 0x8b;
const text = (gz ? zlib.gunzipSync(bytes) : bytes).toString('utf8');
const p = parse(text);
const v = validate(text);

const counts = {};
const materials = [];
const transforms = [];
const urls = [];
let ifs = 0;

function nums(val) {
  if (!val || val.type !== 'Numbers') return null;
  return val.values.map((n) => n.lexeme);
}

function walk(n) {
  if (!n || typeof n !== 'object') return;
  if (Array.isArray(n)) { n.forEach(walk); return; }
  if (n.type === 'Node') {
    counts[n.nodeType] = (counts[n.nodeType] || 0) + 1;
    if (n.nodeType === 'IndexedFaceSet') ifs += 1;
    if (n.nodeType === 'Material') {
      const f = {};
      for (const fl of n.fields) f[fl.name] = nums(fl.value);
      materials.push({ def: n.def, fields: f });
    }
    if (n.nodeType === 'Transform') {
      const f = {};
      for (const fl of n.fields) {
        if (['translation', 'rotation', 'scale', 'center',
             'scaleOrientation'].includes(fl.name)) f[fl.name] = nums(fl.value);
      }
      transforms.push({ def: n.def, fields: f });
    }
    for (const fl of n.fields || []) {
      if (fl.name === 'url') urls.push(JSON.stringify(fl.value));
    }
  }
  for (const k of Object.keys(n)) {
    if (!/Range$|^range$|^loc$/.test(k)) walk(n[k]);
  }
}
walk(p.tree);

// ---- rendered instances: every Shape reached through the scene graph, with
// the chain of Transform fields above it (USE expanded against WRLForge's own
// DEF table). Geometry arrays are the parser's numbers, not re-lexed text.
const defNode = {};
for (const d of p.defs) defNode[d.name] = d.node;

function field(n, name) {
  for (const fl of n.fields || []) if (fl.name === name) return fl.value;
  return null;
}
function flat(val) {
  if (!val) return [];
  if (val.type === 'Numbers') return val.values.map((x) => x.value);
  if (val.type === 'Array') return val.items.map((x) => x.value);
  return [];
}
const shapesByKey = new Map();
const instances = [];
function shapeKey(n) {
  if (!shapesByKey.has(n)) {
    const app = field(n, 'appearance');
    const mat = app && app.type === 'Node' ? field(app, 'material') : null;
    const geo = field(n, 'geometry');
    const coord = geo ? field(geo, 'coord') : null;
    shapesByKey.set(n, {
      id: shapesByKey.size,
      material: mat && mat.type === 'Node' ? mat.def : null,
      creaseAngle: geo ? flat(field(geo, 'creaseAngle'))[0] : null,
      solid: geo && field(geo, 'solid') ? JSON.stringify(field(geo, 'solid')) : null,
      point: coord ? flat(field(coord, 'point')) : [],
      coordIndex: geo ? flat(field(geo, 'coordIndex')) : [],
    });
  }
  return shapesByKey.get(n).id;
}
function visit(n, chain) {
  if (!n) return;
  if (n.type === 'Use') { visit(defNode[n.name], chain); return; }
  if (n.type !== 'Node') return;
  if (n.nodeType === 'Shape') {
    instances.push({ shape: shapeKey(n), chain });
    return;
  }
  let next = chain;
  if (n.nodeType === 'Transform') {
    next = chain.concat([{
      translation: flat(field(n, 'translation')),
      rotation: flat(field(n, 'rotation')),
      scale: flat(field(n, 'scale')),
    }]);
  }
  const kids = field(n, 'children');
  if (kids && kids.type === 'Array') kids.items.forEach((k) => visit(k, next));
  else if (kids) visit(kids, next);
}
(p.tree.statements || []).forEach((s) => visit(s, []));
const shapes = [...shapesByKey.values()];

const worldInfo = [];
for (const st of p.tree.statements || []) {
  if (st.type === 'Node' && st.nodeType === 'WorldInfo') {
    const o = {};
    for (const fl of st.fields) {
      if (fl.value.type === 'String') o[fl.name] = fl.value.value;
      else if (fl.value.type === 'Array') o[fl.name] = fl.value.items.map((i) => i.value);
    }
    worldInfo.push(o);
  }
}

console.log(JSON.stringify({
  file: path.basename(file),
  artifact_is_gzip: gz,
  artifact_bytes: bytes.length,
  header: text.split('\n', 1)[0],
  wrlforge: {
    ok: v.ok, gzipBytes_reencoded: v.gzipBytes, rawBytes: v.rawBytes,
    results: v.results.map((r) => ({ pass: r.pass, severity: r.severity,
                                     name: r.name, detail: r.detail })),
  },
  diagnostics: p.diagnostics.map((d) => ({ severity: d.severity, code: d.code,
                                           message: d.message })),
  duplicate_defs: p.duplicateDefs.map((d) => d.name || d),
  defs: p.defs.map((d) => d.name),
  uses: p.uses.map((u) => ({ name: u.name, resolved: u.resolved })),
  routes: p.routes.length,
  node_counts: counts,
  indexed_face_sets: ifs,
  materials,
  transforms,
  urls,
  world_info: worldInfo,
  shapes,
  instances,
}));
