use std::fs;
use std::path::Path;

enum ArchFmt { TarBz2, Tar, SevenZ }

struct Dep {
    repo: &'static str,
    asset_contains: &'static str,
    archive_name: &'static str,
    format: ArchFmt,
    marker: &'static str,       // skip download if this exists
    rename_from: Option<&'static str>, // gbe extracts to "release/", rename it
}

const DEPS: &[Dep] = &[
    Dep {
        repo: "Detanup01/gbe_fork",
        asset_contains: "emu-linux-release.tar.bz2",
        archive_name: "emu-linux-release.tar.bz2",
        format: ArchFmt::TarBz2,
        marker: "gbe-linux/regular/x64/steamclient.so",
        rename_from: Some("gbe-linux"),
    },
    Dep {
        repo: "Detanup01/gbe_fork",
        asset_contains: "emu-win-release.7z",
        archive_name: "emu-win-release.7z",
        format: ArchFmt::SevenZ,
        marker: "gbe-win/steamclient_experimental/steamclient.dll",
        rename_from: Some("gbe-win"),
    },
    Dep {
        repo: "Open-Wine-Components/umu-launcher",
        asset_contains: "umu-launcher-",
        archive_name: "umu-launcher-latest-zipapp.tar",
        format: ArchFmt::Tar,
        marker: "umu/umu-run",
        rename_from: None,
    },
];

// (src relative to project root, dst relative to target dir)
const BUNDLE: &[(&str, &str)] = &[
    // goldberg linux
    ("deps/gbe-linux/regular/x64/steamclient.so", "res/goldberg/linux64/steamclient.so"),
    ("deps/gbe-linux/regular/x32/steamclient.so", "res/goldberg/linux32/steamclient.so"),
    // goldberg windows
    ("deps/gbe-win/steamclient_experimental/steamclient.dll", "res/goldberg/win/steamclient.dll"),
    ("deps/gbe-win/steamclient_experimental/steamclient64.dll", "res/goldberg/win/steamclient64.dll"),
    ("deps/gbe-win/steamclient_experimental/GameOverlayRenderer.dll", "res/goldberg/win/GameOverlayRenderer.dll"),
    ("deps/gbe-win/steamclient_experimental/GameOverlayRenderer64.dll", "res/goldberg/win/GameOverlayRenderer64.dll"),
    // umu
    ("deps/umu/umu-run", "bin/umu-run"),
    // resources
    ("res/splitscreen_kwin.js", "res/splitscreen_kwin.js"),
    ("res/splitscreen_kwin_vertical.js", "res/splitscreen_kwin_vertical.js"),
];

const BUNDLE_OPTIONAL: &[(&str, &str)] = &[
    ("deps/gamescope/build-gcc/src/gamescope", "bin/gamescope-kbm"),
];

fn main() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let deps_dir = root.join("deps");
    fs::create_dir_all(&deps_dir).expect("failed to create deps/");

    for dep in DEPS {
        fetch_dep(&deps_dir, dep).unwrap_or_else(|e| {
            panic!("failed to fetch {} from {}: {e}", dep.asset_contains, dep.repo);
        });
    }

    // cargo puts OUT_DIR a few levels deep, walk up to the profile dir (target/release/)
    let target_dir = Path::new(&std::env::var("OUT_DIR").unwrap())
        .ancestors().nth(3).unwrap().to_path_buf();

    for &(src, dst) in BUNDLE {
        let from = root.join(src);
        let to = target_dir.join(dst);
        fs::create_dir_all(to.parent().unwrap()).unwrap();
        fs::copy(&from, &to).unwrap_or_else(|e| {
            panic!("copy {} -> {}: {e}", from.display(), to.display());
        });
    }

    for &(src, dst) in BUNDLE_OPTIONAL {
        let from = root.join(src);
        if from.exists() {
            let to = target_dir.join(dst);
            fs::create_dir_all(to.parent().unwrap()).unwrap();
            let _ = fs::copy(&from, &to);
        }
    }
}

fn fetch_dep(deps_dir: &Path, dep: &Dep) -> Result<(), Box<dyn std::error::Error>> {
    if deps_dir.join(dep.marker).exists() {
        return Ok(());
    }

    let url = find_release_asset(dep.repo, dep.asset_contains)?;
    let archive = deps_dir.join(dep.archive_name);
    download(&url, &archive)?;

    let _ = fs::remove_dir_all(deps_dir.join("release"));
    if let Some(name) = dep.rename_from {
        let _ = fs::remove_dir_all(deps_dir.join(name));
    }

    match dep.format {
        ArchFmt::TarBz2 => {
            let f = fs::File::open(&archive)?;
            tar::Archive::new(bzip2::read::BzDecoder::new(f)).unpack(deps_dir)?;
        }
        ArchFmt::Tar => {
            let f = fs::File::open(&archive)?;
            tar::Archive::new(f).unpack(deps_dir)?;
        }
        ArchFmt::SevenZ => {
            sevenz_rust::decompress_file(&archive, deps_dir)?;
        }
    }

    if let Some(name) = dep.rename_from {
        fs::rename(deps_dir.join("release"), deps_dir.join(name))?;
    }
    fs::remove_file(&archive)?;
    Ok(())
}

fn find_release_asset(repo: &str, name_contains: &str) -> Result<String, Box<dyn std::error::Error>> {
    let client = reqwest::blocking::Client::new();
    let resp: serde_json::Value = client
        .get(format!("https://api.github.com/repos/{repo}/releases/latest"))
        .header("User-Agent", "partydeck-build")
        .send()?.error_for_status()?.json()?;

    for asset in resp["assets"].as_array().ok_or("no assets in release")? {
        let name = asset["name"].as_str().unwrap_or("");
        if name.contains(name_contains) {
            return Ok(asset["browser_download_url"].as_str()
                .ok_or("missing download url")?.to_string());
        }
    }
    Err(format!("no asset matching '{name_contains}' in {repo}").into())
}

fn download(url: &str, dest: &Path) -> Result<(), Box<dyn std::error::Error>> {
    let client = reqwest::blocking::Client::new();
    let mut resp = client.get(url)
        .header("User-Agent", "partydeck-build")
        .send()?.error_for_status()?;
    let mut file = fs::File::create(dest)?;
    std::io::copy(&mut resp, &mut file)?;
    Ok(())
}
