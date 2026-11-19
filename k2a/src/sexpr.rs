//! A small reader for KiCad's S-expression files.

use std::fmt;

#[derive(Debug, Clone, PartialEq)]
pub enum Sexpr {
    /// A bare word or number, e.g. `segment`, `0.25`, `yes`
    Atom(String),
    /// A quoted string, unescaped
    Str(String),
    List(Vec<Sexpr>),
}

#[derive(Debug)]
pub struct ParseError {
    pub offset: usize,
    pub message: String,
}

impl fmt::Display for ParseError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "{} at byte {}", self.message, self.offset)
    }
}

impl std::error::Error for ParseError {}

pub fn parse(text: &str) -> Result<Sexpr, ParseError> {
    let mut p = Parser {
        s: text.as_bytes(),
        i: 0,
    };
    p.ws();
    let v = p.value()?;
    p.ws();
    if p.i != p.s.len() {
        return Err(p.err("trailing data"));
    }
    Ok(v)
}

struct Parser<'a> {
    s: &'a [u8],
    i: usize,
}

impl Parser<'_> {
    fn err(&self, m: &str) -> ParseError {
        ParseError {
            offset: self.i,
            message: m.to_string(),
        }
    }

    fn ws(&mut self) {
        while self.i < self.s.len() && self.s[self.i].is_ascii_whitespace() {
            self.i += 1;
        }
    }

    fn value(&mut self) -> Result<Sexpr, ParseError> {
        match self.s.get(self.i) {
            None => Err(self.err("unexpected end")),
            Some(b'(') => {
                // Iterative descent would be safer for absurd nesting; KiCad never goes deep
                self.i += 1;
                let mut items = Vec::new();
                loop {
                    self.ws();
                    match self.s.get(self.i) {
                        None => return Err(self.err("unclosed list")),
                        Some(b')') => {
                            self.i += 1;
                            return Ok(Sexpr::List(items));
                        }
                        _ => items.push(self.value()?),
                    }
                }
            }
            Some(b')') => Err(self.err("unexpected ')'")),
            Some(b'"') => {
                self.i += 1;
                let mut out = Vec::new();
                loop {
                    match self.s.get(self.i) {
                        None => return Err(self.err("unclosed string")),
                        Some(b'"') => {
                            self.i += 1;
                            break;
                        }
                        Some(b'\\') => {
                            let c = *self
                                .s
                                .get(self.i + 1)
                                .ok_or_else(|| self.err("bad escape"))?;
                            out.push(match c {
                                b'n' => b'\n',
                                b't' => b'\t',
                                b'r' => b'\r',
                                other => other,
                            });
                            self.i += 2;
                        }
                        Some(&c) => {
                            out.push(c);
                            self.i += 1;
                        }
                    }
                }
                String::from_utf8(out)
                    .map(Sexpr::Str)
                    .map_err(|_| self.err("string is not UTF-8"))
            }
            Some(_) => {
                let start = self.i;
                while self.i < self.s.len() {
                    let c = self.s[self.i];
                    if c.is_ascii_whitespace() || c == b'(' || c == b')' || c == b'"' {
                        break;
                    }
                    self.i += 1;
                }
                Ok(Sexpr::Atom(
                    String::from_utf8_lossy(&self.s[start..self.i]).into_owned(),
                ))
            }
        }
    }
}

impl Sexpr {
    /// The head word of a list: `(segment ...)` -> "segment"
    pub fn head(&self) -> Option<&str> {
        match self {
            Sexpr::List(v) => match v.first() {
                Some(Sexpr::Atom(a)) => Some(a),
                _ => None,
            },
            _ => None,
        }
    }

    pub fn items(&self) -> &[Sexpr] {
        match self {
            Sexpr::List(v) => v,
            _ => &[],
        }
    }

    /// Everything after the head word
    pub fn args(&self) -> &[Sexpr] {
        let v = self.items();
        if v.is_empty() {
            v
        } else {
            &v[1..]
        }
    }

    /// The text of an atom or string
    pub fn text(&self) -> Option<&str> {
        match self {
            Sexpr::Atom(s) | Sexpr::Str(s) => Some(s),
            Sexpr::List(_) => None,
        }
    }

    pub fn num(&self) -> Option<f64> {
        self.text()?.parse().ok()
    }

    /// The first child list with this head
    pub fn child(&self, name: &str) -> Option<&Sexpr> {
        self.args().iter().find(|c| c.head() == Some(name))
    }

    /// Every child list with this head
    pub fn children<'a>(&'a self, name: &'a str) -> impl Iterator<Item = &'a Sexpr> + 'a {
        self.args().iter().filter(move |c| c.head() == Some(name))
    }

    /// `(name a b ...)` -> the n-th argument's text
    pub fn arg_text(&self, name: &str, n: usize) -> Option<&str> {
        self.child(name)?.args().get(n)?.text()
    }

    pub fn arg_num(&self, name: &str, n: usize) -> Option<f64> {
        self.child(name)?.args().get(n)?.num()
    }

    /// Whether a bare flag word is among the arguments: `(pad "1" smd rect locked ...)`
    pub fn has_atom(&self, word: &str) -> bool {
        self.args()
            .iter()
            .any(|a| matches!(a, Sexpr::Atom(s) if s == word))
    }

    /// `(name yes)` / `(name no)` / bare `(name)`
    pub fn flag(&self, name: &str) -> Option<bool> {
        let c = self.child(name)?;
        Some(match c.args().first().and_then(|a| a.text()) {
            None => true,
            Some(t) => t == "yes" || t == "true",
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_nested_lists_and_strings() {
        let v = parse(r#"(kicad_pcb (version 20260206) (net "a \"b\"") (at 1.5 -2))"#).unwrap();
        assert_eq!(v.head(), Some("kicad_pcb"));
        assert_eq!(v.arg_num("version", 0), Some(20260206.0));
        assert_eq!(v.arg_text("net", 0), Some("a \"b\""));
        assert_eq!(v.arg_num("at", 1), Some(-2.0));
    }

    #[test]
    fn rejects_unbalanced() {
        assert!(parse("(a (b)").is_err());
        assert!(parse("(a))").is_err());
    }
}
