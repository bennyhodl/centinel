//! The web workspace bundle: every file `npm run build` leaves in `web-dist/`, the
//! route it is served at, and its content type. `build.rs` embeds these files and
//! `centinel web --rebuild` reads them back from disk, so both agree on one table.

use std::path::{Path, PathBuf};

/// Each file under `dist` as `(route, content type, path)`, sorted by route. The shell
/// is served at `/web`; everything else under `/web/`, where Vite's `base` points.
pub fn files(dist: &Path) -> std::io::Result<Vec<(String, &'static str, PathBuf)>> {
    let mut found = Vec::new();
    visit(dist, &mut found)?;
    let mut files: Vec<_> = found
        .into_iter()
        .map(|file| {
            let relative = file
                .strip_prefix(dist)
                .unwrap_or(&file)
                .to_string_lossy()
                .replace('\\', "/");
            let route = if relative == "index.html" {
                "/web".to_string()
            } else {
                format!("/web/{relative}")
            };
            (route, mime(&file), file)
        })
        .collect();
    files.sort();
    Ok(files)
}

fn visit(dir: &Path, files: &mut Vec<PathBuf>) -> std::io::Result<()> {
    for entry in std::fs::read_dir(dir)? {
        let path = entry?.path();
        if path.is_dir() {
            visit(&path, files)?;
        } else if path.is_file() {
            files.push(path);
        }
    }
    Ok(())
}

fn mime(path: &Path) -> &'static str {
    match path.extension().and_then(|x| x.to_str()) {
        Some("html") => "text/html; charset=utf-8",
        Some("js") => "text/javascript; charset=utf-8",
        Some("css") => "text/css; charset=utf-8",
        Some("svg") => "image/svg+xml",
        Some("png") => "image/png",
        Some("woff2") => "font/woff2",
        _ => "application/octet-stream",
    }
}
