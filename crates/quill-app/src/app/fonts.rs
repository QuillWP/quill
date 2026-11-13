//! Finding the system's fonts and the extra font folders at launch.

use super::*;

/// The extra font folders that are switched on, and how deep to search them.
#[derive(Clone, Debug, PartialEq)]
pub(super) struct FontFolders {
    pub(super) paths: Vec<PathBuf>,
    pub(super) levels: u8,
}

impl FontFolders {
    /// The sources switched on in the settings. One that is too broad to
    /// search (the settings file can be edited by hand) is left out.
    pub(super) fn current() -> Self {
        crate::settings::with(|s| FontFolders {
            paths: s
                .font_folders
                .iter()
                .filter(|f| f.enabled)
                .filter(|f| quill_layout::fonts::source_refusal(&f.path).is_none())
                .map(|f| f.path.clone())
                .collect(),
            levels: s.font_folder_levels,
        })
    }

    pub(super) fn apply(&self, system: &Catalog) -> Catalog {
        system.with_folders(&self.paths, self.levels)
    }
}

/// Finding the fonts at launch: the system's, then the extra sources'
/// with them, on a thread of its own while the window system starts up.
pub struct FontLoad {
    pub(super) folders: FontFolders,
    /// The system's fonts alone, and with the sources added.
    pub(super) found: JoinHandle<(Catalog, Catalog)>,
}

/// Start finding the fonts. Call it after the settings are loaded.
pub fn start_font_load() -> FontLoad {
    crate::settings::drop_missing_font_folders();
    let folders = FontFolders::current();
    let wanted = folders.clone();
    let found = std::thread::spawn(move || {
        let system = Catalog::scan_system();
        let all = wanted.apply(&system);
        (system, all)
    });
    FontLoad { folders, found }
}
