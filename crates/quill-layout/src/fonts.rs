//! Installed fonts: finding them, loading them on demand, and measuring.
//!
//! Font files are parsed lazily when used. Fonts you installed are copied into
//! private memory, which stays valid if the file is replaced or truncated; the
//! Mac's read-only system fonts are memory-mapped.

use ab_glyph::{Font, FontRef, GlyphId, VariableFont};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

pub type FaceId = u16;

/// A face style that is actually installed for a font family.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FontVariant {
    pub bold: bool,
    pub italic: bool,
}

/// One visible system font family and the installed styles Quill can express.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FontFamilyInfo {
    pub name: String,
    pub variants: Vec<FontVariant>,
}

/// A loaded face plus the styling we have to fake because the family has
/// no real bold or italic.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct FaceKey {
    pub face: FaceId,
    pub fake_bold: bool,
    pub fake_italic: bool,
}

#[derive(Clone, Debug)]
struct FaceSource {
    path: PathBuf,
    index: u32,
    weight: u16,
    italic: bool,
}

#[derive(Clone, Debug)]
struct Family {
    name: String,
    faces: Vec<FaceSource>,
    /// A system interface font, such as macOS's "System Font", whose
    /// faces have private names starting with a dot. It still lays out
    /// text, but stays out of the Font menu: asking AppKit for it by name
    /// gets Times instead, with a console warning.
    hidden: bool,
}

/// The installed font families, found by scanning the system. Scanning can
/// take a moment on a cold start, so it can run on a background thread.
#[derive(Clone, Debug, Default)]
pub struct Catalog {
    families: Vec<Family>,
}

impl Catalog {
    /// Scan the system's font folders.
    pub fn scan_system() -> Catalog {
        let mut db = fontdb::Database::new();
        db.load_system_fonts();
        let mut catalog = Catalog::default();
        catalog.add_faces(&db);
        catalog
    }

    /// This catalog plus the fonts found in `folders`, each searched down
    /// `levels` folder levels below it. A family that is already here gains
    /// the new faces; the rest join the list. Nothing is installed: drop the
    /// folder and call this again to take the fonts away.
    pub fn with_folders(&self, folders: &[PathBuf], levels: u8) -> Catalog {
        let mut files = Vec::new();
        for folder in folders {
            collect_font_files(folder, levels, &mut files);
        }
        if files.is_empty() {
            return self.clone();
        }
        let mut db = fontdb::Database::new();
        for file in &files {
            // A broken or unreadable font file is skipped, not fatal.
            let _ = db.load_font_file(file);
        }
        let mut catalog = self.clone();
        catalog.add_faces(&db);
        catalog
    }

    /// Fold the font files in `db` into the family list, keeping it sorted.
    fn add_faces(&mut self, db: &fontdb::Database) {
        let mut index: HashMap<String, usize> = self
            .families
            .iter()
            .enumerate()
            .map(|(i, f)| (f.name.to_lowercase(), i))
            .collect();
        for face in db.faces() {
            let fontdb::Source::File(path) = &face.source else {
                continue;
            };
            let Some((name, _)) = face.families.first() else {
                continue;
            };
            let key = name.to_lowercase();
            let fam = *index.entry(key).or_insert_with(|| {
                self.families.push(Family {
                    name: name.clone(),
                    faces: Vec::new(),
                    hidden: name.starts_with('.'),
                });
                self.families.len() - 1
            });
            if face.post_script_name.starts_with('.') {
                self.families[fam].hidden = true;
            }
            if self.families[fam]
                .faces
                .iter()
                .any(|f| f.path == *path && f.index == face.index)
            {
                continue;
            }
            self.families[fam].faces.push(FaceSource {
                path: path.clone(),
                index: face.index,
                weight: face.weight.0,
                italic: face.style != fontdb::Style::Normal,
            });
        }
        self.families.sort_by_key(|f| f.name.to_lowercase());
    }

    /// A catalog of specific font files, for tests and for bundled fonts.
    pub fn from_files(files: &[(&str, PathBuf, u32, u16, bool)]) -> Catalog {
        let mut catalog = Catalog::default();
        for (name, path, index, weight, italic) in files {
            let source = FaceSource {
                path: path.clone(),
                index: *index,
                weight: *weight,
                italic: *italic,
            };
            match catalog.families.iter_mut().find(|f| f.name == *name) {
                Some(f) => f.faces.push(source),
                None => catalog.families.push(Family {
                    name: name.to_string(),
                    faces: vec![source],
                    hidden: name.starts_with('.'),
                }),
            }
        }
        catalog
    }

    fn find(&self, name: &str) -> Option<&Family> {
        self.families
            .iter()
            .find(|f| f.name.eq_ignore_ascii_case(name))
    }
}

/// Most font files Quill will pick up from the extra folders in one go, so
/// pointing a slot at a whole disk can't hang the scan.
const MAX_FOLDER_FONT_FILES: usize = 20_000;

fn is_font_file(path: &Path) -> bool {
    path.extension().and_then(|e| e.to_str()).is_some_and(|e| {
        matches!(
            e.to_ascii_lowercase().as_str(),
            "ttf" | "otf" | "ttc" | "otc"
        )
    })
}

/// Font files in `dir` and in folders up to `levels` below it (0 means the
/// folder itself only). Shortcuts to folders are not followed, so a loop can't
/// trap the search; hidden files and folders are skipped.
pub fn collect_font_files(dir: &Path, levels: u8, out: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    let mut entries: Vec<_> = entries.flatten().collect();
    entries.sort_by_key(|e| e.file_name());
    for entry in entries {
        if out.len() >= MAX_FOLDER_FONT_FILES {
            return;
        }
        if entry.file_name().to_string_lossy().starts_with('.') {
            continue;
        }
        let path = entry.path();
        let Ok(kind) = entry.file_type() else {
            continue;
        };
        if kind.is_dir() {
            if levels > 0 {
                collect_font_files(&path, levels - 1, out);
            }
        } else if is_font_file(&path) && (kind.is_file() || path.is_file()) {
            out.push(path);
        }
    }
}

/// Host interface font role, independent of imported document font names.
pub const UI_FONT: &str = "Quill UI";

/// Installed look-alikes for the classic Mac fonts the original's documents use,
/// for the fonts current Office documents use (which Office carries inside
/// itself, so other apps don't have them), and the usual cross-platform
/// stand-ins. Tried in order after the name itself. Carlito and Caladea,
/// free fonts with Calibri's and Cambria's widths, come first when someone
/// has installed them, so lines break where they did in Word.
fn aliases(name: &str) -> &'static [&'static str] {
    match name.to_ascii_lowercase().as_str() {
        "quill ui" => {
            #[cfg(target_os = "macos")]
            {
                &[
                    ".SF NS",
                    ".SF Compact",
                    "Helvetica Neue",
                    "Helvetica",
                    "Arial",
                ]
            }
            #[cfg(windows)]
            {
                &["Segoe UI", "Arial"]
            }
            #[cfg(not(any(target_os = "macos", windows)))]
            {
                &["Noto Sans", "DejaVu Sans", "Liberation Sans", "Arial"]
            }
        }
        "new york" => &[
            ".New York",
            "Times",
            "Times New Roman",
            "Liberation Serif",
            "DejaVu Serif",
        ],
        "georgia" => &[
            "Georgia",
            "Charter",
            "Noto Serif",
            "Liberation Serif",
            "DejaVu Serif",
        ],
        // Legacy document font substitutions; the interface has its own role.
        "geneva" => &[
            ".SF NS",
            ".SF Compact",
            "Segoe UI",
            "Noto Sans",
            "Helvetica Neue",
            "Lucida Grande",
            "Verdana",
            "DejaVu Sans",
            "Arial",
            "Liberation Sans",
        ],
        "chicago" | "charcoal" => &[
            ".SF NS",
            ".SF Compact",
            "Segoe UI",
            "Noto Sans",
            "Helvetica Neue",
            "Lucida Grande",
            "DejaVu Sans",
            "Arial",
        ],
        "monaco" => &["Menlo", "Consolas", "DejaVu Sans Mono", "Courier New"],
        "times" => &["Times New Roman", "Liberation Serif", "DejaVu Serif"],
        "times new roman" => &["Times", "Liberation Serif", "DejaVu Serif"],
        "helvetica" => &["Arial", "Liberation Sans", "DejaVu Sans"],
        "arial" => &["Helvetica", "Liberation Sans", "DejaVu Sans"],
        "courier" => &["Courier New", "Liberation Mono", "DejaVu Sans Mono"],
        "courier new" => &["Courier", "Liberation Mono", "DejaVu Sans Mono"],
        "palatino" => &[
            "Palatino Linotype",
            "Book Antiqua",
            "TeX Gyre Pagella",
            "DejaVu Serif",
        ],
        "symbol" => &["Symbol", "Standard Symbols PS"],
        // Office's sans-serif faces.
        "calibri" | "calibri light" => &[
            "Carlito",
            "Helvetica Neue",
            "Avenir Next",
            "Segoe UI",
            "Arial",
            "Liberation Sans",
            "DejaVu Sans",
        ],
        "aptos" | "aptos display" | "aptos narrow" | "segoe ui" | "candara" | "corbel" => &[
            "Helvetica Neue",
            "Avenir Next",
            "Segoe UI",
            "Arial",
            "Liberation Sans",
            "DejaVu Sans",
        ],
        "century gothic" => &["Avenir Next", "Futura", "Helvetica Neue", "Arial"],
        // Office's serif faces.
        "cambria" => &[
            "Caladea",
            "Charter",
            "Georgia",
            "Times New Roman",
            "Liberation Serif",
            "DejaVu Serif",
        ],
        "constantia" => &["Charter", "Georgia", "Times New Roman", "Liberation Serif"],
        "book antiqua" | "palatino linotype" => &["Palatino", "TeX Gyre Pagella", "DejaVu Serif"],
        "garamond" => &[
            "EB Garamond",
            "Baskerville",
            "Times New Roman",
            "Liberation Serif",
        ],
        "consolas" | "lucida console" => &["Menlo", "Monaco", "DejaVu Sans Mono", "Courier New"],
        // Japanese and Chinese faces Office documents name.
        "ms mincho" | "ms pmincho" | "yu mincho" | "ＭＳ 明朝" | "游明朝" => {
            &["Hiragino Mincho ProN", "Noto Serif CJK JP", "Noto Serif JP"]
        }
        "ms gothic" | "ms pgothic" | "meiryo" | "yu gothic" | "ＭＳ ゴシック" | "游ゴシック" => {
            &[
                "Hiragino Sans",
                "Hiragino Kaku Gothic ProN",
                "Noto Sans CJK JP",
                "Noto Sans JP",
            ]
        }
        "simsun" | "nsimsun" | "宋体" => &["Songti SC", "STSong", "Noto Serif CJK SC"],
        "microsoft yahei" | "simhei" | "dengxian" | "微软雅黑" | "黑体" | "等线" => {
            &["PingFang SC", "Hiragino Sans GB", "Noto Sans CJK SC"]
        }
        _ => &[],
    }
}

/// Used when a font and all its aliases are missing.
const LAST_RESORT: &[&str] = &[
    "Times",
    "Times New Roman",
    "Liberation Serif",
    "DejaVu Serif",
    "Georgia",
    "Helvetica",
    "Arial",
    "DejaVu Sans",
];

/// Searched, in order, for characters the chosen font lacks.
const FALLBACKS: &[&str] = &[
    "Apple Symbols",
    "Segoe UI Symbol",
    "DejaVu Sans",
    "Arial Unicode MS",
    "Lucida Grande",
    "Hiragino Sans",
    "Microsoft YaHei",
    "Noto Sans CJK SC",
    "Apple Color Emoji",
];

/// One piece of a glyph outline, in font units.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Curve {
    Line((f32, f32), (f32, f32)),
    Quad((f32, f32), (f32, f32), (f32, f32)),
    Cubic((f32, f32), (f32, f32), (f32, f32), (f32, f32)),
}

/// A face ready for measuring and drawing.
pub struct LoadedFace {
    pub font: FontRef<'static>,
    /// The font file's bytes and the face's index in it (a collection
    /// holds several), for reading tables `font` does not show.
    pub data: &'static [u8],
    pub index: u32,
    /// A name for the face: its file's name, with the weight it was
    /// loaded at when that is not the default. For PDF font names.
    pub name: String,
    pub units_per_em: f32,
    /// Distance above the baseline, in font units (positive).
    pub ascent: f32,
    /// Distance below the baseline, in font units (positive).
    pub descent: f32,
    pub line_gap: f32,
}

/// Vertical metrics of a face at a size, in points.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct VMetrics {
    pub ascent: f32,
    pub descent: f32,
    pub line_gap: f32,
}

pub struct Fonts {
    catalog: Catalog,
    faces: Vec<LoadedFace>,
    loaded: HashMap<(PathBuf, u32, u16), FaceId>,
    resolved: HashMap<(String, bool, bool), FaceKey>,
    fallback_faces: Option<Vec<FaceId>>,
}

/// Font bytes live for the whole run, so faces can borrow them as `'static`.
/// A file is read once however many faces use it. Fonts you installed get a
/// private copy: they can be replaced or truncated while Quill is open, which
/// can turn a live memory map into a process-killing SIGBUS. The Mac's own
/// fonts sit on the sealed system volume and cannot change under us, so they
/// are mapped instead: only the pages a document touches are read in, which
/// matters for a 190 MB emoji font or a 55 MB CJK one.
fn map_file(path: &PathBuf) -> Option<&'static [u8]> {
    static MAPS: OnceLock<std::sync::Mutex<HashMap<PathBuf, &'static [u8]>>> = OnceLock::new();
    let maps = MAPS.get_or_init(Default::default);
    let mut maps = maps.lock().ok()?;
    if let Some(data) = maps.get(path) {
        return Some(data);
    }
    let data = match map_immutable(path) {
        Some(data) => data,
        None => Box::leak(std::fs::read(path).ok()?.into_boxed_slice()),
    };
    maps.insert(path.clone(), data);
    Some(data)
}

/// Map a font file from the Mac's read-only system volume.
#[cfg(target_os = "macos")]
fn map_immutable(path: &Path) -> Option<&'static [u8]> {
    if !path.starts_with("/System/") {
        return None;
    }
    let file = std::fs::File::open(path).ok()?;
    // SAFETY: the file is on the sealed system volume, which nothing can
    // modify or truncate while the process runs.
    let map = unsafe { memmap2::Mmap::map(&file) }.ok()?;
    let map: &'static memmap2::Mmap = Box::leak(Box::new(map));
    Some(&map[..])
}

#[cfg(not(target_os = "macos"))]
fn map_immutable(_: &Path) -> Option<&'static [u8]> {
    None
}

impl Fonts {
    pub fn new(catalog: Catalog) -> Fonts {
        Fonts {
            catalog,
            faces: Vec::new(),
            loaded: HashMap::new(),
            resolved: HashMap::new(),
            fallback_faces: None,
        }
    }

    /// Family names for the Font menu, sorted, without hidden system fonts.
    pub fn family_names(&self) -> Vec<&str> {
        self.catalog
            .families
            .iter()
            .filter(|f| !f.hidden)
            .map(|f| f.name.as_str())
            .collect()
    }

    /// Visible installed families and their real regular/bold/italic faces.
    /// Multiple weights on the same side of bold are one variant until Quill's
    /// document model grows named weight support.
    pub fn family_info(&self) -> Vec<FontFamilyInfo> {
        self.catalog
            .families
            .iter()
            .filter(|f| !f.hidden)
            .map(|f| {
                let mut variants = Vec::new();
                for (bold, italic) in [(false, false), (true, false), (false, true), (true, true)] {
                    if f.faces
                        .iter()
                        .any(|face| (face.weight >= 600) == bold && face.italic == italic)
                    {
                        variants.push(FontVariant { bold, italic });
                    }
                }
                FontFamilyInfo {
                    name: f.name.clone(),
                    variants,
                }
            })
            .collect()
    }

    /// True when a family by this name (or a look-alike) is installed.
    pub fn has_family(&self, name: &str) -> bool {
        self.catalog.find(name).is_some()
    }

    pub fn face(&self, id: FaceId) -> &LoadedFace {
        &self.faces[id as usize]
    }

    /// The installed family shown in place of `name`, chosen the way
    /// `resolve` chooses it. None when `name` is installed, or when the
    /// stand-in is the same design under a private name (macOS keeps New
    /// York as ".New York").
    pub fn substitute(&self, name: &str) -> Option<&str> {
        if self.catalog.find(name).is_some() {
            return None;
        }
        let found = aliases(name)
            .iter()
            .chain(LAST_RESORT)
            .find_map(|n| self.catalog.find(n))
            .or_else(|| self.catalog.families.first())?;
        if found
            .name
            .trim_start_matches('.')
            .eq_ignore_ascii_case(name)
        {
            return None;
        }
        Some(&found.name)
    }

    /// The face to use for `name` in bold and/or italic.
    pub fn resolve(&mut self, name: &str, bold: bool, italic: bool) -> FaceKey {
        let key = (name.to_lowercase(), bold, italic);
        if let Some(k) = self.resolved.get(&key) {
            return *k;
        }
        let found = std::iter::once(name)
            .chain(aliases(name).iter().copied())
            .chain(LAST_RESORT.iter().copied())
            .find_map(|n| self.catalog.find(n).cloned())
            .or_else(|| self.catalog.families.first().cloned());
        let result = match found {
            Some(family) => {
                self.pick_face(&family, bold, italic, name.eq_ignore_ascii_case(UI_FONT))
            }
            None => FaceKey::default(),
        };
        self.resolved.insert(key, result);
        result
    }

    /// A glyph's outline in font units (y up), for drawing it as vectors,
    /// and the face's units per em. None for glyphs with no outline.
    pub fn outline(&self, key: FaceKey, id: GlyphId) -> Option<(f32, Vec<Curve>)> {
        let face = self.face(key.face);
        let o = face.font.outline(id)?;
        let p = |q: ab_glyph::Point| (q.x, q.y);
        let curves = o
            .curves
            .iter()
            .map(|c| match *c {
                ab_glyph::OutlineCurve::Line(a, b) => Curve::Line(p(a), p(b)),
                ab_glyph::OutlineCurve::Quad(a, b, c) => Curve::Quad(p(a), p(b), p(c)),
                ab_glyph::OutlineCurve::Cubic(a, b, c, d) => Curve::Cubic(p(a), p(b), p(c), p(d)),
            })
            .collect();
        Some((face.units_per_em, curves))
    }

    /// A face `resolve` has already found, for drawing code that cannot
    /// load new ones. Falls back to the first face.
    pub fn find_resolved(&self, name: &str, bold: bool, italic: bool) -> FaceKey {
        self.resolved
            .get(&(name.to_lowercase(), bold, italic))
            .copied()
            .unwrap_or_default()
    }

    fn pick_face(&mut self, family: &Family, bold: bool, italic: bool, interface: bool) -> FaceKey {
        let want = if bold { 700 } else { 400 };
        let best = family
            .faces
            .iter()
            .min_by_key(|f| {
                let w = (f.weight as i32 - want).unsigned_abs();
                let i = if f.italic == italic { 0 } else { 10_000 };
                w + i
            })
            .expect("families have faces");
        let mut real_bold = best.weight >= 600;
        let real_italic = best.italic;
        let mut weight_axis = 0;
        if interface && self.is_variable_weight(best) {
            // System variable fonts need an explicit regular UI instance;
            // their file default is not necessarily the platform's regular weight.
            weight_axis = want as u16;
            real_bold = bold;
        } else if bold && !real_bold && self.is_variable_weight(best) {
            weight_axis = 700;
            real_bold = true;
        }
        let Some(face) = self.load(best, weight_axis) else {
            return FaceKey::default();
        };
        FaceKey {
            face,
            fake_bold: bold && !real_bold,
            fake_italic: italic && !real_italic,
        }
    }

    fn is_variable_weight(&self, src: &FaceSource) -> bool {
        map_file(&src.path)
            .and_then(|data| FontRef::try_from_slice_and_index(data, src.index).ok())
            .is_some_and(|f| {
                f.variations()
                    .iter()
                    .any(|a| &a.tag == b"wght" && a.max_value >= 700.0)
            })
    }

    fn load(&mut self, src: &FaceSource, weight_axis: u16) -> Option<FaceId> {
        let key = (src.path.clone(), src.index, weight_axis);
        if let Some(id) = self.loaded.get(&key) {
            return Some(*id);
        }
        let data = map_file(&src.path)?;
        let mut font = FontRef::try_from_slice_and_index(data, src.index).ok()?;
        if weight_axis != 0 {
            font.set_variation(b"wght", weight_axis as f32);
        }
        // Variable fonts with an optical size axis (like macOS's New York)
        // default to their display design; text sizes read better.
        font.set_variation(b"opsz", 12.0);
        let units_per_em = font.units_per_em().unwrap_or(1000.0);
        let stem = src
            .path
            .file_stem()
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_default();
        let name = match (src.index, weight_axis) {
            (0, 0) => stem,
            (i, 0) => format!("{stem}-{i}"),
            (i, w) => format!("{stem}-{i}-{w}"),
        };
        let face = LoadedFace {
            data,
            index: src.index,
            name,
            ascent: font.ascent_unscaled(),
            descent: -font.descent_unscaled(),
            line_gap: font.line_gap_unscaled(),
            units_per_em,
            font,
        };
        self.faces.push(face);
        let id = (self.faces.len() - 1) as FaceId;
        self.loaded.insert(key, id);
        Some(id)
    }

    /// The glyph for `c` in `key`, or in a fallback font when `key`'s face
    /// lacks it. Returns the face actually used.
    pub fn glyph(&mut self, key: FaceKey, c: char) -> (FaceKey, GlyphId) {
        let id = self
            .faces
            .get(key.face as usize)
            .map(|f| f.font.glyph_id(c));
        match id {
            Some(id) if id.0 != 0 => (key, id),
            _ => {
                for face in self.fallbacks() {
                    let g = self.faces[face as usize].font.glyph_id(c);
                    if g.0 != 0 {
                        return (FaceKey { face, ..key }, g);
                    }
                }
                (key, GlyphId(0))
            }
        }
    }

    fn fallbacks(&mut self) -> Vec<FaceId> {
        if self.fallback_faces.is_none() {
            let mut list = Vec::new();
            for name in FALLBACKS {
                if let Some(family) = self.catalog.find(name).cloned() {
                    list.push(self.pick_face(&family, false, false, false).face);
                }
            }
            self.fallback_faces = Some(list);
        }
        self.fallback_faces.clone().unwrap_or_default()
    }

    /// The glyph for `c` in exactly this face (no fallback); 0 if missing.
    pub fn glyph_in(&self, key: FaceKey, c: char) -> GlyphId {
        self.faces
            .get(key.face as usize)
            .map_or(GlyphId(0), |f| f.font.glyph_id(c))
    }

    /// Advance width of a glyph at `size` points, in points.
    pub fn advance(&self, key: FaceKey, glyph: GlyphId, size: f32) -> f32 {
        let face = &self.faces[key.face as usize];
        let adv = face.font.h_advance_unscaled(glyph) * size / face.units_per_em;
        if key.fake_bold {
            adv + bold_offset(size)
        } else {
            adv
        }
    }

    /// Kerning between two glyphs of the same face, in points.
    pub fn kern(&self, key: FaceKey, a: GlyphId, b: GlyphId, size: f32) -> f32 {
        let face = &self.faces[key.face as usize];
        face.font.kern_unscaled(a, b) * size / face.units_per_em
    }

    pub fn vmetrics(&self, key: FaceKey, size: f32) -> VMetrics {
        let face = &self.faces[key.face as usize];
        let s = size / face.units_per_em;
        VMetrics {
            ascent: face.ascent * s,
            descent: face.descent * s,
            line_gap: face.line_gap * s,
        }
    }
}

/// How far synthetic bold smears a glyph to the right, in points. QuickDraw
/// used one pixel at screen sizes; this scales the same idea.
pub fn bold_offset(size: f32) -> f32 {
    (size / 24.0).max(0.5)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn system() -> Fonts {
        Fonts::new(Catalog::scan_system())
    }

    #[test]
    fn resolves_something_for_any_name() {
        let mut fonts = system();
        if fonts.family_names().is_empty() {
            return; // no fonts on this machine (a bare CI container)
        }
        let k = fonts.resolve("No Such Font Anywhere", false, false);
        let face = fonts.face(k.face);
        assert!(face.units_per_em > 0.0);
    }

    fn font_folder_tree(name: &str) -> PathBuf {
        let root = std::env::temp_dir().join(format!("quill-fonts-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        for sub in ["", "a", "a/b", "a/b/c", ".hidden"] {
            std::fs::create_dir_all(root.join(sub)).unwrap();
            std::fs::write(root.join(sub).join("x.ttf"), b"not a font").unwrap();
            std::fs::write(root.join(sub).join("notes.txt"), b"text").unwrap();
        }
        root
    }

    #[test]
    fn folder_search_stops_at_the_chosen_depth_and_takes_only_font_files() {
        let root = font_folder_tree("depth");
        for (levels, expected) in [(0u8, 1usize), (1, 2), (2, 3), (7, 4)] {
            let mut found = Vec::new();
            collect_font_files(&root, levels, &mut found);
            assert_eq!(found.len(), expected, "levels {levels}");
            assert!(
                found
                    .iter()
                    .all(|p| !p.to_string_lossy().contains(".hidden"))
            );
        }
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn extra_folders_add_families_and_dropping_them_takes_them_away() {
        let base = Catalog::scan_system();
        let Some(source) = base
            .families
            .iter()
            .flat_map(|f| f.faces.iter())
            .map(|f| f.path.clone())
            .find(|p| is_font_file(p))
        else {
            return;
        };
        let name = format!("copy.{}", source.extension().unwrap().to_string_lossy());
        let root = font_folder_tree("merge");
        let deep = root.join("a/b");
        std::fs::copy(&source, deep.join(&name)).unwrap();
        let shallow = base.with_folders(std::slice::from_ref(&root), 1);
        let deeper = base.with_folders(std::slice::from_ref(&root), 3);
        let count = |c: &Catalog| c.families.iter().map(|f| f.faces.len()).sum::<usize>();
        assert_eq!(count(&shallow), count(&base), "too deep for one level");
        assert!(
            count(&deeper) > count(&base),
            "the copied font joined its family"
        );
        assert_eq!(count(&base.with_folders(&[], 4)), count(&base));
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn classic_mac_names_resolve() {
        let mut fonts = system();
        if fonts.family_names().is_empty() {
            return;
        }
        for name in [
            "New York",
            "Geneva",
            "Chicago",
            "Monaco",
            "Helvetica",
            "Times",
            "Courier",
        ] {
            let k = fonts.resolve(name, false, false);
            let (_, g) = fonts.glyph(k, 'A');
            assert_ne!(g.0, 0, "{name} has no 'A'");
            assert!(fonts.advance(k, g, 12.0) > 3.0, "{name} 'A' too narrow");
        }
    }

    #[test]
    fn bold_is_real_or_faked() {
        let mut fonts = system();
        if fonts.family_names().is_empty() {
            return;
        }
        let regular = fonts.resolve("Times", false, false);
        let bold = fonts.resolve("Times", true, false);
        assert!(bold.face != regular.face || bold.fake_bold);
    }

    #[test]
    fn substitutes_name_the_font_shown_instead() {
        let path = PathBuf::from("unused.ttf");
        let fonts = Fonts::new(Catalog::from_files(&[
            ("Times", path.clone(), 0, 400, false),
            ("Arial", path.clone(), 0, 400, false),
            (".New York", path, 0, 400, false),
        ]));
        assert_eq!(fonts.substitute("Arial"), None);
        assert_eq!(fonts.substitute("New York"), None);
        // Office's fonts get a face of their kind, sans or serif.
        assert_eq!(fonts.substitute("Calibri"), Some("Arial"));
        assert_eq!(fonts.substitute("Cambria"), Some("Times"));
        assert_eq!(fonts.substitute("Zapfino"), Some("Times"));
        assert_eq!(fonts.substitute("Helvetica"), Some("Arial"));
    }

    #[test]
    fn family_info_reports_only_installed_style_combinations() {
        let path = PathBuf::from("unused.ttf");
        let fonts = Fonts::new(Catalog::from_files(&[
            ("Arial", path.clone(), 0, 400, false),
            ("Arial", path.clone(), 1, 700, false),
            ("Arial", path.clone(), 2, 400, true),
            ("Wingdings", path, 0, 400, false),
        ]));
        let families = fonts.family_info();
        assert_eq!(families[0].name, "Arial");
        assert_eq!(
            families[0].variants,
            vec![
                FontVariant {
                    bold: false,
                    italic: false
                },
                FontVariant {
                    bold: true,
                    italic: false
                },
                FontVariant {
                    bold: false,
                    italic: true
                },
            ]
        );
        assert_eq!(families[1].variants.len(), 1);
    }

    #[test]
    fn loaded_font_bytes_survive_the_source_file_being_truncated() {
        let path = std::env::temp_dir().join(format!(
            "quill-font-copy-{}-{:?}.bin",
            std::process::id(),
            std::thread::current().id()
        ));
        std::fs::write(&path, b"stable font bytes").unwrap();
        let loaded = map_file(&path).unwrap();
        std::fs::write(&path, []).unwrap();
        assert_eq!(loaded, b"stable font bytes");
        std::fs::remove_file(path).unwrap();
    }
}
