// inspect.mjs <skel> <bone name>: constraints that touch the bone (or its parents), and the animation timelines on it
import * as sp from '@esotericsoftware/spine-core';
import fs from 'node:fs';
class Tex extends sp.Texture { setFilters() {} setWraps() {} dispose() {} }
const [skelPath, boneName] = process.argv.slice(2);
const atlas = new sp.TextureAtlas(fs.readFileSync(skelPath.replace(/\.skel$/, '.atlas'), 'utf8'));
atlas.pages.forEach((p) => p.setTexture(new Tex({ width: p.width, height: p.height })));
const sd = new sp.SkeletonBinary(new sp.AtlasAttachmentLoader(atlas)).readSkeletonData(new Uint8Array(fs.readFileSync(skelPath)));
const b = sd.findBone(boneName);
const chain = new Set();
for (let x = b; x; x = x.parent) chain.add(x.name);
console.log('bone', b.index, b.name, 'mode', b.transformMode, 'rot', b.rotation);
const touches = (arr) => arr.some((x) => chain.has(x.name));
for (const c of sd.ikConstraints) if (touches(c.bones) || chain.has(c.target.name)) console.log('IK', c.name, 'order', c.order, 'bones', c.bones.map((x) => x.name), 'target', c.target.name, 'mix', c.mix, 'soft', c.softness, 'bend', c.bendDirection, 'comp', c.compress, 'stretch', c.stretch, 'uniform', c.uniform);
for (const c of sd.transformConstraints) if (touches(c.bones) || chain.has(c.target.name)) console.log('TF', c.name, 'order', c.order, 'bones', c.bones.map((x) => x.name), 'target', c.target.name, 'mixRotate', c.mixRotate, 'mixX', c.mixX, 'mixY', c.mixY, 'mixScaleX', c.mixScaleX, 'offsets', c.offsetRotation, c.offsetX, c.offsetY, 'local', c.local, 'relative', c.relative);
for (const c of sd.pathConstraints) if (touches(c.bones) || chain.has(c.target.name)) console.log('PATH', c.name, c.order);
