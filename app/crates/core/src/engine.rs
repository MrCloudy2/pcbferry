//! The converter engine: pcbferry's patched kicad-cli, downloaded once per version.
//!
//! KiCad's own Altium importer, with pcbferry's patches, does the Altium -> KiCad conversion;
//! each release publishes it built for Linux, macOS and Windows, with `engine.json` naming
//! each platform's file and its SHA-256. The KiCad plugin downloads the same engine into the
//! same folder, so the two share it.
//!
//! Environment: `KICAD_CLI` a kicad-cli to use instead; `PCBFERRY_ENGINE_DIR` an unpacked
//! engine (its `pcbferry-engine` folder) to use instead.

use sha2::{Digest, Sha256};
use std::io::{Read, Write};
use std::path::{Component, Path, PathBuf};

pub const VERSION: &str = env!("CARGO_PKG_VERSION");
pub const REPO: &str = "https://github.com/MrCloudy2/pcbferry";

/// `linux-x86_64`, `macos-arm64`, `windows-x86_64`, ...
pub fn platform_key() -> String {
    let arch = match std::env::consts::ARCH {
        "aarch64" => "arm64",
        a => a,
    };
    format!("{}-{arch}", std::env::consts::OS)
}

/// Where pcbferry keeps its engine (the plugin uses the same folder)
pub fn data_dir() -> PathBuf {
    let home = || {
        std::env::var_os("HOME")
            .map(PathBuf::from)
            .unwrap_or_default()
    };
    let base = if cfg!(windows) {
        std::env::var_os("LOCALAPPDATA")
            .map(PathBuf::from)
            .unwrap_or_else(|| home().join("AppData").join("Local"))
    } else if cfg!(target_os = "macos") {
        home().join("Library").join("Application Support")
    } else {
        std::env::var_os("XDG_DATA_HOME")
            .map(PathBuf::from)
            .filter(|p| p.is_absolute())
            .unwrap_or_else(|| home().join(".local").join("share"))
    };
    base.join("pcbferry")
}

fn exe(name: &str) -> String {
    if cfg!(windows) {
        format!("{name}.exe")
    } else {
        name.to_string()
    }
}

/// The kicad-cli inside an unpacked engine (`root` holds `pcbferry-engine/`)
pub fn cli_in(root: &Path) -> PathBuf {
    root.join("pcbferry-engine")
        .join("bin")
        .join(exe("kicad-cli"))
}

/// A kicad-cli that can run pcbferry's conversions
#[derive(Debug, Clone)]
pub struct Engine {
    pub cli: PathBuf,
    /// Downloaded by pcbferry (rather than one the user pointed us at)
    pub bundled: bool,
}

/// The engine to use, if there is one: `KICAD_CLI`, `PCBFERRY_ENGINE_DIR`, then the one
/// downloaded for this version
pub fn find() -> Option<Engine> {
    if let Some(cli) = std::env::var_os("KICAD_CLI").filter(|s| !s.is_empty()) {
        let cli = PathBuf::from(cli);
        return Some(Engine {
            cli,
            bundled: false,
        });
    }
    if let Some(dir) = std::env::var_os("PCBFERRY_ENGINE_DIR") {
        let cli = PathBuf::from(dir).join("bin").join(exe("kicad-cli"));
        return cli.is_file().then_some(Engine { cli, bundled: true });
    }
    installed()
}

pub fn installed() -> Option<Engine> {
    let cli = cli_in(&data_dir().join(format!("engine-{VERSION}")));
    cli.is_file().then_some(Engine { cli, bundled: true })
}

/// One platform's engine in a release
#[derive(Debug, Clone)]
pub struct Download {
    pub url: String,
    pub sha256: String,
    pub size: u64,
}

/// This platform's engine for this version, from the release's engine.json
pub fn download_info() -> Result<Download, String> {
    let url = format!("{REPO}/releases/download/v{VERSION}/engine.json");
    let text = match std::env::var_os("PCBFERRY_ENGINE_MANIFEST") {
        Some(p) => std::fs::read_to_string(&p).map_err(|e| format!("{}: {e}", p.display()))?,
        None => http()
            .get(&url)
            .call()
            .map_err(|e| format!("couldn't reach GitHub for the engine list ({e})"))?
            .body_mut()
            .read_to_string()
            .map_err(|e| e.to_string())?,
    };
    manifest_entry(&text, &platform_key())
}

fn manifest_entry(text: &str, key: &str) -> Result<Download, String> {
    let m: serde_json::Value =
        serde_json::from_str(text).map_err(|e| format!("engine.json: {e}"))?;
    let files = m["files"].as_object().ok_or("engine.json lists no files")?;
    let Some(f) = files.get(key) else {
        let mut have: Vec<&str> = files.keys().map(String::as_str).collect();
        have.sort();
        return Err(format!(
            "no pcbferry engine for {key} yet (available: {})",
            if have.is_empty() {
                "none".to_string()
            } else {
                have.join(", ")
            }
        ));
    };
    let base = m["base_url"].as_str().unwrap_or_default();
    Ok(Download {
        url: format!(
            "{base}{}",
            f["name"].as_str().ok_or("engine.json: no file name")?
        ),
        sha256: f["sha256"]
            .as_str()
            .ok_or("engine.json: no checksum")?
            .to_lowercase(),
        size: f["size"].as_u64().unwrap_or(0),
    })
}

fn http() -> ureq::Agent {
    ureq::Agent::config_builder()
        .timeout_connect(Some(std::time::Duration::from_secs(30)))
        .build()
        .into()
}

/// Download, check and unpack the engine; returns it. `progress(done, total)` is called as
/// bytes arrive.
pub fn install(info: &Download, progress: &dyn Fn(u64, u64)) -> Result<Engine, String> {
    let root = data_dir();
    std::fs::create_dir_all(&root).map_err(|e| format!("{}: {e}", root.display()))?;
    let target = root.join(format!("engine-{VERSION}"));
    let tmp = root.join(format!("download-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).map_err(|e| e.to_string())?;
    let result = (|| {
        let archive = tmp.join("engine.tar.xz");
        fetch(info, &archive, progress)?;
        let unpacked = tmp.join("unpacked");
        unpack(&archive, &unpacked)?;
        let _ = std::fs::remove_dir_all(&target);
        std::fs::rename(&unpacked, &target).map_err(|e| format!("{}: {e}", target.display()))
    })();
    let _ = std::fs::remove_dir_all(&tmp);
    result?;
    // Earlier versions
    if let Ok(rd) = std::fs::read_dir(&root) {
        for e in rd.flatten() {
            let name = e.file_name().to_string_lossy().to_string();
            if name.starts_with("engine-") && e.path() != target {
                let _ = std::fs::remove_dir_all(e.path());
            }
        }
    }
    let cli = cli_in(&target);
    if !cli.is_file() {
        return Err("the engine archive holds no kicad-cli".into());
    }
    Ok(Engine { cli, bundled: true })
}

fn fetch(info: &Download, to: &Path, progress: &dyn Fn(u64, u64)) -> Result<(), String> {
    let mut resp = http()
        .get(&info.url)
        .call()
        .map_err(|e| format!("download failed: {e}"))?;
    let mut body = resp.body_mut().with_config().limit(u64::MAX).reader();
    let mut out = std::fs::File::create(to).map_err(|e| e.to_string())?;
    let mut digest = Sha256::new();
    let mut buf = vec![0u8; 1 << 20];
    let mut done = 0u64;
    loop {
        let n = body
            .read(&mut buf)
            .map_err(|e| format!("download failed: {e}"))?;
        if n == 0 {
            break;
        }
        out.write_all(&buf[..n]).map_err(|e| e.to_string())?;
        digest.update(&buf[..n]);
        done += n as u64;
        progress(done, info.size);
    }
    let got: String = digest
        .finalize()
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect();
    if got != info.sha256 {
        return Err("the downloaded engine doesn't match its checksum; try again".into());
    }
    Ok(())
}

/// Unpack an engine archive, refusing anything outside `pcbferry-engine/` and anything but
/// plain files and folders
pub fn unpack(archive: &Path, dest: &Path) -> Result<(), String> {
    let file = std::fs::File::open(archive).map_err(|e| e.to_string())?;
    let xz = lzma_rust2::XzReader::new(std::io::BufReader::new(file), true);
    let mut tar = tar::Archive::new(xz);
    tar.set_preserve_permissions(true);
    std::fs::create_dir_all(dest).map_err(|e| e.to_string())?;
    for entry in tar.entries().map_err(|e| format!("engine archive: {e}"))? {
        let mut entry = entry.map_err(|e| format!("engine archive: {e}"))?;
        let path = entry.path().map_err(|e| e.to_string())?.into_owned();
        let kind = entry.header().entry_type();
        let first = path.components().next();
        let safe = path
            .components()
            .all(|c| matches!(c, Component::Normal(_) | Component::CurDir))
            && matches!(first, Some(Component::Normal(p)) if p == "pcbferry-engine")
            && (kind.is_file() || kind.is_dir());
        if !safe {
            // pax and GNU long-name headers are read by the tar crate itself
            if kind.is_pax_global_extensions() || kind.is_pax_local_extensions() {
                continue;
            }
            return Err(format!(
                "unexpected entry in the engine archive: {}",
                path.display()
            ));
        }
        let to = dest.join(&path);
        if kind.is_dir() {
            std::fs::create_dir_all(&to).map_err(|e| e.to_string())?;
        } else {
            if let Some(parent) = to.parent() {
                std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
            }
            entry
                .unpack(&to)
                .map_err(|e| format!("{}: {e}", to.display()))?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn manifest() {
        let m = r#"{"version": "0.2.0", "base_url": "https://x/v0.2.0/", "files": {
            "linux-x86_64": {"name": "e-linux.tar.xz", "sha256": "AB", "size": 5}}}"#;
        let d = manifest_entry(m, "linux-x86_64").unwrap();
        assert_eq!(d.url, "https://x/v0.2.0/e-linux.tar.xz");
        assert_eq!(d.sha256, "ab");
        assert!(manifest_entry(m, "macos-arm64")
            .unwrap_err()
            .contains("linux-x86_64"));
    }
}
