#!/usr/bin/env node
// Mall hard gates + parser diagnostics for Sundat Cloudy, via WRLForge.
//
//   node qa/validate.js              // the upload candidate, sundat-cloudy
//   node qa/validate.js <stem>       // any other stem in this folder
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
// One public artifact in this lane. Naming a stem that does not exist on
// disk exits non-zero rather than silently validating nothing, which is the
// trap `the-206` hit when its default named a file it had never built.
const DEFAULT = ['sundat-cloudy'];

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
  const clean = p.diagnostics === 0 && p.duplicateDefs === 0 &&
    p.unresolvedUses === 0 && p.danglingRoutes === 0 &&
    p.assetRefs === 0 && !p.truncated;
  console.log('  parser  defs=%d uses=%d unresolved=%d routes=%d dangling=%d ' +
    'dupDEF=%d assetRefs=%d diagnostics=%d  %s',
    p.defs, p.uses, p.unresolvedUses, p.routes, p.danglingRoutes,
    p.duplicateDefs, p.assetRefs, p.diagnostics, clean ? 'CLEAN' : 'DIRTY');
  if (!clean) { bad++; console.log('   ', JSON.stringify(p.detail)); }

  const roundtrip = zlib.gunzipSync(fs.readFileSync(up)).toString('utf8');
  const same = roundtrip === text;
  console.log('  gzip round-trips to .edit.wrl   %s', same ? 'PASS' : 'FAIL');
  if (!same) bad++;
}

process.exit(bad ? 1 : 0);
