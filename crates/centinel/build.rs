use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

#[path = "src/bundle.rs"]
mod bundle;

fn main() {
    let crate_dir = PathBuf::from(std::env::var("CARGO_MANIFEST_DIR").unwrap());
    let root = crate_dir.join("../..");
    let dist = root.join("web-dist");
    println!("cargo:rerun-if-changed={}", root.join("web").display());
    println!(
        "cargo:rerun-if-changed={}",
        root.join("package.json").display()
    );
    println!(
        "cargo:rerun-if-changed={}",
        root.join("package-lock.json").display()
    );
    println!(
        "cargo:rerun-if-changed={}",
        root.join("vite.config.ts").display()
    );
    println!(
        "cargo:rerun-if-changed={}",
        root.join("tsconfig.json").display()
    );

    let version = std::env::var("CARGO_PKG_VERSION").unwrap();
    require_node(&root);
    let modules = root.join("node_modules");
    let lock = root.join("package-lock.json");
    let lock_is_newer = modified(&lock) > modified(&modules);
    if !modules.is_dir() || lock_is_newer {
        run(&root, "npm", &["ci"], &version);
    }
    run(&root, "npm", &["run", "build"], &version);
    let index = dist.join("index.html");
    if !index.is_file() {
        panic!(
            "web-dist/index.html is absent; run `npm ci` and `npm run build` before building Centinel"
        );
    }
    let html = fs::read_to_string(&index).unwrap();
    match stamped_version(&html) {
        Some(stamped) if stamped == version => {}
        Some(stamped) => panic!(
            "web-dist/index.html is stamped v{stamped} but Centinel is v{version}; \
             the bundle and the binary must carry the same version"
        ),
        None => panic!(
            "web-dist/index.html carries no centinel-version meta tag; \
             the Start root route must stamp it"
        ),
    }
    warn(&format!(
        "web workspace v{version}: {} bytes embedded from web-dist/index.html",
        html.len()
    ));

    let mut generated = String::from("pub static WEB_ASSETS: &[(&str, &str, &[u8])] = &[\n");
    for (route, mime, file) in bundle::files(&dist).unwrap() {
        generated.push_str(&format!(
            "({route:?}, {mime:?}, include_bytes!({file:?})),\n"
        ));
    }
    generated.push_str("];\n");
    let out = PathBuf::from(std::env::var("OUT_DIR").unwrap()).join("web_assets.rs");
    fs::write(out, generated).unwrap();
}

/// Cargo shows a build script nothing but its `cargo:warning=` lines, so every line the
/// web build prints is relayed as one. Without this the Vite build is silent, and a
/// person waiting on `cargo build` cannot tell a slow bundle from a hung one.
fn run(root: &Path, program: &str, args: &[&str], version: &str) {
    warn(&format!("{program} {}", args.join(" ")));
    let output = Command::new(program)
        .args(args)
        .current_dir(root)
        .env("CENTINEL_VERSION", version)
        .env("NO_COLOR", "1")
        .output()
        .unwrap_or_else(|error| {
            panic!("could not start `{program}`: {error}. Install Node.js, then run `npm ci` and `npm run build`")
        });
    for line in String::from_utf8_lossy(&output.stdout)
        .lines()
        .chain(String::from_utf8_lossy(&output.stderr).lines())
    {
        let line = line.trim_end();
        if !line.trim().is_empty() {
            warn(&format!("  {line}"));
        }
    }
    if !output.status.success() {
        panic!(
            "`{program} {}` failed. Run `npm ci` and `npm run build` before building Centinel",
            args.join(" ")
        );
    }
}

fn warn(line: &str) {
    println!("cargo:warning=web: {line}");
}

/// The version the Start root route stamped into the page head.
fn stamped_version(html: &str) -> Option<String> {
    let start = html.find("name=\"centinel-version\"")?;
    let rest = &html[start..];
    let content = rest.find("content=\"")? + "content=\"".len();
    let rest = &rest[content..];
    let end = rest.find('"')?;
    Some(rest[..end].to_string())
}

fn modified(path: &Path) -> Option<std::time::SystemTime> {
    fs::metadata(path).and_then(|m| m.modified()).ok()
}

fn require_node(root: &Path) {
    let output = Command::new("node")
        .arg("--version")
        .current_dir(root)
        .output()
        .unwrap_or_else(|error| {
            panic!("could not start `node`: {error}. Install Node.js 22.12 or newer")
        });
    let version = String::from_utf8_lossy(&output.stdout);
    let parts: Vec<u64> = version
        .trim()
        .trim_start_matches('v')
        .split('.')
        .filter_map(|p| p.parse().ok())
        .collect();
    let supported = matches!(parts.as_slice(), [major, minor, ..] if *major >= 22 && (*major > 22 || *minor >= 12));
    if !output.status.success() || !supported {
        panic!(
            "Centinel web requires Node.js 22.12+ to build; found `{}`",
            version.trim()
        );
    }
}
