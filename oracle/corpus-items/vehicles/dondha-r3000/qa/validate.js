// Dondha R3000 - Mall hard-rule validation and VRML97 parse diagnostics.
//
//   node qa/validate.js                 # every variant that exists on disk
//   node qa/validate.js dondha-r3000-rsi
//
// Both halves come from WRLForge and neither is re-implemented here:
// `validator.js` is the Mall rule set, `src/vrml` the parser that reports
// duplicate DEF, unresolved USE and dangling ROUTE. AGENTS.md: do not write
// another regex validator.
//
// The gzip figure `validator.js` reports is its own re-gzip of the text. That
// is the right number for the readable source, and it is NOT the uploaded
// artifact - the shipped `.wrl` is already gzip on disk. The uploaded bytes are
// printed separately from `stat`, which is the figure the Mall form sees.
const fs = require('fs');
const path = require('path');
const zlib = require('zlib');

const FORGE = '/home/ryan/Projects/cybertown/wrlforge';
const { validate } = require(path.join(FORGE, 'validator.js'));

const ITEM = path.join(__dirname, '..');
const STEMS = ['dondha-r3000', 'dondha-r3000-rsi', 'dondha-r3000-dx'];

function parseDiagnostics(text) {
  // WRLForge's parser: duplicate DEF, unresolved USE, dangling ROUTE.
  const p = require(path.join(FORGE, 'src', 'vrml')).parse(text);
  // `defsByName` is a Map and each USE carries its own `resolved` flag, so ask
  // the parser rather than re-deriving the lookup.
  const unresolved = p.uses.filter(u => u.resolved === false);
  const dangling = p.routes.filter(r => r.resolved === false);
  return {
    diagnostics: p.diagnostics.length,
    syntax: p.syntaxDiagnostics.length,
    semantic: p.semanticDiagnostics.length,
    duplicateDefs: p.duplicateDefs.length,
    defs: p.defs.length,
    uses: p.uses.length,
    unresolvedUses: unresolved.length,
    routes: p.routes.length,
    danglingRoutes: dangling.length,
    truncated: !!p.truncated,
    assetRefs: p.assetRefs.length,
    detail: p.diagnostics.slice(0, 5),
  };
}

let bad = 0;
const explicit = !!process.argv[2];
for (const stem of (explicit ? [process.argv[2]] : STEMS)) {
  const edit = path.join(ITEM, stem + '.edit.wrl');
  const up = path.join(ITEM, stem + '.wrl');
  // Skipping a variant that has not been built yet is fine; silently skipping
  // one that was ASKED for is a gate that cannot fail.
  if (!fs.existsSync(edit) || !fs.existsSync(up)) {
    if (!explicit) continue;
    console.error('no such artifact pair: ' + stem);
    process.exit(2);
  }
  const text = fs.readFileSync(edit, 'utf8');

  console.log('=== ' + stem + ' ===');
  const { results, ok, gzipBytes, rawBytes } = validate(text);
  for (const r of results) {
    if (!r.pass && r.severity === 'hard') bad++;
    console.log('  %s %s %s', r.pass ? 'PASS' : 'FAIL', r.name.padEnd(46),
                r.detail);
  }
  // The artifact the Mall actually receives.
  if (!ok) bad++;
  console.log('  raw .edit.wrl bytes %d, re-gzip of that text %d',
              rawBytes, gzipBytes);
  const uploaded = fs.statSync(up).size;
  const roundtrip = zlib.gunzipSync(fs.readFileSync(up)).toString('utf8');
  console.log('  %s uploaded .wrl bytes                          %d',
              uploaded < 80 * 1024 ? 'PASS' : 'FAIL', uploaded);
  console.log('  %s gzip round-trips to the .edit.wrl text',
              roundtrip === text ? 'PASS' : 'FAIL');
  if (uploaded >= 80 * 1024 || roundtrip !== text) bad++;

  const p = parseDiagnostics(text);
  console.log('  parse:', JSON.stringify(p));
  const clean = p.diagnostics === 0 && p.duplicateDefs === 0 &&
                p.unresolvedUses === 0 && p.danglingRoutes === 0 &&
                p.assetRefs === 0 && !p.truncated;
  console.log('  %s parser clean, and zero external asset refs',
              clean ? 'PASS' : 'FAIL');
  if (!clean) bad++;
}
process.exit(bad ? 1 : 0);
