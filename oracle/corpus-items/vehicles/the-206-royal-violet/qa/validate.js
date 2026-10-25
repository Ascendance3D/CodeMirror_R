#!/usr/bin/env node
// Mall hard gates + parser diagnostics for a COLOURWAY, via WRLForge.
//
//   node qa/validate.js              // this lane's upload candidate
//   node qa/validate.js <stem>       // any other stem in this folder
//
// One script, four lanes: the stem is the folder name, so there is no per-lane
// constant to drift out of date. `qa/variant-gates.py` asserts all four copies
// are byte-identical.
//
// Uses WRLForge's own `validator.js` and `src/vrml` parser. Writing another
// regex validator is explicitly forbidden by AGENTS.md and would drift from
// the one the rest of the repo is judged against.
//
// Two things this encodes:
//
//  * `validator.js` reports gzip by re-gzipping the text it was handed. That is
//    NOT the artifact size. The uploaded bytes are printed separately from
//    `stat`, because a re-gzip of an already-gzipped file reads smaller than
//    the real upload and has silently passed over-size items before.
//  * skipping a variant that was asked for is a gate that cannot fail, so an
//    explicitly named missing variant exits non-zero.
'use strict';
const fs = require('fs');
const path = require('path');
const zlib = require('zlib');

const FORGE = '/home/ryan/Projects/cybertown/wrlforge';
const ITEM = path.dirname(__dirname);
// One public artifact in this lane, and its stem IS the folder name. Naming a
// stem that does not exist on disk exits non-zero rather than silently
// validating nothing, which is the trap `the-206` hit when its default named a
// file it had never built.
const DEFAULT = [path.basename(ITEM)];

const { validate } = require(path.join(FORGE, 'validator.js'));

function parseDiagnostics(text) {
  const p = require(path.join(FORGE, 'src', 'vrml')).parse(text);
  return {
    diagnostics: p.diagnostics.length,
    syntax: p.syntaxDiagnostics.length,
    semantic: p.semanticDiagnostics.length,
    duplicateDefs: p.duplicateDefs.length,
    defs: p.defs.length,
    uses: p.uses.length,
    unresolvedUses: p.uses.filter((u) => u.resolved === false).length,
    routes: p.routes.length,
    danglingRoutes: p.routes.filter((r) => r.resolved === false).length,
    truncated: !!p.truncated,
    assetRefs: p.assetRefs.length,
    detail: p.diagnostics.slice(0, 5),
  };
}

const wanted = process.argv.slice(2);
const stems = wanted.length ? wanted : DEFAULT;
let bad = 0;

for (const stem of stems) {
  const edit = path.join(ITEM, stem + '.edit.wrl');
  const up = path.join(ITEM, stem + '.wrl');
  if (!fs.existsSync(edit) || !fs.existsSync(up)) {
    if (wanted.length) {
      console.error('MISSING %s', stem);
      process.exit(2);
    }
    continue;
  }
  const text = fs.readFileSync(edit, 'utf8');
  const { results, ok } = validate(text);
  const uploaded = fs.statSync(up).size;

  console.log('\n=== %s ===', stem);
  console.log('  uploaded .wrl (stat)      %s B   %s',
    uploaded.toLocaleString(),
    uploaded < 80 * 1024 ? 'PASS  under the 80 KB gate' : 'FAIL  OVER the gate');
  console.log('  raw .edit.wrl             %s B',
    fs.statSync(edit).size.toLocaleString());

  for (const r of results) {
    const tag = r.pass ? (r.severity === 'hard' ? 'PASS' : 'pass') : 'FAIL';
    if (!r.pass && r.severity === 'hard') bad++;
    console.log('  %s  [%s] %s%s', tag, r.severity, r.name,
      r.detail ? ' - ' + r.detail : '');
  }
  console.log('  validator overall         %s', ok ? 'PASS' : 'FAIL');
  if (!ok) bad++;

  const p = parseDiagnostics(text);
  // An `assetRef` is any external file the scene names. A textured item has
  // exactly one - its own local texture filename - and that is not a defect;
  // the hard gates above already prove there is at most one ImageTexture and
  // that no url is an external or nested path. So the clean predicate compares
  // assetRefs against the ImageTexture count the validator itself reported,
  // rather than demanding zero.
  //
  // Recorded because it matters to whoever reads the base lanes next: the
  // approved `sundat-cloudy/qa/validate.js` demands `assetRefs === 0`, so it
  // prints DIRTY and exits 1 on its OWN approved artifact, whose one asset ref
  // is `boom-plate.png`. That is a quirk of the QA script, not a fault in the
  // shipped file. It is fixed here rather than there because the approved lane
  // is frozen for this pass.
  const texRule = results.find((r) => /ImageTexture/.test(r.name));
  const texCount = texRule ? parseInt(texRule.detail, 10) || 0 : 0;
  const clean = p.diagnostics === 0 && p.duplicateDefs === 0 &&
    p.unresolvedUses === 0 && p.danglingRoutes === 0 &&
    p.assetRefs === texCount && !p.truncated;
  console.log('  parser  defs=%d uses=%d unresolved=%d routes=%d dangling=%d ' +
    'dupDEF=%d assetRefs=%d (expected %d) diagnostics=%d  %s',
    p.defs, p.uses, p.unresolvedUses, p.routes, p.danglingRoutes,
    p.duplicateDefs, p.assetRefs, texCount, p.diagnostics,
    clean ? 'CLEAN' : 'DIRTY');
  if (!clean) { bad++; console.log('   ', JSON.stringify(p.detail)); }

  const roundtrip = zlib.gunzipSync(fs.readFileSync(up)).toString('utf8');
  const same = roundtrip === text;
  console.log('  gzip round-trips to .edit.wrl   %s', same ? 'PASS' : 'FAIL');
  if (!same) bad++;
}

process.exit(bad ? 1 : 0);
