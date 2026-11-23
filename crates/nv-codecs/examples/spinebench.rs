//! `spinebench <skeleton> [--png prefix]`: time the Spine rasteriser at several canvas sizes (1x and 2x supersampling) and save a
//! picture of each, so sharpness can be compared by eye. The frame is the first animation at a quarter of its length.
fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let Some(path) = args.first() else {
        eprintln!("usage: spinebench <skeleton> [--png prefix]");
        return;
    };
    let prefix = args.iter().position(|a| a == "--png").and_then(|i| args.get(i + 1)).cloned();
    let rig = nv_codecs::spinefmt::load(std::path::Path::new(path)).expect("load");
    if args.iter().any(|a| a == "--constraints") {
        println!("{} bones, ik {}, transform {}, path {}, physics {}", rig.bones.len(), rig.ik.len(), rig.transform.len(), rig.path.len(), rig.physics.len());
        for c in &rig.ik {
            println!("ik {:<28} order {} bones {:?} target {} mix {} soft {} bend+ {} compress {} stretch {} uniform {} skinreq {}", c.name, c.order, c.bones.iter().map(|b| rig.bones[*b].name.clone()).collect::<Vec<_>>(), rig.bones[c.target].name, c.mix, c.softness, c.bend_positive, c.compress, c.stretch, c.uniform, c.skin_required);
        }
        for c in rig.transform.iter().take(30) {
            println!("tf {:<28} order {} bones {} target {}", c.name, c.order, c.bones.len(), rig.bones[c.target].name);
        }
        for name in ["idle7", "mix7_22_4"] {
            let a = rig.animations.iter().find(|a| a.name == name).unwrap();
            println!("{name}: bones {} slots {} ik {} transform {} deform {} path {}", a.bones.len(), a.slots.len(), a.ik.len(), a.transform.len(), a.deform.len(), a.path.len());
            for (ci, _) in &a.ik {
                println!("   ik timeline for {}", rig.ik[*ci].name);
            }
        }
        return;
    }
    if let Some(out) = args.iter().position(|a| a == "--bones").and_then(|i| args.get(i + 1)) {
        let tt = args.iter().position(|a| a == "--t").and_then(|i| args.get(i + 1)).and_then(|v| v.parse::<f32>().ok()).unwrap_or(0.0);
        let a = args.iter().position(|a| a == "--anim").and_then(|i| args.get(i + 1)).and_then(|n| rig.animations.iter().position(|x| &x.name == n));
        let pose = rig.pose(0, a, tt);
        let mut s = String::new();
        if let Ok(names) = std::env::var("NV_PARENTS") {
            for n in names.split(',') {
                let mut i = rig.bones.iter().position(|b| b.name == n);
                while let Some(k) = i {
                    let b = &rig.bones[k];
                    let p = &pose.bones[k];
                    eprintln!("{} (#{k}) parent {:?} local x {:.2} y {:.2} rot {:.2} s {:.3},{:.3} | applied x {:.2} y {:.2} rot {:.2} s {:.3},{:.3} inherit {:?}", b.name, b.parent.map(|q| rig.bones[q].name.clone()), b.x, b.y, b.rotation, b.scale_x, b.scale_y, p.x, p.y, p.rotation, p.scale_x, p.scale_y, b.inherit);
                    i = b.parent;
                }
            }
        }
        for (i, b) in rig.bones.iter().enumerate() {
            let p = &pose.bones[i];
            s.push_str(&format!("{}	{}	{}	{}	{}	{}	{}
", b.name, p.wx, p.wy, p.a, p.b, p.c, p.d));
        }
        std::fs::write(out, s).unwrap();
        return;
    }
    if args.iter().any(|a| a == "--list") {
        println!("{} skins, {} slots, {} animations", rig.skins.len(), rig.slots.len(), rig.animations.len());
        for (i, a) in rig.animations.iter().enumerate() {
            println!("  {i}: {} ({:.2}s)", a.name, a.duration);
        }
        return;
    }
    let want = args.iter().position(|a| a == "--anim").and_then(|i| args.get(i + 1)).cloned();
    let setup = args.iter().any(|a| a == "--setup");
    let anim = if setup { None } else { match &want {
        Some(n) => Some(rig.animations.iter().position(|a| &a.name == n).expect("no such animation")),
        None => if rig.animations.is_empty() { None } else { Some(0) },
    } };
    let t = args.iter().position(|a| a == "--t").and_then(|i| args.get(i + 1)).and_then(|v| v.parse::<f32>().ok()).unwrap_or_else(|| anim.map(|a| rig.animations[a].duration * 0.25).unwrap_or(0.0));
    // --box x,y,w,h: draw exactly this part of the world (like a camera) into a canvas of --size WxH
    let boxed: Option<Vec<f32>> = args.iter().position(|a| a == "--box").and_then(|i| args.get(i + 1)).map(|v| v.split(',').filter_map(|n| n.parse().ok()).collect());
    if let Some(b) = boxed.filter(|b| b.len() == 4) {
        let size: Vec<usize> = args.iter().position(|a| a == "--size").and_then(|i| args.get(i + 1)).map(|v| v.split('x').filter_map(|n| n.parse().ok()).collect()).unwrap_or_else(|| vec![1170, 934]);
        let (w, h) = (size[0], size[1]);
        let view = nv_anim::View::fit([b[0], b[1], b[0] + b[2], b[1] + b[3]], w, h, 0.0);
        let pose = rig.pose(0, anim, t);
        let rgba = rig.draw(&pose, w, h, view, 2, Some([0.125, 0.125, 0.125]));
        let f = std::fs::File::create(prefix.clone().unwrap_or("box".into()) + "_box.png").unwrap();
        let mut enc = png::Encoder::new(std::io::BufWriter::new(f), w as u32, h as u32);
        enc.set_color(png::ColorType::Rgba);
        enc.set_depth(png::BitDepth::Eight);
        enc.write_header().unwrap().write_image_data(&rgba).unwrap();
        return;
    }
    if args.iter().any(|a| a == "--items") {
        let pose = rig.pose(0, anim, t);
        let items = rig.geometry(&pose);
        println!("bounds_of: {:?}", rig.bounds_of(&pose));
        println!("camera_bounds: {:?}", rig.camera_bounds(0));
        for it in &items {
            let (mut x0, mut y0, mut x1, mut y1) = (f32::MAX, f32::MAX, f32::MIN, f32::MIN);
            for v in &it.verts {
                x0 = x0.min(v[0]);
                y0 = y0.min(v[1]);
                x1 = x1.max(v[0]);
                y1 = y1.max(v[1]);
            }
            println!("{:>4} {:<28} {:?} pma={} page={} col=[{:.2},{:.2},{:.2},{:.2}] dark={:?} clip={} box=[{:.0},{:.0},{:.0},{:.0}] tris={}", it.slot, rig.slots[it.slot].name, it.blend, it.pma, it.page, it.color[0], it.color[1], it.color[2], it.color[3], it.dark, it.clip.is_some(), x0, y0, x1, y1, it.tris.len() / 3);
        }
        return;
    }
    {
        let pose = rig.pose(0, anim, t);
        let t0 = std::time::Instant::now();
        let items = rig.geometry(&pose);
        let tris: usize = items.iter().map(|i| i.tris.len() / 3).sum();
        println!("pose + geometry only: {:.2} ms ({} items, {} triangles)", t0.elapsed().as_secs_f64() * 1000.0, items.len(), tris);
        let t0 = std::time::Instant::now();
        let _ = rig.pose(0, anim, t);
        println!("pose only: {:.2} ms", t0.elapsed().as_secs_f64() * 1000.0);
    }
    for max in [1024usize, 1920, 2560, 3840] {
        for ss in [1usize, 2] {
            let (w, h, view) = nv_codecs::spinefmt::canvas_for(&rig, 0, anim, max);
            let pose = rig.pose(0, anim, t);
            let t0 = std::time::Instant::now();
            let n = 5;
            let mut rgba = Vec::new();
            for _ in 0..n {
                rgba = rig.draw(&pose, w, h, view, ss, None);
            }
            let ms = t0.elapsed().as_secs_f64() * 1000.0 / n as f64;
            println!("canvas {w}x{h} ssaa {ss}: {ms:.1} ms / frame");
            if let Some(p) = &prefix {
                let f = std::fs::File::create(format!("{p}_{max}_ss{ss}.png")).unwrap();
                let mut enc = png::Encoder::new(std::io::BufWriter::new(f), w as u32, h as u32);
                enc.set_color(png::ColorType::Rgba);
                enc.set_depth(png::BitDepth::Eight);
                enc.write_header().unwrap().write_image_data(&rgba).unwrap();
            }
        }
    }
}
