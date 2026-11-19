//! Writing Altium's PcbDoc: an OLE compound file holding one storage per object kind, each
//! with a `Header` (record count) and a `Data` stream. Lengths in Altium's internal unit of
//! 0.1 microinch (2.54 nm), y pointing up.

use std::collections::BTreeMap;
use std::io::Write;

/// Altium's internal unit is 1/10000 mil = 2.54 nm
pub fn units(nm: i64) -> i32 {
    (nm as f64 / 2.54)
        .round()
        .clamp(i32::MIN as f64, i32::MAX as f64) as i32
}

/// A length as a property value: "12.5mil"
pub fn mil(nm: i64) -> String {
    let m = units(nm) as f64 / 10000.0;
    let s = format!("{:.4}", m);
    let s = s.trim_end_matches('0').trim_end_matches('.');
    format!("{}mil", if s.is_empty() || s == "-" { "0" } else { s })
}

#[derive(Default)]
pub struct Buf(pub Vec<u8>);

impl Buf {
    pub fn u8(&mut self, v: u8) -> &mut Self {
        self.0.push(v);
        self
    }
    pub fn u16(&mut self, v: u16) -> &mut Self {
        self.0.extend(v.to_le_bytes());
        self
    }
    pub fn u32(&mut self, v: u32) -> &mut Self {
        self.0.extend(v.to_le_bytes());
        self
    }
    pub fn i32(&mut self, v: i32) -> &mut Self {
        self.0.extend(v.to_le_bytes());
        self
    }
    pub fn f64(&mut self, v: f64) -> &mut Self {
        self.0.extend(v.to_le_bytes());
        self
    }
    pub fn bytes(&mut self, b: &[u8]) -> &mut Self {
        self.0.extend_from_slice(b);
        self
    }
    /// A length-prefixed block: subrecords and property lists
    pub fn block(&mut self, b: &[u8]) -> &mut Self {
        self.u32(b.len() as u32).bytes(b)
    }
    /// One-byte length then Latin-1 bytes, at most 255 of them
    pub fn pascal(&mut self, s: &str) -> &mut Self {
        let b: Vec<u8> = latin1(s).into_iter().take(255).collect();
        self.u8(b.len() as u8).bytes(&b)
    }
}

/// Overwrite fields of a template record at given offsets
pub struct Patch<'a>(pub &'a mut [u8]);

impl Patch<'_> {
    pub fn u8(&mut self, at: usize, v: u8) -> &mut Self {
        self.0[at] = v;
        self
    }
    pub fn u16(&mut self, at: usize, v: u16) -> &mut Self {
        self.0[at..at + 2].copy_from_slice(&v.to_le_bytes());
        self
    }
    pub fn u32(&mut self, at: usize, v: u32) -> &mut Self {
        self.0[at..at + 4].copy_from_slice(&v.to_le_bytes());
        self
    }
    pub fn i32(&mut self, at: usize, v: i32) -> &mut Self {
        self.0[at..at + 4].copy_from_slice(&v.to_le_bytes());
        self
    }
    pub fn f64(&mut self, at: usize, v: f64) -> &mut Self {
        self.0[at..at + 8].copy_from_slice(&v.to_le_bytes());
        self
    }
    pub fn bytes(&mut self, at: usize, b: &[u8]) -> &mut Self {
        self.0[at..at + b.len()].copy_from_slice(b);
        self
    }
}

/// Characters outside Latin-1 become '?' (the %UTF8% twin of the key carries them)
pub fn latin1(s: &str) -> Vec<u8> {
    s.chars()
        .map(|c| if (c as u32) < 256 { c as u8 } else { b'?' })
        .collect()
}

/// Altium property list: |KEY=VALUE|KEY=VALUE... and a closing NUL, length-prefixed
#[derive(Default)]
pub struct Props(Vec<(String, String)>);

impl Props {
    pub fn new() -> Props {
        Props(Vec::new())
    }
    pub fn set(&mut self, k: &str, v: impl Into<String>) -> &mut Self {
        let v: String = v.into();
        // '|' separates the fields, so Altium stores it in a value as a broken bar
        let v = v.replace('|', "\u{a6}");
        if v.chars().any(|c| c as u32 >= 256) {
            self.0.push((format!("%UTF8%{k}"), v.clone()));
        }
        self.0.push((k.to_string(), v));
        self
    }
    pub fn bool(&mut self, k: &str, v: bool) -> &mut Self {
        self.set(k, if v { "TRUE" } else { "FALSE" })
    }
    pub fn len(&mut self, k: &str, nm: i64) -> &mut Self {
        self.set(k, mil(nm))
    }
    pub fn num(&mut self, k: &str, v: impl std::fmt::Display) -> &mut Self {
        self.set(k, v.to_string())
    }
    pub fn encode(&self) -> Vec<u8> {
        let mut s = Vec::new();
        for (k, v) in &self.0 {
            s.push(b'|');
            s.extend(k.as_bytes());
            s.push(b'=');
            if k.starts_with("%UTF8%") {
                s.extend(v.as_bytes());
            } else {
                s.extend(latin1(v));
            }
        }
        s.push(0);
        let mut b = Buf::default();
        b.block(&s);
        b.0
    }
}

/// One storage of the compound file: a record count and the records
#[derive(Default)]
pub struct Storage {
    pub count: u32,
    pub data: Vec<u8>,
}

impl Storage {
    pub fn push(&mut self, rec: &[u8]) {
        self.count += 1;
        self.data.extend_from_slice(rec);
    }
}

#[derive(Default)]
pub struct Doc {
    pub storages: BTreeMap<String, Storage>,
    pub streams: BTreeMap<String, Vec<u8>>,
}

impl Doc {
    pub fn storage(&mut self, name: &str) -> &mut Storage {
        self.storages.entry(name.to_string()).or_default()
    }

    pub fn write(&self, path: &std::path::Path) -> std::io::Result<()> {
        let mut cf = cfb::create(path)?;
        for (name, data) in &self.streams {
            cf.create_stream(format!("/{name}"))?.write_all(data)?;
        }
        for (name, st) in &self.storages {
            cf.create_storage(format!("/{name}"))?;
            cf.create_stream(format!("/{name}/Header"))?
                .write_all(&st.count.to_le_bytes())?;
            cf.create_stream(format!("/{name}/Data"))?
                .write_all(&st.data)?;
        }
        cf.flush()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn units_and_mils() {
        assert_eq!(units(254), 100);
        assert_eq!(mil(254_000), "10mil");
        assert_eq!(mil(100_000), "3.937mil");
        assert_eq!(mil(0), "0mil");
    }

    #[test]
    fn props_are_length_prefixed_and_nul_terminated() {
        let mut p = Props::new();
        p.set("NAME", "GND").set("X", "a|b");
        let e = p.encode();
        assert_eq!(
            u32::from_le_bytes(e[0..4].try_into().unwrap()) as usize,
            e.len() - 4
        );
        assert_eq!(&e[4..], b"|NAME=GND|X=a\xa6b\0");
    }
}
