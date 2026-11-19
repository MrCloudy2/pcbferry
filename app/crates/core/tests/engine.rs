//! Unpacking a released engine archive: the pure-Rust xz and tar readers, the checks on what
//! the archive holds, and kicad-cli runnable afterwards (executable bits kept). Needs an
//! engine archive (PCBFERRY_TEST_ENGINE_ARCHIVE).

use std::path::PathBuf;

#[test]
fn unpacks_a_released_engine() {
    let Some(archive) = std::env::var_os("PCBFERRY_TEST_ENGINE_ARCHIVE").map(PathBuf::from) else {
        eprintln!("skipped: set PCBFERRY_TEST_ENGINE_ARCHIVE");
        return;
    };
    let dest = std::env::temp_dir().join(format!("pcbferry-unpack-{}", std::process::id()));
    let started = std::time::Instant::now();
    pcbferry_core::engine::unpack(&archive, &dest).unwrap();
    println!("unpacked in {:.1} s", started.elapsed().as_secs_f64());
    let cli = pcbferry_core::engine::cli_in(&dest);
    let out = std::process::Command::new(&cli)
        .arg("version")
        .output()
        .unwrap();
    let _ = std::fs::remove_dir_all(&dest);
    assert!(
        out.status.success(),
        "{}: {}",
        cli.display(),
        String::from_utf8_lossy(&out.stderr)
    );
    println!(
        "kicad-cli version: {}",
        String::from_utf8_lossy(&out.stdout).trim()
    );
}
