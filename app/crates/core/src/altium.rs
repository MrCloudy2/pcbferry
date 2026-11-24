//! What pcbferry reads from Altium files itself: the documents a project lists.

use std::path::{Path, PathBuf};

/// The absolute path without `.` and `..` (kicad-cli's reports and the libraries it registers
/// repeat the paths it is given). Symbolic links stay as they are, and the path needn't exist.
pub fn resolve(path: &Path) -> std::io::Result<PathBuf> {
    use std::path::Component;
    let mut out = PathBuf::new();
    for part in std::path::absolute(path)?.components() {
        match part {
            Component::ParentDir => {
                out.pop();
            }
            Component::CurDir => {}
            p => out.push(p),
        }
    }
    Ok(out)
}

/// A file a .PrjPcb names, found as Windows would (case doesn't matter there)
pub fn find_case_insensitive(base: &Path, rel: &str) -> Option<PathBuf> {
    let mut path = base.to_path_buf();
    let rel = rel.replace('\\', "/");
    for part in Path::new(&rel).components() {
        use std::path::Component;
        match part {
            Component::ParentDir => {
                path.pop();
                continue;
            }
            Component::CurDir => continue,
            Component::Normal(p) => {
                let want = p.to_string_lossy().to_lowercase();
                let found = std::fs::read_dir(&path)
                    .ok()?
                    .flatten()
                    .find(|e| e.file_name().to_string_lossy().to_lowercase() == want)?;
                path = found.path();
            }
            // An absolute path from the PC the project was made on
            _ => return None,
        }
    }
    path.is_file().then_some(path)
}

/// The sections of an INI file and their keys, in file order (as Python's configparser reads
/// a .PrjPcb: later duplicates win)
pub fn ini(text: &str) -> Vec<(String, Vec<(String, String)>)> {
    let mut out: Vec<(String, Vec<(String, String)>)> = Vec::new();
    // Keys after a repeated section header go to the first one
    let mut current: Option<usize> = None;
    for line in text.lines() {
        let t = line.trim();
        if t.is_empty() || t.starts_with('#') || t.starts_with(';') {
            continue;
        }
        if let Some(name) = t.strip_prefix('[').and_then(|r| r.strip_suffix(']')) {
            current = Some(match out.iter().position(|(s, _)| s == name) {
                Some(i) => i,
                None => {
                    out.push((name.to_string(), Vec::new()));
                    out.len() - 1
                }
            });
            continue;
        }
        let Some(i) = current else { continue };
        let (k, v) = match t.find(['=', ':']) {
            Some(c) => (t[..c].trim(), t[c + 1..].trim()),
            None => (t, ""),
        };
        let keys = &mut out[i].1;
        match keys.iter_mut().find(|(key, _)| key == k) {
            Some(slot) => slot.1 = v.to_string(),
            None => keys.push((k.to_string(), v.to_string())),
        }
    }
    out
}

/// Older Altium writes the Windows code page; newer UTF-8 with a BOM
pub fn decode(raw: &[u8]) -> String {
    let raw = raw.strip_prefix(b"\xEF\xBB\xBF").unwrap_or(raw);
    match std::str::from_utf8(raw) {
        Ok(s) => s.to_string(),
        Err(_) => raw.iter().map(|&b| b as char).collect(),
    }
}

/// A document a project lists
#[derive(Debug, Clone)]
pub enum Listed {
    Found(PathBuf),
    /// The path as the project gives it
    Missing(String),
}

/// The documents a .PrjPcb lists, in order
pub fn project_documents(prjpcb: &Path) -> std::io::Result<Vec<Listed>> {
    let text = decode(&std::fs::read(prjpcb)?);
    let base = prjpcb.parent().unwrap_or(Path::new("."));
    let mut out = Vec::new();
    for (section, keys) in ini(&text) {
        if !section.starts_with("Document") {
            continue;
        }
        let rel = keys
            .iter()
            .find(|(k, _)| k == "DocumentPath")
            .map(|(_, v)| v.as_str())
            .unwrap_or("");
        if rel.is_empty() {
            continue;
        }
        out.push(match find_case_insensitive(base, rel) {
            Some(p) => Listed::Found(p),
            None => Listed::Missing(rel.to_string()),
        });
    }
    Ok(out)
}

fn has_ext(p: &Path, ext: &str) -> bool {
    p.extension().is_some_and(|e| e.eq_ignore_ascii_case(ext))
}

/// The boards a project lists that exist
pub fn project_boards(prjpcb: &Path) -> std::io::Result<Vec<PathBuf>> {
    Ok(project_documents(prjpcb)?
        .into_iter()
        .filter_map(|d| match d {
            Listed::Found(p) if has_ext(&p, "pcbdoc") => Some(p),
            _ => None,
        })
        .collect())
}

pub fn is(p: &Path, ext: &str) -> bool {
    has_ext(p, ext)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolves_parents() {
        let cwd = std::env::current_dir().unwrap();
        assert_eq!(
            resolve(Path::new("a/../b/./c")).unwrap(),
            cwd.join("b").join("c")
        );
    }

    #[test]
    fn reads_a_project() {
        let s = ini("[Design]\nVersion=1.0\n\n[Document1]\nDocumentPath=Sheet.SchDoc\nX = a:b\n[Document1]\nDocumentPath=Other.SchDoc\n");
        assert_eq!(s.len(), 2);
        let d1 = s.iter().find(|(n, _)| n == "Document1").unwrap();
        assert_eq!(d1.1[0], ("DocumentPath".into(), "Other.SchDoc".into()));
        assert_eq!(d1.1[1], ("X".into(), "a:b".into()));
        assert_eq!(decode(b"\xEF\xBB\xBFab"), "ab");
        assert_eq!(decode(b"\xb5m"), "\u{b5}m");
    }
}
