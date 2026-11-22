//! The texture atlas next to a skeleton (`.atlas`): which rectangle of which page is which picture. Reads the format of Spine 3.8
//! (`xy:` / `size:` / `orig:` / `offset:` on indented lines) and 4.0 - 4.2 (`bounds:` / `offsets:` / `rotate:`), and tolerates both mixed.

use std::sync::Arc;

/// A decoded atlas page, straight RGBA8 as stored in the file (premultiplied when `Page::pma`).
#[derive(Clone, Debug)]
pub struct PageImage {
    pub width: u32,
    pub height: u32,
    pub rgba: Vec<u8>,
}

#[derive(Clone, Debug)]
pub struct Page {
    pub name: String,
    /// The size the atlas file says the page has (regions are written in these pixels; a scaled-down page image still works).
    pub width: u32,
    pub height: u32,
    /// The picture's colours are already multiplied by its alpha.
    pub pma: bool,
    pub image: Option<Arc<PageImage>>,
}

#[derive(Clone, Debug)]
pub struct Region {
    pub name: String,
    pub page: usize,
    pub index: i32,
    /// Where it sits on the page, as the file writes it: for a rotated region the *unrotated* size.
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
    /// 0 or 90.
    pub rotate: u16,
    /// Trimming: where the packed rectangle sat inside the original picture (from the left, from the BOTTOM) and the original size.
    pub off_x: f32,
    pub off_y: f32,
    pub orig_w: f32,
    pub orig_h: f32,
}

#[derive(Clone, Debug, Default)]
pub struct Atlas {
    pub pages: Vec<Page>,
    pub regions: Vec<Region>,
}

fn nums(s: &str) -> Vec<f32> {
    s.split(',').filter_map(|p| p.trim().parse::<f32>().ok()).collect()
}

impl Atlas {
    /// Parse the text. Page images are filled in later (`Page::image`).
    pub fn parse(text: &str) -> Result<Atlas, String> {
        let mut atlas = Atlas::default();
        let mut expect_page = true;
        let mut region_open = false;
        for raw in text.lines() {
            let line = raw.trim();
            if line.is_empty() {
                expect_page = true;
                region_open = false;
                continue;
            }
            if let Some((k, v)) = line.split_once(':') {
                let (k, v) = (k.trim(), v.trim());
                if !region_open {
                    let Some(p) = atlas.pages.last_mut() else { continue };
                    match k {
                        "size" => {
                            let n = nums(v);
                            if n.len() >= 2 {
                                p.width = n[0] as u32;
                                p.height = n[1] as u32;
                            }
                        }
                        "pma" => p.pma = v == "true",
                        _ => {}
                    }
                } else if let Some(r) = atlas.regions.last_mut() {
                    let n = nums(v);
                    match k {
                        "rotate" => r.rotate = if v == "true" { 90 } else { v.parse::<u16>().ok().filter(|d| matches!(d, 90 | 180 | 270)).unwrap_or(0) },
                        "xy" if n.len() >= 2 => {
                            r.x = n[0];
                            r.y = n[1];
                        }
                        "size" if n.len() >= 2 => {
                            r.w = n[0];
                            r.h = n[1];
                            if r.orig_w == 0.0 {
                                r.orig_w = n[0];
                                r.orig_h = n[1];
                            }
                        }
                        "bounds" if n.len() >= 4 => {
                            r.x = n[0];
                            r.y = n[1];
                            r.w = n[2];
                            r.h = n[3];
                            if r.orig_w == 0.0 {
                                r.orig_w = n[2];
                                r.orig_h = n[3];
                            }
                        }
                        "orig" if n.len() >= 2 => {
                            r.orig_w = n[0];
                            r.orig_h = n[1];
                        }
                        "offset" if n.len() >= 2 => {
                            r.off_x = n[0];
                            r.off_y = n[1];
                        }
                        "offsets" if n.len() >= 4 => {
                            r.off_x = n[0];
                            r.off_y = n[1];
                            r.orig_w = n[2];
                            r.orig_h = n[3];
                        }
                        "index" => r.index = v.parse().unwrap_or(-1),
                        _ => {}
                    }
                }
                continue;
            }
            if expect_page {
                atlas.pages.push(Page { name: line.to_string(), width: 0, height: 0, pma: false, image: None });
                expect_page = false;
                region_open = false;
            } else {
                if atlas.pages.is_empty() {
                    return Err("atlas: a region before any page".into());
                }
                atlas.regions.push(Region { name: line.to_string(), page: atlas.pages.len() - 1, index: -1, x: 0.0, y: 0.0, w: 0.0, h: 0.0, rotate: 0, off_x: 0.0, off_y: 0.0, orig_w: 0.0, orig_h: 0.0 });
                region_open = true;
            }
        }
        if atlas.pages.is_empty() {
            return Err("atlas: no page in this file".into());
        }
        Ok(atlas)
    }

    /// The region called `name` (the lowest index when several share it).
    pub fn find(&self, name: &str) -> Option<&Region> {
        self.regions.iter().filter(|r| r.name == name).min_by_key(|r| r.index.max(-1))
    }

    /// Size of the picture a page's regions are cut from, for the texture coordinates (the file's `size:`, else the image).
    pub fn page_size(&self, page: usize) -> (f32, f32) {
        let p = &self.pages[page];
        if p.width > 0 && p.height > 0 {
            (p.width as f32, p.height as f32)
        } else if let Some(i) = &p.image {
            (i.width as f32, i.height as f32)
        } else {
            (1.0, 1.0)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_old_and_the_new_format_give_the_same_regions() {
        let old = "\nskel.png\nsize: 256,128\nformat: RGBA8888\nfilter: Linear,Linear\nrepeat: none\nhead\n  rotate: false\n  xy: 2, 4\n  size: 30, 20\n  orig: 40, 30\n  offset: 5, 3\n  index: -1\narm\n  rotate: true\n  xy: 100, 10\n  size: 8, 16\n  orig: 8, 16\n  offset: 0, 0\n  index: -1\n";
        let new = "skel.png\nsize:256,128\nfilter:Linear,Linear\npma:true\nhead\nbounds:2,4,30,20\noffsets:5,3,40,30\narm\nbounds:100,10,8,16\nrotate:90\n";
        let (a, b) = (Atlas::parse(old).unwrap(), Atlas::parse(new).unwrap());
        assert_eq!((a.pages[0].width, a.pages[0].height, a.pages[0].pma), (256, 128, false));
        assert!(b.pages[0].pma);
        for (x, y) in a.regions.iter().zip(&b.regions) {
            assert_eq!((x.name.as_str(), x.x, x.y, x.w, x.h, x.rotate, x.off_x, x.off_y, x.orig_w, x.orig_h), (y.name.as_str(), y.x, y.y, y.w, y.h, y.rotate, y.off_x, y.off_y, y.orig_w, y.orig_h));
        }
        assert_eq!(a.find("head").unwrap().orig_w, 40.0);
        assert_eq!(a.find("arm").unwrap().rotate, 90);
        assert!(a.find("leg").is_none());
    }

    #[test]
    fn several_pages_and_junk() {
        let t = "a.png\nsize:64,64\nr1\nbounds:0,0,8,8\n\nb.png\nsize:32,32\nr2\nbounds:1,1,4,4\n";
        let a = Atlas::parse(t).unwrap();
        assert_eq!(a.pages.len(), 2);
        assert_eq!(a.find("r2").unwrap().page, 1);
        assert!(Atlas::parse("").is_err());
    }
}
