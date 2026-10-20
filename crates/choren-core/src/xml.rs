//! Lossless XML tree. Attribute values and text keep their raw (escaped) form and all
//! whitespace is kept, so an unmodified document writes back byte for byte.

pub type NodeId = usize;

#[derive(Clone, Debug)]
pub enum Kind {
    Element(Element),
    /// Raw escaped character data, whitespace included.
    Text(String),
    CData(String),
    /// Declaration, processing instruction, comment or doctype, verbatim.
    Other(String),
}

#[derive(Clone, Debug)]
pub struct Element {
    pub name: String,
    pub attrs: Vec<Attr>,
    /// Whitespace before `>` or `/>`, kept for byte-exact output.
    pub tail: String,
    pub children: Vec<NodeId>,
    /// Written as `<name ... />` while it has no children.
    pub empty: bool,
}

#[derive(Clone, Debug)]
pub struct Attr {
    /// Whitespace before the name.
    pub pre: String,
    pub key: String,
    /// Raw (escaped) value.
    pub raw: String,
    pub quote: char,
}

#[derive(Clone, Debug)]
pub struct Node {
    pub parent: Option<NodeId>,
    pub kind: Kind,
}

/// Arena-backed document. Removed nodes stay in the arena, so undo can re-attach them
/// and NodeIds stay stable for the whole session.
#[derive(Clone, Debug, Default)]
pub struct Doc {
    pub nodes: Vec<Node>,
    /// Prolog, root element and trailing nodes.
    pub top: Vec<NodeId>,
}

impl Doc {
    pub fn parse(src: &str) -> Result<Doc, String> {
        let mut doc = Doc::default();
        doc.parse_into(src, None)?;
        if doc.root().is_none() {
            return Err("XML document has no root element".into());
        }
        Ok(doc)
    }

    /// Parses `src` as children of `parent` (or top level) and returns the new top nodes.
    pub fn parse_into(&mut self, src: &str, parent: Option<NodeId>) -> Result<Vec<NodeId>, String> {
        let b = src.as_bytes();
        let mut stack: Vec<NodeId> = parent.into_iter().collect();
        let base = stack.len();
        let mut made = Vec::new();
        let mut i = 0;
        let err = |i: usize, m: &str| format!("XML error at byte {i}: {m}");
        while i < b.len() {
            let (kind, next) = if b[i] != b'<' {
                let end = src[i..].find('<').map_or(b.len(), |n| i + n);
                (Kind::Text(src[i..end].to_string()), end)
            } else if src[i..].starts_with("<![CDATA[") {
                let end = src[i..].find("]]>").ok_or_else(|| err(i, "unterminated CDATA"))? + i;
                (Kind::CData(src[i + 9..end].to_string()), end + 3)
            } else if src[i..].starts_with("<!--") {
                let end = src[i..].find("-->").ok_or_else(|| err(i, "unterminated comment"))? + i + 3;
                (Kind::Other(src[i..end].to_string()), end)
            } else if src[i..].starts_with("<?") {
                let end = src[i..].find("?>").ok_or_else(|| err(i, "unterminated declaration"))? + i + 2;
                (Kind::Other(src[i..end].to_string()), end)
            } else if src[i..].starts_with("<!") {
                let end = src[i..].find('>').ok_or_else(|| err(i, "unterminated doctype"))? + i + 1;
                (Kind::Other(src[i..end].to_string()), end)
            } else if src[i..].starts_with("</") {
                let end = src[i..].find('>').ok_or_else(|| err(i, "unterminated end tag"))? + i;
                let name = src[i + 2..end].trim();
                let open = if stack.len() > base { stack.pop() } else { None };
                match open.map(|n| &self.nodes[n].kind) {
                    Some(Kind::Element(e)) if e.name == name => {}
                    _ => return Err(err(i, &format!("unexpected </{name}>"))),
                }
                i = end + 1;
                continue;
            } else {
                let (el, next) = parse_tag(src, i).map_err(|m| err(i, &m))?;
                (Kind::Element(el), next)
            };
            let id = self.nodes.len();
            let open = matches!(&kind, Kind::Element(e) if !e.empty);
            self.nodes.push(Node { parent: stack.last().copied(), kind });
            match stack.last() {
                Some(&p) => {
                    self.el_mut(p).children.push(id);
                    if stack.len() == base { made.push(id) }
                }
                _ => {
                    self.top.push(id);
                    made.push(id);
                }
            }
            if open {
                stack.push(id);
            }
            i = next;
        }
        if stack.len() > base {
            return Err(err(b.len(), "unclosed element"));
        }
        Ok(made)
    }

    /// Parses one element into the arena without attaching it anywhere.
    pub fn parse_fragment(&mut self, src: &str) -> Result<NodeId, String> {
        let before = self.top.len();
        let parsed = self.parse_into(src, None);
        self.top.truncate(before);
        let els: Vec<NodeId> = parsed?.into_iter().filter(|&n| matches!(self.nodes[n].kind, Kind::Element(_))).collect();
        match els[..] {
            [one] => Ok(one),
            _ => Err("Fragment must hold exactly one element".into()),
        }
    }

    pub fn write(&self) -> String {
        let mut out = String::new();
        for &n in &self.top {
            self.write_node(n, &mut out);
        }
        out
    }

    pub fn write_node(&self, n: NodeId, out: &mut String) {
        match &self.nodes[n].kind {
            Kind::Text(t) => out.push_str(t),
            Kind::CData(t) => {
                out.push_str("<![CDATA[");
                out.push_str(t);
                out.push_str("]]>");
            }
            Kind::Other(t) => out.push_str(t),
            Kind::Element(e) => {
                out.push('<');
                out.push_str(&e.name);
                for a in &e.attrs {
                    out.push_str(&a.pre);
                    out.push_str(&a.key);
                    out.push('=');
                    out.push(a.quote);
                    out.push_str(&a.raw);
                    out.push(a.quote);
                }
                // A self-closing element that gained children drops the space before `/>`.
                if !(e.empty && !e.children.is_empty()) {
                    out.push_str(&e.tail);
                }
                if e.children.is_empty() && e.empty {
                    out.push_str("/>");
                    return;
                }
                out.push('>');
                for &c in &e.children {
                    self.write_node(c, out);
                }
                out.push_str("</");
                out.push_str(&e.name);
                out.push('>');
            }
        }
    }

    pub fn root(&self) -> Option<NodeId> {
        self.top.iter().copied().find(|&n| matches!(self.nodes[n].kind, Kind::Element(_)))
    }

    pub fn el(&self, n: NodeId) -> &Element {
        match &self.nodes[n].kind {
            Kind::Element(e) => e,
            _ => panic!("node {n} is not an element"),
        }
    }

    pub fn el_mut(&mut self, n: NodeId) -> &mut Element {
        match &mut self.nodes[n].kind {
            Kind::Element(e) => e,
            _ => panic!("node {n} is not an element"),
        }
    }

    pub fn is_el(&self, n: NodeId, name: &str) -> bool {
        matches!(&self.nodes[n].kind, Kind::Element(e) if e.name == name)
    }

    pub fn name(&self, n: NodeId) -> &str {
        &self.el(n).name
    }

    /// Child elements with the given tag (local name; namespace prefixes are not used by XAR).
    pub fn kids<'a>(&'a self, n: NodeId, name: &'a str) -> impl Iterator<Item = NodeId> + 'a {
        self.el(n).children.iter().copied().filter(move |&c| self.is_el(c, name))
    }

    pub fn kid(&self, n: NodeId, name: &str) -> Option<NodeId> {
        self.kids(n, name).next()
    }

    pub fn attr_raw(&self, n: NodeId, key: &str) -> Option<&str> {
        self.el(n).attrs.iter().find(|a| a.key == key).map(|a| a.raw.as_str())
    }

    pub fn attr(&self, n: NodeId, key: &str) -> Option<String> {
        self.attr_raw(n, key).map(unescape)
    }

    pub fn attr_or(&self, n: NodeId, key: &str) -> String {
        self.attr(n, key).unwrap_or_default()
    }

    pub fn attr_num(&self, n: NodeId, key: &str, default: f64) -> f64 {
        self.attr_raw(n, key).and_then(|v| v.trim().parse().ok()).unwrap_or(default)
    }

    /// Sets (or with `None` removes) an attribute given in raw form; returns the old raw value.
    pub fn set_attr_raw(&mut self, n: NodeId, key: &str, raw: Option<String>) -> Option<String> {
        let attrs = &mut self.el_mut(n).attrs;
        let pos = attrs.iter().position(|a| a.key == key);
        match (pos, raw) {
            (Some(p), Some(v)) => Some(std::mem::replace(&mut attrs[p].raw, v)),
            (Some(p), None) => Some(attrs.remove(p).raw),
            (None, Some(v)) => {
                attrs.push(Attr { pre: " ".into(), key: key.to_string(), raw: v, quote: '"' });
                None
            }
            (None, None) => None,
        }
    }

    /// Like `set_attr_raw`, but a new attribute goes before the first existing one that `order`
    /// ranks after it (appended when unranked).
    pub fn set_attr_ordered(&mut self, n: NodeId, key: &str, raw: Option<String>, order: &[&str]) -> Option<String> {
        let rank = |k: &str| order.iter().position(|&o| o == k);
        let attrs = &self.el(n).attrs;
        let at = match (raw.is_some() && !attrs.iter().any(|a| a.key == key), rank(key)) {
            (true, Some(r)) => attrs.iter().position(|a| rank(&a.key).is_some_and(|x| x > r)),
            _ => None,
        };
        let old = self.set_attr_raw(n, key, raw);
        if let Some(i) = at {
            let attrs = &mut self.el_mut(n).attrs;
            let a = attrs.pop().unwrap();
            attrs.insert(i, a);
        }
        old
    }

    /// Concatenated unescaped text and CDATA of an element.
    pub fn text(&self, n: NodeId) -> String {
        let mut s = String::new();
        for &c in &self.el(n).children {
            match &self.nodes[c].kind {
                Kind::Text(t) => s.push_str(&unescape(t)),
                Kind::CData(t) => s.push_str(t),
                Kind::Element(_) => s.push_str(&self.text(c)),
                Kind::Other(_) => {}
            }
        }
        s
    }

    /// Indentation depth of an element (root = 0).
    pub fn depth(&self, mut n: NodeId) -> usize {
        let mut d = 0;
        while let Some(p) = self.nodes[n].parent {
            d += 1;
            n = p;
        }
        d
    }

    pub fn new_text(&mut self, raw: String) -> NodeId {
        self.nodes.push(Node { parent: None, kind: Kind::Text(raw) });
        self.nodes.len() - 1
    }

    /// Indent width used by this document (spaces before the root's first child), default 4.
    pub fn indent_unit(&self) -> usize {
        let root = self.root().unwrap();
        self.el(root).children.iter().find_map(|&c| match &self.nodes[c].kind {
            Kind::Text(t) if t.trim().is_empty() && t.contains('\n') => Some(t.len() - t.rfind('\n').unwrap() - 1),
            _ => None,
        }).filter(|&n| n > 0).unwrap_or(4)
    }

    /// Re-lays out the indentation inside element `n` so that `n` sits at `depth` with `unit`
    /// spaces per level, whatever indentation the fragment came with. CDATA and text are untouched.
    pub fn reformat(&mut self, n: NodeId, depth: usize, unit: usize) {
        let spaces = |t: &str| t.len() - t.rfind('\n').map_or(0, |i| i + 1);
        let ws: Vec<usize> = self.el(n).children.iter().filter_map(|&c| match &self.nodes[c].kind {
            Kind::Text(t) if t.trim().is_empty() && t.contains('\n') => Some(spaces(t)),
            _ => None,
        }).collect();
        let (Some(&first), Some(&base)) = (ws.first(), ws.last()) else { return };
        let src_unit = if first > base { first - base } else { 4 };
        self.relayout(n, base, src_unit, depth, unit);
    }

    fn relayout(&mut self, n: NodeId, base: usize, src_unit: usize, depth: usize, unit: usize) {
        for c in self.el(n).children.clone() {
            match &mut self.nodes[c].kind {
                Kind::Text(t) if t.trim().is_empty() && t.contains('\n') => {
                    let lines: Vec<String> = t.split('\n').enumerate().map(|(i, line)| {
                        if i == 0 { return line.to_string() }
                        let level = (line.len().saturating_sub(base) + src_unit / 2) / src_unit;
                        " ".repeat((depth + level) * unit)
                    }).collect();
                    *t = lines.join("\n");
                }
                Kind::Element(_) => self.relayout(c, base, src_unit, depth, unit),
                _ => {}
            }
        }
    }

    pub fn subtree_string(&self, n: NodeId) -> String {
        let mut s = String::new();
        self.write_node(n, &mut s);
        s
    }
}

fn parse_tag(src: &str, start: usize) -> Result<(Element, usize), String> {
    let b = src.as_bytes();
    let mut i = start + 1;
    let name_end = src[i..].find(|c: char| c.is_whitespace() || c == '>' || c == '/').ok_or("unterminated tag")? + i;
    let name = src[i..name_end].to_string();
    if name.is_empty() {
        return Err("empty tag name".into());
    }
    i = name_end;
    let mut attrs = Vec::new();
    loop {
        let ws = i;
        while i < b.len() && b[i].is_ascii_whitespace() {
            i += 1;
        }
        let tail = src[ws..i].to_string();
        match b.get(i) {
            None => return Err("unterminated tag".into()),
            Some(b'>') => return Ok((Element { name, attrs, tail, children: vec![], empty: false }, i + 1)),
            Some(b'/') if b.get(i + 1) == Some(&b'>') => {
                return Ok((Element { name, attrs, tail, children: vec![], empty: true }, i + 2));
            }
            _ if tail.is_empty() => return Err("missing space before attribute".into()),
            _ => {}
        }
        let eq = src[i..].find('=').ok_or("attribute without value")? + i;
        let key = src[i..eq].trim().to_string();
        let mut j = eq + 1;
        while j < b.len() && b[j].is_ascii_whitespace() {
            j += 1;
        }
        let q = *b.get(j).ok_or("unterminated attribute")? as char;
        if q != '"' && q != '\'' {
            return Err(format!("unquoted attribute {key}"));
        }
        let end = src[j + 1..].find(q).ok_or("unterminated attribute value")? + j + 1;
        attrs.push(Attr { pre: tail, key, raw: src[j + 1..end].to_string(), quote: q });
        i = end + 1;
    }
}

pub fn unescape(raw: &str) -> String {
    if !raw.contains('&') {
        return raw.to_string();
    }
    let mut out = String::with_capacity(raw.len());
    let mut rest = raw;
    while let Some(p) = rest.find('&') {
        out.push_str(&rest[..p]);
        rest = &rest[p..];
        let Some(semi) = rest.find(';') else { break };
        let ent = &rest[1..semi];
        let ch = match ent {
            "lt" => Some('<'),
            "gt" => Some('>'),
            "amp" => Some('&'),
            "quot" => Some('"'),
            "apos" => Some('\''),
            _ if ent.starts_with("#x") || ent.starts_with("#X") => u32::from_str_radix(&ent[2..], 16).ok().and_then(char::from_u32),
            _ if ent.starts_with('#') => ent[1..].parse().ok().and_then(char::from_u32),
            _ => None,
        };
        match ch {
            Some(c) => {
                out.push(c);
                rest = &rest[semi + 1..];
            }
            None => {
                out.push('&');
                rest = &rest[1..];
            }
        }
    }
    out.push_str(rest);
    out
}

/// Escapes an attribute value the way Choregraphe writes it (newlines as `&#x0A;`).
pub fn escape_attr(v: &str) -> String {
    let mut s = String::with_capacity(v.len());
    for c in v.chars() {
        match c {
            '&' => s.push_str("&amp;"),
            '<' => s.push_str("&lt;"),
            '>' => s.push_str("&gt;"),
            '"' => s.push_str("&quot;"),
            '\'' => s.push_str("&apos;"),
            '\n' => s.push_str("&#x0A;"),
            '\r' => s.push_str("&#x0D;"),
            c => s.push(c),
        }
    }
    s
}

pub fn escape_text(v: &str) -> String {
    v.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trips_and_unescapes() {
        let src = "<?xml version=\"1.0\" ?>\n<a x=\"1&#x0A;&quot;\" y='b'>\n    <b />\n    <c><![CDATA[ <raw> ]]></c><!-- c -->t&amp;</a>\n";
        let d = Doc::parse(src).unwrap();
        assert_eq!(d.write(), src);
        let a = d.root().unwrap();
        assert_eq!(d.attr(a, "x").unwrap(), "1\n\"");
        assert_eq!(d.text(d.kid(a, "c").unwrap()), " <raw> ");
        assert!(Doc::parse("<a><b></a>").is_err());
        assert!(Doc::parse("<a x=1/>").is_err());
    }
}
