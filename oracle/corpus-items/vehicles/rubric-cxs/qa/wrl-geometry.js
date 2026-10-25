#!/usr/bin/env node
'use strict';
// wrl-geometry.js -- dump the shipped .wrl's geometry as JSON, for parity work.
//
//   node qa/wrl-geometry.js <item.wrl> > geom.json
//
// Parsing is WRLForge's VRML97 parser (src/vrml), the workspace's one parser --
// this adds a CONSUMER of it, not a second parser, and deliberately does no
// regex work on the file text.  See AGENTS.md "Tooling".
//
// Output, per Shape: the material's DEF name and the IndexedFaceSet's points
// and coordIndex, in the file's own local coordinates.  Mapping those back to
// Blender space is the caller's job and is done in one place --
// `qa/blend-wrl-parity.py` -- so the convention is written down once.

const fs = require('fs');
const path = require('path');
const zlib = require('zlib');

const WRLFORGE = process.env.WRLFORGE || path.resolve(__dirname, '../../../../../wrlforge');
let parse, walk;
try {
  ({ parse } = require(path.join(WRLFORGE, 'src/vrml')));
  ({ walk } = require(path.join(WRLFORGE, 'src/vrml/ast')));
} catch (e) {
  console.error(`wrl-geometry: cannot load WRLForge's VRML parser from ${WRLFORGE}`);
  console.error('  (' + e.message + ')');
  process.exit(2);
}

const file = process.argv[2];
if (!file) {
  console.error('usage: node wrl-geometry.js <item.wrl>');
  process.exit(2);
}
const buf = fs.readFileSync(file);
const isGz = buf.length > 2 && buf[0] === 0x1f && buf[1] === 0x8b;
const text = (isGz ? zlib.gunzipSync(buf) : buf).toString('utf8');

const ast = parse(text);

const fieldOf = (node, name) =>
  (node.fields || []).find((f) => f.type === 'Field' && f.name === name) || null;
const valueOf = (node, name) => {
  const f = fieldOf(node, name);
  return f ? f.value : null;
};
function numberList(v) {
  if (!v) return [];
  if (v.type === 'Number') return [v.value];
  if (v.type === 'Numbers') return v.values.map((n) => (typeof n === 'number' ? n : n.value));
  if (v.type === 'Array') {
    const out = [];
    for (const item of v.items) out.push(...numberList(item));
    return out;
  }
  return [];
}
function childNodes(v) {
  if (!v) return [];
  if (v.type === 'Node' || v.type === 'Use') return [v];
  if (v.type === 'Array') return v.items.filter((i) => i.type === 'Node' || i.type === 'Use');
  return [];
}

// DEF table, so a USEd Material or Coordinate resolves to the node it names.
const defs = new Map();
walk(ast, (n) => {
  if (n && n.type === 'Node' && n.def) defs.set(n.def, n);
});
const deref = (n) => (n && n.type === 'Use' ? defs.get(n.name) || null : n);

// The single root placement Transform, reported so the caller can invert it.
let root = null;
walk(ast, (n) => {
  if (root) return;
  if (n && n.type === 'Node' && n.nodeType === 'Transform') {
    root = {
      def: n.def || null,
      translation: numberList(valueOf(n, 'translation')),
      rotation: numberList(valueOf(n, 'rotation')),
      scale: numberList(valueOf(n, 'scale')),
    };
  }
});

const shapes = [];
walk(ast, (n) => {
  if (!n || n.type !== 'Node' || n.nodeType !== 'Shape') return;
  const app = deref(childNodes(valueOf(n, 'appearance'))[0]);
  const mat = app ? deref(childNodes(valueOf(app, 'material'))[0]) : null;
  const geom = deref(childNodes(valueOf(n, 'geometry'))[0]);
  if (!geom || geom.nodeType !== 'IndexedFaceSet') return;
  const coord = deref(childNodes(valueOf(geom, 'coord'))[0]);
  const pts = coord ? numberList(valueOf(coord, 'point')) : [];
  shapes.push({
    material: mat ? mat.def || mat.name || null : null,
    diffuse: mat ? numberList(valueOf(mat, 'diffuseColor')) : [],
    solid: (() => {
      const v = valueOf(geom, 'solid');
      return v && v.type === 'Bool' ? v.value : null;
    })(),
    creaseAngle: numberList(valueOf(geom, 'creaseAngle'))[0] ?? null,
    points: pts,
    coordIndex: numberList(valueOf(geom, 'coordIndex')),
  });
});

process.stdout.write(JSON.stringify({ file, gzipped: isGz, root, shapes }));
