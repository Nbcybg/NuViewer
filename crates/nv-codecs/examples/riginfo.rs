//! `riginfo <skeleton.skel|json> [--png out.png] [--anim name] [--t seconds]`: load a Spine skeleton with its atlas and say what is in it; optionally draw a frame.
fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let Some(path) = args.first() else {
        eprintln!("usage: riginfo <skeleton> [--png out.png] [--anim name] [--t seconds]");
        return;
    };
    let t0 = std::time::Instant::now();
    let rig = match nv_codecs::spinefmt::load(std::path::Path::new(path)) {
        Ok(r) => r,
        Err(e) => {
            println!("FAILED: {e}");
            return;
        }
    };
    println!("Spine {}: {} bones, {} slots, {} skins, {} ik, {} transform, {} animations; read in {:.0} ms", rig.version, rig.bones.len(), rig.slots.len(), rig.skins.len(), rig.ik.len(), rig.transform.len(), rig.animations.len(), t0.elapsed().as_secs_f64() * 1000.0);
    println!("box {} {} {} {}  fps {}  unsupported {:?}", rig.x, rig.y, rig.width, rig.height, rig.fps, rig.unsupported);
    for (i, a) in rig.animations.iter().enumerate().take(12) {
        println!("  anim {i}: {} {:.2}s bones {} slots {} deform {} ik {} tf {} order {}", a.name, a.duration, a.bones.len(), a.slots.len(), a.deform.len(), a.ik.len(), a.transform.len(), a.draw_order.len());
    }
    for (i, s) in rig.skins.iter().enumerate().take(8) {
        println!("  skin {i}: {} ({} slot maps)", s.name, s.slots.iter().filter(|m| !m.is_empty()).count());
    }
    if args.iter().any(|a| a == "--all") {
        let t = std::time::Instant::now();
        let (mut bad, mut worst) = (0usize, 0.0f64);
        for a in 0..rig.animations.len() {
            for k in 0..4 {
                let tt = rig.animations[a].duration * k as f32 / 3.0;
                let t1 = std::time::Instant::now();
                let items = rig.geometry(&rig.pose(0, Some(a), tt));
                worst = worst.max(t1.elapsed().as_secs_f64() * 1000.0);
                if items.iter().any(|i| i.verts.iter().any(|v| !v[0].is_finite() || !v[1].is_finite() || v[0].abs() > 1.0e10 || v[1].abs() > 1.0e10)) {
                    bad += 1;
                    if bad <= 2 {
                        let pose = rig.pose(0, Some(a), tt);
                        for it in items.iter().filter(|i| i.verts.iter().any(|v| !v[0].is_finite() || !v[1].is_finite() || v[0].abs() > 1.0e10 || v[1].abs() > 1.0e10)).take(3) {
                            let s = &rig.slots[it.slot];
                            let bn = &pose.bones[s.bone];
                            for (bi, b) in pose.bones.iter().enumerate() {
                                if !b.a.is_finite() || b.a.abs() > 200.0 || b.b.abs() > 200.0 || b.c.abs() > 200.0 || b.d.abs() > 200.0 {
                                    let ik: Vec<&str> = rig.ik.iter().filter(|c| c.bones.contains(&bi)).map(|c| c.name.as_str()).collect();
                                    let tf: Vec<&str> = rig.transform.iter().filter(|c| c.bones.contains(&bi)).map(|c| c.name.as_str()).collect();
                                    for c in rig.ik.iter().filter(|c| c.bones.contains(&bi)) {
                                        println!("   ik {}: bones {:?} target {} ({}) mix {} soft {} bend {} compress {} stretch {} uniform {}; bone length {}", c.name, c.bones, c.target, rig.bones[c.target].name, c.mix, c.softness, c.bend_positive, c.compress, c.stretch, c.uniform, rig.bones[bi].length);
                                    }
                                    println!("   extreme bone {bi} {}: a={} b={} c={} d={} local x={} y={} rot={} sx={} sy={} shx={} shy={} ik={ik:?} tf={tf:?}", rig.bones[bi].name, b.a, b.b, b.c, b.d, b.x, b.y, b.rotation, b.scale_x, b.scale_y, b.shear_x, b.shear_y);
                                    break;
                                }
                            }
                            let st = &pose.slots[it.slot];
                            let bv = it.verts.iter().find(|v| !v[0].is_finite() || !v[1].is_finite() || v[0].abs() > 1.0e10 || v[1].abs() > 1.0e10);
                            println!("   anim {} t {tt:.2}: first bad vertex {:?}, slot att {:?}, deforms {:?}", rig.animations[a].name, bv, st.attachment, st.deform.iter().map(|d| (d.1.clone(), d.2.len(), d.2.iter().cloned().fold(0.0f32, |m, x| m.max(x.abs())))).collect::<Vec<_>>());
                            println!("   bad item: slot {} bone {} ({}) a={} b={} c={} d={} wx={} wy={} local rot={} sx={} sy={}", s.name, s.bone, rig.bones[s.bone].name, bn.a, bn.b, bn.c, bn.d, bn.wx, bn.wy, bn.rotation, bn.scale_x, bn.scale_y);
                        }
                    }
                    println!("  bad geometry: {} at {tt:.2}", rig.animations[a].name);
                }
            }
        }
        println!("all {} animations at 4 times each: {} with bad geometry, {:.0} ms in all, worst pose {worst:.1} ms", rig.animations.len(), bad, t.elapsed().as_secs_f64() * 1000.0);
    }
    if args.iter().any(|a| a == "--constraints") {
        for c in rig.transform.iter().take(6) {
            println!("  tf {} order {} bones {:?} target {} rot {} x {} y {} sx {} sy {} shy {} mix r {} x {} y {} sx {} sy {} shy {} local {} rel {}", c.name, c.order, c.bones, c.target, c.rotation, c.x, c.y, c.scale_x, c.scale_y, c.shear_y, c.mix_rotate, c.mix_x, c.mix_y, c.mix_scale_x, c.mix_scale_y, c.mix_shear_y, c.local, c.relative);
        }
        for c in rig.ik.iter().take(6) {
            println!("  ik {} order {} bones {:?} target {} mix {} compress {} stretch {} bend {}", c.name, c.order, c.bones, c.target, c.mix, c.compress, c.stretch, c.bend_positive);
        }
        let setup = rig.pose(0, None, 0.0);
        for c in rig.ik.iter().filter(|c| c.name == "bone675") {
            let (b, t) = (&setup.bones[c.bones[0]], &setup.bones[c.target]);
            println!("  setup: ik bone {} at ({:.1},{:.1}) parent {:?}; target {} at ({:.1},{:.1}) parent {:?}", c.bones[0], b.wx, b.wy, rig.bones[c.bones[0]].parent, c.target, t.wx, t.wy, rig.bones[c.target].parent);
        }
        let orders: Vec<i32> = rig.ik.iter().map(|c| c.order).chain(rig.transform.iter().map(|c| c.order)).collect();
        let (mn, mx) = (orders.iter().min().unwrap(), orders.iter().max().unwrap());
        let mut seen = std::collections::HashSet::new();
        println!("  constraint orders {mn}..{mx}, {} distinct of {}", orders.iter().filter(|o| seen.insert(**o)).count(), orders.len());
    }
    if let Some(bi) = args.iter().position(|a| a == "--bone").and_then(|i| args.get(i + 1)).and_then(|v| v.parse::<usize>().ok()) {
        let an = args.iter().position(|a| a == "--anim").and_then(|i| args.get(i + 1)).and_then(|n| rig.animation_index(n));
        let tt: f32 = args.iter().position(|a| a == "--t").and_then(|i| args.get(i + 1)).and_then(|v| v.parse().ok()).unwrap_or(0.0);
        let pose = rig.pose(0, an, tt);
        let mut chain = Some(bi);
        while let Some(c) = chain {
            let b = &pose.bones[c];
            let d = &rig.bones[c];
            println!("  bone {c} {}: local x {} y {} rot {} sx {} sy {} (setup x {} y {} rot {} sx {}) world ({:.1},{:.1}) a={} inherit {:?}", d.name, b.x, b.y, b.rotation, b.scale_x, b.scale_y, d.x, d.y, d.rotation, d.scale_x, b.wx, b.wy, b.a, d.inherit);
            chain = d.parent;
        }
        if let Some(a) = an {
            for (b, tl) in &rig.animations[a].bones {
                if *b == bi {
                    let (kind, tr) = match tl {
                        nv_anim::BoneTl::Rotate(t) => ("rotate", t),
                        nv_anim::BoneTl::Translate(t) => ("translate", t),
                        nv_anim::BoneTl::TranslateX(t) => ("translateX", t),
                        nv_anim::BoneTl::TranslateY(t) => ("translateY", t),
                        nv_anim::BoneTl::Scale(t) => ("scale", t),
                        nv_anim::BoneTl::ScaleX(t) => ("scaleX", t),
                        nv_anim::BoneTl::ScaleY(t) => ("scaleY", t),
                        nv_anim::BoneTl::Shear(t) => ("shear", t),
                        nv_anim::BoneTl::ShearX(t) => ("shearX", t),
                        nv_anim::BoneTl::ShearY(t) => ("shearY", t),
                    };
                    println!("  timeline {kind}: {} keys, times {:?}, first values {:?}, sample at t {:?}", tr.len(), &tr.times[..tr.times.len().min(4)], &tr.values[..tr.values.len().min(8)], tr.sample(tt, false));
                }
            }
            for c in rig.transform.iter().filter(|c| c.name == "idle15513") {
                println!("  tf idle15513: bones {:?} ({}) target {} ({}) rot {} x {} y {} sx {} sy {} shy {} mix r {} x {} y {} sx {} sy {} shy {} local {} rel {}", c.bones, rig.bones[c.bones[0]].name, c.target, rig.bones[c.target].name, c.rotation, c.x, c.y, c.scale_x, c.scale_y, c.shear_y, c.mix_rotate, c.mix_x, c.mix_y, c.mix_scale_x, c.mix_scale_y, c.mix_shear_y, c.local, c.relative);
                let ci = rig.transform.iter().position(|x| x.name == "idle15513").unwrap();
                for (i, tr) in &rig.animations[a].transform {
                    if *i == ci {
                        println!("  animated mixes at t: {:?}", tr.sample(tt, false));
                    }
                }
            }
            println!("  constraints on it: ik {:?} tf {:?}", rig.ik.iter().filter(|c| c.bones.contains(&bi) || c.target == bi).map(|c| (c.name.clone(), c.order)).collect::<Vec<_>>(), rig.transform.iter().filter(|c| c.bones.contains(&bi) || c.target == bi).map(|c| (c.name.clone(), c.order)).collect::<Vec<_>>());
        }
    }
    let ai = args.iter().position(|a| a == "--anim").and_then(|i| args.get(i + 1)).and_then(|n| rig.animation_index(n)).or(if rig.animations.is_empty() { None } else { Some(0) });
    let t: f32 = args.iter().position(|a| a == "--t").and_then(|i| args.get(i + 1)).and_then(|v| v.parse().ok()).unwrap_or(0.0);
    let (w, h, view) = nv_codecs::spinefmt::canvas_for(&rig, 0, ai, 1024);
    let t1 = std::time::Instant::now();
    let pose = rig.pose(0, ai, t);
    let items = rig.geometry(&pose);
    let tp = t1.elapsed().as_secs_f64() * 1000.0;
    let mut big: Vec<(f32, String, [f32; 4], usize, [f32; 4])> = items.iter().map(|it| {
        let (mut x0, mut y0, mut x1, mut y1) = (f32::MAX, f32::MAX, f32::MIN, f32::MIN);
        for v in &it.verts {
            x0 = x0.min(v[0]);
            y0 = y0.min(v[1]);
            x1 = x1.max(v[0]);
            y1 = y1.max(v[1]);
        }
        ((x1 - x0) * (y1 - y0), rig.slots[it.slot].name.clone(), [x0, y0, x1, y1], it.page, it.color)
    }).collect();
    big.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap());
    for b in big.iter().take(6) {
        println!("  item area {:.0} slot {} box {:?} page {} colour {:?}", b.0, b.1, b.2, b.3, b.4);
    }
    println!("  median area {:.0}", big[big.len() / 2].0);
    let t2 = std::time::Instant::now();
    let rgba = nv_anim::render(&rig, &items, w, h, view, 1, None);
    println!("canvas {w}x{h}: pose + geometry {tp:.1} ms ({} items), raster {:.1} ms", items.len(), t2.elapsed().as_secs_f64() * 1000.0);
    if let Some(i) = args.iter().position(|a| a == "--png") {
        if let Some(out) = args.get(i + 1) {
            let f = std::fs::File::create(out).unwrap();
            let mut enc = png::Encoder::new(std::io::BufWriter::new(f), w as u32, h as u32);
            enc.set_color(png::ColorType::Rgba);
            enc.set_depth(png::BitDepth::Eight);
            enc.write_header().unwrap().write_image_data(&rgba).unwrap();
            println!("wrote {out}");
        }
    }
}
