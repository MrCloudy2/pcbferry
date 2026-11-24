//! Running kicad-cli with its output streamed to a log, line by line.

use std::io::{BufRead, BufReader, Read};
use std::process::{Command, Stdio};

/// Where progress goes: the terminal for the command line, the window for the app
pub type Log<'a> = &'a (dyn Fn(String) + Sync);

/// Run `cmd`; every non-empty line it prints goes to `log`, indented. Returns the exit code
/// (None when it couldn't start or was killed by a signal).
pub fn run(mut cmd: Command, log: Log) -> Option<i32> {
    cmd.stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        cmd.creation_flags(CREATE_NO_WINDOW);
    }
    let mut child = match cmd.spawn() {
        Ok(c) => c,
        Err(e) => {
            log(format!(
                "pcbferry: error: couldn't start {}: {e}",
                cmd.get_program().to_string_lossy()
            ));
            return None;
        }
    };
    let out = child.stdout.take().unwrap();
    let err = child.stderr.take().unwrap();
    std::thread::scope(|s| {
        s.spawn(|| lines(out, log));
        s.spawn(|| lines(err, log));
    });
    child.wait().ok().and_then(|st| st.code())
}

// kicad-cli prints progress on stdout and wx noise on stderr; keep both visible.
fn lines(r: impl Read, log: Log) {
    let mut r = BufReader::new(r);
    let mut buf = Vec::new();
    loop {
        buf.clear();
        match r.read_until(b'\n', &mut buf) {
            Ok(0) | Err(_) => return,
            Ok(_) => {
                let line = String::from_utf8_lossy(&buf);
                let line = line.trim_end();
                if !line.trim().is_empty() {
                    log(format!("  {line}"));
                }
            }
        }
    }
}
