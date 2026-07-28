use resvg::usvg;
use std::path::Path;

/// Loads a small, deterministic Windows font set instead of scanning the whole
/// system font directory on every drawing open or command-line conversion.
pub fn configure(options: &mut usvg::Options<'_>, svg: &str) {
    if !svg.contains("<text") {
        return;
    }

    #[cfg(windows)]
    {
        let database = options.fontdb_mut();
        for path in [
            r"C:\Windows\Fonts\arial.ttf",
            r"C:\Windows\Fonts\arialbd.ttf",
            r"C:\Windows\Fonts\msyh.ttc",
            r"C:\Windows\Fonts\simsun.ttc",
        ] {
            if Path::new(path).is_file() {
                let _ = database.load_font_file(path);
            }
        }
        database.set_sans_serif_family("Arial");
        database.set_serif_family("Arial");
    }

    #[cfg(not(windows))]
    options.fontdb_mut().load_system_fonts();
}
