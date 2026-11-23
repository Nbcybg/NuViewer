// first.mjs <skel> <ours.txt> <ref.txt> <anim> [n]: the first n bones (file order = parents first) where ours and spine-core differ, with names
import * as sp from '@esotericsoftware/spine-core';
import fs from 'node:fs';
class Tex extends sp.Texture { setFilters() {} setWraps() {} dispose() {} }
const [skelPath, oursPath, refPath, anim, nStr] = process.argv.slice(2);
const atlas = new sp.TextureAtlas(fs.readFileSync(skelPath.replace(/\.skel$/, '.atlas'), 'utf8'));
atlas.pages.forEach((p) => p.setTexture(new Tex({ width: p.width, height: p.height })));
const sd = new sp.SkeletonBinary(new sp.AtlasAttachmentLoader(atlas)).readSkeletonData(new Uint8Array(fs.readFileSync(skelPath)));
function pick(p) {
  const rows = [];
  let on = false;
  for (const line of fs.readFileSync(p, 'utf8').split('\n')) {
    const c = line.split('\t');
    if (c[0] === '@') { if (on) break; on = c[1] === anim; }
    else if (on && c[0] === 'B') rows.push(c.slice(2).map(Number));
  }
  return rows;
}
const o = pick(oursPath), r = pick(refPath);
let shown = 0;
for (let i = 0; i < r.length && shown < Number(nStr || 6); i++) {
  const d = Math.max(Math.abs(o[i][0] - r[i][0]), Math.abs(o[i][1] - r[i][1]), 100 * Math.max(...[2, 3, 4, 5].map((j) => Math.abs(o[i][j] - r[i][j]))));
  const mag = Math.max(Math.abs(r[i][0]), Math.abs(r[i][1]));
  if (d - 2e-5 * mag <= Number(process.argv[7] || 0.5)) continue;
  const b = sd.bones[i];
  console.log(`#${i} ${b.name} (parent ${b.parent ? b.parent.index + ' ' + b.parent.name : '-'}) mode ${b.transformMode} rot ${b.rotation.toFixed(2)} sx ${b.scaleX.toFixed(3)} sy ${b.scaleY.toFixed(3)} shx ${b.shearX.toFixed(2)} shy ${b.shearY.toFixed(2)}`);
  console.log('   ours', o[i].map((v) => v.toFixed(3)).join(' '));
  console.log('   ref ', r[i].map((v) => v.toFixed(3)).join(' '));
  shown++;
}
