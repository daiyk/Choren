//! libqi type signatures: `b c C w W i I l L f d s r m o v X`, `[T]`, `{KV}`, `(T…)<Name,f…>`.

#[derive(Clone, Debug, PartialEq)]
pub enum Sig {
    Void,
    Bool,
    Int(u8, bool), // byte width, signed
    Float,
    Double,
    Str,
    Raw,
    Dynamic,
    Object,
    Unknown,
    List(Box<Sig>),
    Map(Box<Sig>, Box<Sig>),
    /// Fields, plus an optional `<Name,field…>` annotation.
    Tuple(Vec<Sig>, Option<String>),
    VarArgs(Box<Sig>),
}

impl Sig {
    pub fn parse(s: &str) -> Result<Sig, String> {
        let (sig, rest) = parse_one(s.as_bytes()).map_err(|e| format!("bad signature {s:?}: {e}"))?;
        if !rest.is_empty() {
            return Err(format!("bad signature {s:?}: trailing {:?}", String::from_utf8_lossy(rest)));
        }
        Ok(sig)
    }

    /// Parses a parameter list such as `(sI)` into its element signatures.
    pub fn params(s: &str) -> Result<Vec<Sig>, String> {
        match Sig::parse(s)? {
            Sig::Tuple(f, _) => Ok(f),
            other => Ok(vec![other]),
        }
    }

    pub fn tuple(fields: Vec<Sig>) -> Sig {
        Sig::Tuple(fields, None)
    }
}

impl std::fmt::Display for Sig {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        match self {
            Sig::Void => f.write_str("v"),
            Sig::Bool => f.write_str("b"),
            Sig::Int(w, s) => f.write_str(match (w, s) {
                (1, true) => "c",
                (1, false) => "C",
                (2, true) => "w",
                (2, false) => "W",
                (4, true) => "i",
                (4, false) => "I",
                (8, true) => "l",
                _ => "L",
            }),
            Sig::Float => f.write_str("f"),
            Sig::Double => f.write_str("d"),
            Sig::Str => f.write_str("s"),
            Sig::Raw => f.write_str("r"),
            Sig::Dynamic => f.write_str("m"),
            Sig::Object => f.write_str("o"),
            Sig::Unknown => f.write_str("X"),
            Sig::List(t) => write!(f, "[{t}]"),
            Sig::Map(k, v) => write!(f, "{{{k}{v}}}"),
            Sig::VarArgs(t) => write!(f, "#{t}"),
            Sig::Tuple(fs, name) => {
                f.write_str("(")?;
                for s in fs {
                    write!(f, "{s}")?;
                }
                f.write_str(")")?;
                if let Some(n) = name {
                    write!(f, "<{n}>")?;
                }
                Ok(())
            }
        }
    }
}

fn parse_one(b: &[u8]) -> Result<(Sig, &[u8]), String> {
    let (&c, rest) = b.split_first().ok_or("unexpected end")?;
    let sig = match c {
        b'v' => Sig::Void,
        b'b' => Sig::Bool,
        b'c' => Sig::Int(1, true),
        b'C' => Sig::Int(1, false),
        b'w' => Sig::Int(2, true),
        b'W' => Sig::Int(2, false),
        b'i' => Sig::Int(4, true),
        b'I' => Sig::Int(4, false),
        b'l' => Sig::Int(8, true),
        b'L' => Sig::Int(8, false),
        b'f' => Sig::Float,
        b'd' => Sig::Double,
        b's' => Sig::Str,
        b'r' => Sig::Raw,
        b'm' => Sig::Dynamic,
        b'o' => Sig::Object,
        b'X' => Sig::Unknown,
        b'#' | b'+' => {
            let (t, rest) = parse_one(rest)?;
            return Ok((Sig::VarArgs(Box::new(t)), rest));
        }
        b'[' => {
            let (t, rest) = parse_one(rest)?;
            let rest = rest.strip_prefix(b"]").ok_or("missing ]")?;
            return Ok((Sig::List(Box::new(t)), rest));
        }
        b'{' => {
            let (k, rest) = parse_one(rest)?;
            let (v, rest) = parse_one(rest)?;
            let rest = rest.strip_prefix(b"}").ok_or("missing }")?;
            return Ok((Sig::Map(Box::new(k), Box::new(v)), rest));
        }
        b'(' => {
            let mut fields = vec![];
            let mut rest = rest;
            while rest.first() != Some(&b')') {
                let (t, r) = parse_one(rest)?;
                fields.push(t);
                rest = r;
            }
            let mut rest = &rest[1..];
            let mut name = None;
            if rest.first() == Some(&b'<') {
                // Struct names can carry template arguments: `<ValueConfidence<float>,value,confidence>`.
                let mut depth = 0;
                let end = rest
                    .iter()
                    .position(|&x| {
                        depth += match x { b'<' => 1, b'>' => -1, _ => 0 };
                        depth == 0
                    })
                    .ok_or("missing >")?;
                name = Some(String::from_utf8_lossy(&rest[1..end]).into_owned());
                rest = &rest[end + 1..];
            }
            return Ok((Sig::Tuple(fields, name), rest));
        }
        other => return Err(format!("unknown type {:?}", other as char)),
    };
    Ok((sig, rest))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn round_trip() {
        for s in ["v", "(sI)", "[m]", "{sm}", "(I(Iss)<MetaSignal,uid,name,signature>)", "[(ss)<P,name,description>]", "L", "(ff[f])", "((ff)<ValueConfidence<float>,value,confidence>f)"] {
            assert_eq!(Sig::parse(s).unwrap().to_string(), s);
        }
        assert!(Sig::parse("(s").is_err());
        assert!(Sig::parse("q").is_err());
    }
}
