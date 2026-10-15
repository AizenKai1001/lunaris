//! TrueType and OpenType text for the interface. A font file of the workspace is read once; each glyph is drawn at the pixel size it is asked for
//! (whole pixels from a fixed origin, so a letter always looks the same wherever it is), kept in an atlas page, and handed out as small textured
//! quads. The reference renderer and the GPU renderer draw those quads alike, so text comes out the same in both.
//!
//! The pages are white with the glyph's coverage as alpha, 512 x 512 texels each; a page is a new `Texture` whenever a glyph was added to it, and
//! quads keep the page they were made from.
use crate::texture::Texture;
use ab_glyph::{point, Font as _, FontVec, PxScale, ScaleFont as _};
use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::Arc;

pub const PAGE: u32 = 512;
/// The largest font file read (bytes).
const MAX_FONT_BYTES: u64 = 32 * 1024 * 1024;

/// One drawn letter, ready to be a quad: `x`, `y` are where its top left corner lies from the start of the line's baseline (y grows down, so it
/// is negative for most letters), `w` x `h` its size in pixels, and `uv` the part of the page it is (u0, v0, u1, v1, v from the top).
#[derive(Clone)]
pub struct PlacedGlyph {
    pub tex: Arc<Texture>,
    pub uv: [f32; 4],
    pub x: i32,
    pub y: i32,
    pub w: u32,
    pub h: u32,
}

/// A line of text laid out: the letters, how wide the line is, and the heights above and below the baseline.
pub struct Run {
    pub glyphs: Vec<PlacedGlyph>,
    pub width: i32,
    pub ascent: i32,
    /// Ascent plus descent: how tall a line of this font is at this size.
    pub height: i32,
}

#[derive(Clone, Copy)]
struct Slot {
    page: usize,
    x: u32,
    y: u32,
    w: u32,
    h: u32,
    left: i32,
    top: i32,
}

struct Page {
    texels: Vec<[f32; 4]>,
    shelf_x: u32,
    shelf_y: u32,
    shelf_h: u32,
    snapshot: Option<Arc<Texture>>,
}

impl Page {
    fn new() -> Page {
        Page { texels: vec![[1.0, 1.0, 1.0, 0.0]; (PAGE * PAGE) as usize], shelf_x: 1, shelf_y: 1, shelf_h: 0, snapshot: None }
    }

    /// Room for a `w` x `h` bitmap (one texel of space round each)? Returns where.
    fn reserve(&mut self, w: u32, h: u32) -> Option<(u32, u32)> {
        if w + 2 > PAGE || h + 2 > PAGE {
            return None;
        }
        if self.shelf_x + w + 1 > PAGE {
            self.shelf_y += self.shelf_h + 1;
            self.shelf_x = 1;
            self.shelf_h = 0;
        }
        if self.shelf_y + h + 1 > PAGE {
            return None;
        }
        let at = (self.shelf_x, self.shelf_y);
        self.shelf_x += w + 1;
        self.shelf_h = self.shelf_h.max(h);
        Some(at)
    }

    fn texture(&mut self) -> Arc<Texture> {
        if self.snapshot.is_none() {
            self.snapshot = Some(Arc::new(Texture { width: PAGE, height: PAGE, texels: self.texels.clone(), heights: None }));
        }
        self.snapshot.clone().expect("just made")
    }
}

/// The fonts of a workspace and the letters drawn from them.
pub struct FontCache {
    root: Option<PathBuf>,
    files: BTreeMap<String, Option<usize>>,
    fonts: Vec<Arc<FontVec>>,
    glyphs: BTreeMap<(usize, char, u32), Option<Slot>>,
    pages: Vec<Page>,
    pub warnings: Vec<String>,
}

/// The scale that makes the font's em `px` pixels tall (ab_glyph's own scale is the height of ascent plus descent).
fn scale_for(font: &FontVec, px: u32) -> PxScale {
    let height = font.height_unscaled();
    let em = font.units_per_em().filter(|u| *u > 0.0).unwrap_or(height);
    PxScale::from(px as f32 * height / em)
}

impl FontCache {
    pub fn new(root: Option<PathBuf>) -> FontCache {
        FontCache { root, files: BTreeMap::new(), fonts: Vec::new(), glyphs: BTreeMap::new(), pages: Vec::new(), warnings: Vec::new() }
    }

    /// Does this name look like a font file?
    pub fn is_font_name(name: &str) -> bool {
        let l = name.to_ascii_lowercase();
        l.ends_with(".ttf") || l.ends_with(".otf")
    }

    fn load(&mut self, name: &str) -> Result<usize, String> {
        if let Some(r) = self.files.get(name) {
            return r.ok_or_else(|| "unreadable (said before)".to_string());
        }
        let r = self.read(name);
        match r {
            Ok(font) => {
                self.fonts.push(Arc::new(font));
                self.files.insert(name.to_string(), Some(self.fonts.len() - 1));
                Ok(self.fonts.len() - 1)
            }
            Err(e) => {
                self.warnings.push(format!("font `{name}`: {e} (the built-in font is used)"));
                self.files.insert(name.to_string(), None);
                Err(e)
            }
        }
    }

    fn read(&self, name: &str) -> Result<FontVec, String> {
        let root = self.root.as_ref().ok_or("no workspace folder to load files from")?;
        let p = std::path::Path::new(name);
        if p.components().any(|c| !matches!(c, std::path::Component::Normal(_) | std::path::Component::CurDir)) {
            return Err("path must be relative and stay inside the workspace".into());
        }
        let path = root.join(p);
        let len = std::fs::metadata(&path).map_err(|e| format!("{e}"))?.len();
        if len > MAX_FONT_BYTES {
            return Err(format!("the file is {len} bytes; fonts up to {MAX_FONT_BYTES} are read"));
        }
        let bytes = std::fs::read(&path).map_err(|e| format!("{e}"))?;
        FontVec::try_from_vec(bytes).map_err(|e| format!("not a TrueType or OpenType font ({e})"))
    }

    /// A letter drawn at `px` pixels (em size): its place in a page. `None` for a letter with no outline (a space).
    fn glyph(&mut self, fi: usize, ch: char, px: u32) -> Option<Slot> {
        if let Some(s) = self.glyphs.get(&(fi, ch, px)) {
            return *s;
        }
        let font = self.fonts[fi].clone();
        let id = font.glyph_id(ch);
        let slot = font.outline_glyph(id.with_scale_and_position(scale_for(&font, px), point(0.0, 0.0))).and_then(|og| {
            let b = og.px_bounds();
            let (w, h) = ((b.max.x - b.min.x).ceil().max(0.0) as u32, (b.max.y - b.min.y).ceil().max(0.0) as u32);
            if w == 0 || h == 0 || w > PAGE - 2 || h > PAGE - 2 {
                return None;
            }
            let (page, x, y) = loop {
                if let Some(p) = self.pages.last_mut() {
                    if let Some((x, y)) = p.reserve(w, h) {
                        break (self.pages.len() - 1, x, y);
                    }
                }
                self.pages.push(Page::new());
            };
            let p = &mut self.pages[page];
            og.draw(|gx, gy, c| {
                if gx < w && gy < h {
                    p.texels[((y + gy) * PAGE + x + gx) as usize] = [1.0, 1.0, 1.0, c.clamp(0.0, 1.0)];
                }
            });
            p.snapshot = None;
            Some(Slot { page, x, y, w, h, left: b.min.x.floor() as i32, top: b.min.y.floor() as i32 })
        });
        self.glyphs.insert((fi, ch, px), slot);
        slot
    }

    /// Lay a line of text out in the font file `file` at `px` pixels. The error is why the file cannot be used (already in `warnings`).
    pub fn layout(&mut self, file: &str, text: &str, px: u32) -> Result<Run, String> {
        let fi = self.load(file)?;
        let px = px.clamp(4, 400);
        let font = self.fonts[fi].clone();
        let scaled = font.as_scaled(scale_for(&font, px));
        let (ascent, descent) = (scaled.ascent().round() as i32, scaled.descent().round() as i32);
        let mut placed: Vec<(Slot, i32)> = Vec::new();
        let (mut pen, mut prev): (f32, Option<ab_glyph::GlyphId>) = (0.0, None);
        for ch in text.chars().take(512) {
            let ch = if ch.is_control() { ' ' } else { ch };
            let id = scaled.glyph_id(ch);
            if let Some(p) = prev {
                pen += scaled.kern(p, id);
            }
            // a letter missing from the font is the font's own "missing" box (glyph 0), as everywhere else
            if let Some(slot) = self.glyph(fi, ch, px) {
                placed.push((slot, pen.round() as i32));
            }
            pen += scaled.h_advance(id);
            prev = Some(id);
        }
        let mut glyphs = Vec::with_capacity(placed.len());
        let mut pages: BTreeMap<usize, Arc<Texture>> = BTreeMap::new();
        for (s, ox) in placed {
            let tex = pages.entry(s.page).or_insert_with(|| self.pages[s.page].texture()).clone();
            let uv = [s.x as f32 / PAGE as f32, s.y as f32 / PAGE as f32, (s.x + s.w) as f32 / PAGE as f32, (s.y + s.h) as f32 / PAGE as f32];
            glyphs.push(PlacedGlyph { tex, uv, x: ox + s.left, y: s.top, w: s.w, h: s.h });
        }
        Ok(Run { glyphs, width: pen.round() as i32, ascent, height: ascent - descent })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cache() -> FontCache {
        FontCache::new(Some(std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("testdata")))
    }

    #[test]
    fn a_line_of_text_becomes_pixel_aligned_glyph_quads() {
        let mut c = cache();
        let run = c.layout("pixel.ttf", "Hi there", 16).unwrap();
        assert!(c.warnings.is_empty(), "{:?}", c.warnings);
        // seven letters have an outline (the space has none)
        assert_eq!(run.glyphs.len(), 7);
        assert!(run.width > 7 * 6 && run.width < 8 * 16, "{}", run.width);
        assert_eq!(run.ascent, 14, "700 of 800 units at 16 pixels");
        assert_eq!(run.height, 16);
        // the capital H stands on the baseline: its quad ends at or above y = 0 and starts above
        let h = &run.glyphs[0];
        assert!(h.y < 0 && h.y + h.h as i32 <= 1, "{} {}", h.y, h.h);
        // its texels are white with some coverage somewhere
        let covered = (0..h.h).any(|r| (0..h.w).any(|c| {
            let (tx, ty) = (((h.uv[0] * PAGE as f32) as u32) + c, ((h.uv[1] * PAGE as f32) as u32) + r);
            h.tex.texels[(ty * PAGE + tx) as usize][3] > 0.5
        }));
        assert!(covered);
        // the same letter twice makes the same slot: one more letter does not make a new page
        let again = c.layout("pixel.ttf", "HH", 16).unwrap();
        assert_eq!(again.glyphs[0].uv, again.glyphs[1].uv);
        assert_eq!(again.glyphs[0].uv, run.glyphs[0].uv);
    }

    #[test]
    fn a_bigger_size_is_a_bigger_letter_and_a_missing_font_is_reported_once() {
        let mut c = cache();
        let small = c.layout("pixel.ttf", "W", 16).unwrap();
        let big = c.layout("pixel.ttf", "W", 32).unwrap();
        assert!(big.glyphs[0].w >= small.glyphs[0].w * 2 - 2, "{} vs {}", big.glyphs[0].w, small.glyphs[0].w);
        assert!(c.layout("nope.ttf", "x", 16).is_err());
        assert!(c.layout("nope.ttf", "x", 16).is_err());
        assert_eq!(c.warnings.len(), 1, "{:?}", c.warnings);
        assert!(c.layout("../x.ttf", "x", 16).is_err());
        // a file that is not a font
        assert!(c.layout("jpg_color.jpg", "x", 16).is_err());
        // a page is made once per glyph set: the snapshot of a page is shared between lines drawn with it
        let a = c.layout("pixel.ttf", "ab", 20).unwrap();
        let b = c.layout("pixel.ttf", "ab", 20).unwrap();
        assert!(Arc::ptr_eq(&a.glyphs[0].tex, &b.glyphs[0].tex));
    }
}
