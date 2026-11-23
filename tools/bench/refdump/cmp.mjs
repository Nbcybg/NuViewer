// cmp.mjs <ours.txt> <ref.txt> [lines]: poses of NuViewer against spine-core 4.1.55 (what BD2-L2D-Viewer runs).
// A sample is "wrong" when what is DRAWN differs: a slot's vertex box moved by more than 3 units (weights, deform and bones included), an
// attachment / colour / draw order differs, or a slot is drawn by one side only. Bones are listed too (tolerance 0.5 units).
import fs from 'node:fs';
function load(p) {
  const m = new Map();
  let cur = null;
  for (const line of fs.readFileSync(p, 'utf8').split('\n')) {
    if (!line) continue;
    const c = line.split('\t');
    if (c[0] === '#') m.skin = c[1];
    else if (c[0] === '@') { cur = { b: [], s: [], o: '', v: new Map() }; m.set(c[1] + '@' + c[2], cur); }
    else if (c[0] === 'B') cur.b.push(c.slice(2).map(Number));
    else if (c[0] === 'S') cur.s.push([c[2], ...c.slice(3).map(Number)]);
    else if (c[0] === 'O') cur.o = c[1];
    else if (c[0] === 'V') cur.v.set(c[1], c.slice(2).map(Number));
  }
  return m;
}
const ours = load(process.argv[2]);
const ref = load(process.argv[3]);
if (ours.skin !== ref.skin) console.log(`  SKIN differs: ours ${ours.skin} reference ${ref.skin}`);
let samples = 0, wrong = 0, missing = 0, boneOnly = 0;
const kinds = { geo: 0, att: 0, color: 0, order: 0 };
let geoWorst = 0, geoWhere = '', boneWorst = 0, boneWhere = '';
const lines = [];
for (const [k, r] of ref) {
  const o = ours.get(k);
  if (!o) { missing++; continue; }
  samples++;
  let bone = -1, bi = -1, att = 0, col = 0, ord = o.o !== r.o ? 1 : 0, geo = 0, gw = 0, gs = '';
  const n = Math.min(o.b.length, r.b.length);
  for (let i = 0; i < n; i++) {
    const d = Math.max(Math.abs(o.b[i][0] - r.b[i][0]), Math.abs(o.b[i][1] - r.b[i][1]));
    // the matrix too: scaled by 100 so a 0.01 difference in a rotation counts like one world unit
    const dm = 100 * Math.max(...[2, 3, 4, 5].map((j) => Math.abs(o.b[i][j] - r.b[i][j])));
    // float32 against double: allow a relative error of 1e-5 of the size of the numbers (some bones sit at 1e9 in the files themselves)
    const dd = Math.max(d, dm) - 2e-5 * Math.max(Math.abs(r.b[i][0]), Math.abs(r.b[i][1]));
    if (dd > bone) { bone = dd; bi = i; }
  }
  for (let i = 0; i < Math.min(o.s.length, r.s.length); i++) {
    if (o.s[i][0] !== r.s[i][0]) att++;
    else if ([1, 2, 3, 4].some((j) => Math.abs(o.s[i][j] - r.s[i][j]) > 0.02)) col++;
  }
  for (const [slot, rv] of r.v) {
    const ov = o.v.get(slot);
    if (!ov) { geo++; gw = 1e6; gs = 'missing#' + slot; continue; }
    const d = Math.max(...rv.map((x, j) => Math.abs(x - ov[j]))) - 2e-5 * Math.max(...rv.map(Math.abs));
    if (d > 3) geo++;
    if (d > gw) { gw = d; gs = 'slot#' + slot; }
  }
  for (const slot of o.v.keys()) if (!r.v.has(slot)) { geo++; gw = 1e6; gs = 'extra#' + slot; }
  if (geo) kinds.geo++;
  if (att) kinds.att++;
  if (col) kinds.color++;
  if (ord) kinds.order++;
  if (gw > geoWorst) { geoWorst = gw; geoWhere = k + ' ' + gs; }
  if (bone > boneWorst) { boneWorst = bone; boneWhere = k + ' bone#' + bi; }
  if (geo || att || col || ord) { wrong++; lines.push(`  ${k}: geometry ${geo} (worst ${gw.toFixed(1)} ${gs}) attachment ${att} colour ${col} order ${ord}`); }
  else if (bone > 0.5) boneOnly++;
}
for (const l of lines.slice(0, Number(process.argv[4] || 4))) console.log(l);
console.log(`DRAWN WRONG ${wrong}/${samples} (geometry ${kinds.geo}, attachment ${kinds.att}, colour ${kinds.color}, order ${kinds.order}); bone-only ${boneOnly}; missing ${missing}; geometry worst ${geoWorst.toFixed(1)} ${geoWhere}; bone worst ${boneWorst.toFixed(1)} ${boneWhere}`);
