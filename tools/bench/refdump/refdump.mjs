// refdump.mjs <skel> <out.txt> [fracs]: pose of every animation with spine-core 4.1.55, tracks set up like BD2-L2D-Viewer
// (setSpineAnimation: mixN_* on track 1 over a looping idleN on track 0). Same format as `spinebench --batch`.
import * as sp from '@esotericsoftware/spine-core';
import fs from 'node:fs';
import path from 'node:path';

class Tex extends sp.Texture {
  setFilters() {}
  setWraps() {}
  dispose() {}
}

const [skelPath, outPath, fr] = process.argv.slice(2);
const fracs = (fr || '0.137,0.613').split(',').map(Number);
const atlasPath = skelPath.replace(/\.skel$/, '.atlas');
const atlas = new sp.TextureAtlas(fs.readFileSync(atlasPath, 'utf8'));
atlas.pages.forEach((p) => p.setTexture(new Tex({ width: p.width, height: p.height })));
const sd = new sp.SkeletonBinary(new sp.AtlasAttachmentLoader(atlas)).readSkeletonData(new Uint8Array(fs.readFileSync(skelPath)));
const names = new Set(sd.animations.map((a) => a.name));
// the order of animations: ours sorts by name in the loader? write by the sorted name, the compare script matches by name
const out = [];
// BD2-L2D-Viewer: the first skin (in order) whose bounds are usable (see SpineViewer.vue computeCameraBounds / isBoundsReasonable)
const isBg = (n) => { n = n.toLowerCase(); return n.startsWith('bg') || n.startsWith('bk') || n.includes(' bg') || n.includes('_bg') || n.includes('background'); };
function bd2Bounds(sk) {
  const off = new sp.Vector2(), size = new sp.Vector2();
  sk.getBounds(off, size, []);
  let minX = Infinity, minY = Infinity, maxX = -Infinity, maxY = -Infinity, found = false;
  for (const slot of sk.drawOrder) {
    if (isBg(slot.data.name)) continue;
    const att = slot.getAttachment();
    if (!att || typeof att.computeWorldVertices !== 'function') continue;
    const n = att.worldVerticesLength || 0;
    if (!n) continue;
    const wv = new Float32Array(n);
    try { att.computeWorldVertices(slot, 0, n, wv, 0, 2); } catch { continue; }
    for (let i = 0; i < n; i += 2) {
      if (!Number.isFinite(wv[i]) || !Number.isFinite(wv[i + 1])) continue;
      minX = Math.min(minX, wv[i]); maxX = Math.max(maxX, wv[i]); minY = Math.min(minY, wv[i + 1]); maxY = Math.max(maxY, wv[i + 1]); found = true;
    }
  }
  if (found && maxX - minX > 0 && maxY - minY > 0) {
    const tw = maxX - minX + 100, th = maxY - minY + 100;
    if (size.x / Math.max(tw, 1) > 10 || size.y / Math.max(th, 1) > 10) return { w: tw, h: th };
  }
  return { w: size.x, h: size.y };
}
const reasonable = (b) => Number.isFinite(b.w) && Number.isFinite(b.h) && b.w > 0 && b.h > 0 && b.w < 50000 && b.h < 50000;
let skinIndex = 0;
{
  const sk = new sp.Skeleton(sd);
  for (let i = 0; i < sd.skins.length; i++) {
    sk.setSkinByName(sd.skins[i].name); sk.setSlotsToSetupPose(); sk.updateWorldTransform();
    if (reasonable(bd2Bounds(sk))) { skinIndex = i; break; }
  }
}
out.push(`#	${skinIndex}`);
// the name a slot's attachment has in the skin (the key), not the name inside the attachment: files may differ in the two
const keyOf = (sk, slot) => {
  const att = slot.attachment;
  if (!att) return '-';
  for (const skin of [sk.skin, sd.defaultSkin]) {
    const m = skin && skin.attachments[slot.data.index];
    if (m) for (const k of Object.keys(m)) if (m[k] === att) return k;
  }
  return att.name;
};
const f = (v, n = 3) => (Math.abs(v) < 5e-4 ? 0 : v).toFixed(n);
for (const an of sd.animations) {
  for (const fk of fracs) {
    const t = an.duration * fk;
    const sk = new sp.Skeleton(sd);
    const data = new sp.AnimationStateData(sd);
    const state = new sp.AnimationState(data);
    sk.setSkinByName(sd.skins[skinIndex].name);
    sk.setToSetupPose();
    sk.updateWorldTransform();
    const m = /^mix(\d+)/i.exec(an.name);
    const idle = m ? `idle${m[1]}` : null;
    if (m && names.has(idle)) {
      const e0 = state.setAnimation(0, idle, true);
      const e1 = state.setAnimation(1, an.name, true);
      e0.trackEnd = Number.MAX_VALUE;
      for (const e of [e0, e1]) { e.mixDuration = 0; e.mixTime = 0; }
    } else {
      const e = state.setAnimation(0, an.name, true);
      e.mixDuration = 0; e.mixTime = 0;
    }
    state.update(t);
    state.apply(sk);
    sk.updateWorldTransform();
    out.push(`@\t${an.name}\t${t.toFixed(4)}`);
    sk.bones.forEach((b, i) => out.push(`B\t${i}\t${f(b.worldX)}\t${f(b.worldY)}\t${f(b.a, 4)}\t${f(b.b, 4)}\t${f(b.c, 4)}\t${f(b.d, 4)}`));
    sk.slots.forEach((s, i) => { const c = s.color; out.push(`S\t${i}\t${keyOf(sk, s)}\t${f(c.r)}\t${f(c.g)}\t${f(c.b)}\t${f(c.a)}`); });
    // the box of the vertices each slot draws (weights and deform included): what the picture is made of
    for (const slot of sk.drawOrder) {
      const att = slot.getAttachment();
      if (!att || typeof att.computeWorldVertices !== 'function' || slot.color.a <= 0.01) continue;
      if (att instanceof sp.ClippingAttachment || att instanceof sp.BoundingBoxAttachment || att instanceof sp.PathAttachment || att instanceof sp.PointAttachment) continue;
      let wv;
      if (att instanceof sp.RegionAttachment) { wv = new Float32Array(8); att.computeWorldVertices(slot, wv, 0, 2); }
      else { wv = new Float32Array(att.worldVerticesLength); att.computeWorldVertices(slot, 0, att.worldVerticesLength, wv, 0, 2); }
      let x0 = Infinity, y0 = Infinity, x1 = -Infinity, y1 = -Infinity;
      for (let k = 0; k < wv.length; k += 2) { x0 = Math.min(x0, wv[k]); x1 = Math.max(x1, wv[k]); y0 = Math.min(y0, wv[k + 1]); y1 = Math.max(y1, wv[k + 1]); }
      out.push(`V\t${slot.data.index}\t${x0.toFixed(2)}\t${y0.toFixed(2)}\t${x1.toFixed(2)}\t${y1.toFixed(2)}`);
    }
    out.push(`O\t${sk.drawOrder.map((s) => s.data.index).join(',')}`);
  }
}
fs.writeFileSync(outPath, out.join('\n') + '\n');
console.log(path.basename(skelPath), sd.bones.length, 'bones', sd.animations.length, 'animations');
