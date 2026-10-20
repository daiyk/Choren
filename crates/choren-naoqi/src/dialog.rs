//! ALDialog: QiChat topics on the virtual robot. Typed input (`forceInput`, `ChorenSim.hear`) and ALMemory
//! events are matched against the active topics; answers are said through ALAnimatedSpeech, as
//! NAOqi's dialog does, then reported as `answered` and `Dialog/Answered`.
//!
//! Matching follows the reference: case and punctuation are ignored and extra input words are
//! tolerated anywhere (the rule needing the fewest wins), missing or different words are not.
//! Between equally good rules, those of the last answer's scope (`u1:` under it) win, then the
//! focused topic's, then the other active topics'; the rest is chance. `[a b]` and `~concept`
//! outputs cycle, proposals are said once unless reactivated.
//! shortcut: no lemmatizer (NAOqi also matches "hellos" to "hello"), no conditions, no `$var` in
//! inputs, no `^call`/`^endDiscuss`, and box inputs are not delivered to topics (on NAOqi they
//! reached no `e:` rule either).

use crate::Robotd;
use choren_qi::{ServiceBuilder, Value};
use std::collections::{HashMap, HashSet};
use std::path::Path;
use std::sync::{Arc, Mutex, Weak};

/// ALDialog language names and topic codes (`share/choregraphe/robotlanguages`).
const LANGUAGES: &[(&str, &str)] = &[
    ("Arabic", "arw"), ("Brazilian", "ptb"), ("Chinese", "mnc"), ("Czech", "czc"), ("Danish", "dad"), ("Dutch", "dun"), ("English", "enu"), ("Finnish", "fif"),
    ("French", "frf"), ("German", "ged"), ("Greek", "grg"), ("Italian", "iti"), ("Japanese", "jpj"), ("Korean", "kok"), ("MandarinTaiwan", "mnt"), ("Norwegian", "nor"),
    ("Polish", "plp"), ("Portuguese", "ptp"), ("Russian", "rur"), ("Spanish", "spe"), ("Swedish", "sws"), ("Turkish", "trt"),
];

/// "English" or "enu" → "enu".
fn code(language: &str) -> String {
    LANGUAGES.iter().find(|l| l.0.eq_ignore_ascii_case(language)).map_or(language, |l| l.1).to_string()
}

#[derive(Clone, Debug)]
enum Pat {
    Word(String),
    Seq(Vec<Pat>),
    Choice(Vec<Pat>),
    Opt(Box<Pat>),
    Concept(String),
    /// `*`: one word or more.
    Star,
    /// `_x`: the words `x` matched are `$1`, `$2`… in order.
    Capture(Box<Pat>),
}

#[derive(Clone, Debug)]
enum Pick {
    Cycle,
    Random,
    First,
}

#[derive(Clone, Debug)]
enum Out {
    /// A word, punctuation, `\markup\` or animation call, said as written.
    Text(String),
    /// `[a "b c"]` (cycles), `^rand[…]`, `^first[…]`; the id keys its counter.
    Choice(usize, Vec<Vec<Out>>, Pick),
    Concept(usize, String),
    /// `$name` or `$1`.
    Var(String),
    /// `$name=value`.
    Set(String, Vec<Out>),
    /// `^goto(label)`, `^gotoReactivate(label)`, `^nextProposal`, `^sameProposal`.
    Goto(String, bool),
    Next,
    Same,
}

#[derive(Clone, Debug, PartialEq)]
enum Input {
    None,
    Event(String),
    Words,
}

#[derive(Clone, Debug)]
struct Rule {
    level: usize,
    parent: Option<usize>,
    proposal: bool,
    input: Input,
    pattern: Pat,
    label: Option<String>,
    output: Vec<Out>,
}

#[derive(Debug, Default)]
pub struct Topic {
    pub name: String,
    pub language: String,
    concepts: HashMap<String, Vec<Pat>>,
    rules: Vec<Rule>,
}

/// Matching state of a loaded topic: proposals already said, the last one, output counters.
#[derive(Clone, Default)]
struct Progress {
    said: HashSet<usize>,
    last_proposal: Option<usize>,
    counters: HashMap<usize, usize>,
}

#[derive(Default)]
struct State {
    language: Option<String>,
    topics: Vec<Arc<Topic>>,
    progress: HashMap<(String, String), Progress>,
    active: Vec<String>,
    focus: Option<String>,
    /// The rule that answered last: its subrules win ties.
    scope: Option<(String, usize)>,
    /// Box ports of Dialog box topics: `$port=…` also sets `<topic><port>`.
    io: HashMap<String, Vec<String>>,
    /// `setConcept` (language code, name) → words.
    dynamic: HashMap<(String, String), Vec<String>>,
}

#[derive(Default)]
pub struct Dialog {
    st: Mutex<State>,
    robot: Mutex<Weak<Robotd>>,
}

/// What a rule does: said parts and memory writes in order, and the text `answered` reports.
#[derive(Debug, PartialEq)]
enum Step {
    Say(String),
    Set(String, String),
}

#[derive(Debug)]
struct Answer {
    steps: Vec<Step>,
    text: String,
}

// --- parsing ---

struct Chars<'a> {
    c: Vec<char>,
    i: usize,
    ids: &'a mut usize,
}

impl Chars<'_> {
    fn peek(&self) -> Option<char> {
        self.c.get(self.i).copied()
    }

    fn skip_ws(&mut self) {
        while self.peek().is_some_and(char::is_whitespace) {
            self.i += 1;
        }
    }

    fn take_while(&mut self, f: impl Fn(char) -> bool) -> String {
        let start = self.i;
        while self.peek().is_some_and(&f) {
            self.i += 1;
        }
        self.c[start..self.i].iter().collect()
    }

    fn id(&mut self) -> usize {
        *self.ids += 1;
        *self.ids
    }

    /// Input elements up to `end` (or the end of text).
    fn pats(&mut self, end: Option<char>) -> Result<Vec<Pat>, String> {
        let mut out = vec![];
        loop {
            self.skip_ws();
            let Some(c) = self.peek() else {
                return match end {
                    Some(e) => Err(format!("missing '{e}'")),
                    None => Ok(out),
                };
            };
            if Some(c) == end {
                self.i += 1;
                return Ok(out);
            }
            if let Some(p) = self.pat()? {
                out.push(p);
            }
        }
    }

    fn pat(&mut self) -> Result<Option<Pat>, String> {
        let c = self.peek().unwrap();
        self.i += 1;
        Ok(Some(match c {
            '[' => Pat::Choice(self.pats(Some(']'))?),
            '{' => Pat::Opt(Box::new(Pat::Seq(self.pats(Some('}'))?))),
            '"' => Pat::Seq(self.pats(Some('"'))?),
            '~' => Pat::Concept(self.take_while(|c| c.is_alphanumeric() || c == '_')),
            '*' => Pat::Star,
            '_' => match self.peek() {
                Some(_) => Pat::Capture(Box::new(self.pat()?.ok_or("nothing to capture after '_'")?)),
                None => return Err("nothing to capture after '_'".into()),
            },
            '$' => return Err("variables in rule inputs are not simulated on the virtual robot".into()),
            ']' | '}' | ')' => return Err(format!("unexpected '{c}'")),
            _ => {
                self.i -= 1;
                let w = self.take_while(|c| !c.is_whitespace() && !"[]{}\"~*()".contains(c));
                let words = words(&w);
                match words.len() {
                    0 => return Ok(None), // punctuation
                    1 => Pat::Word(words[0].clone()),
                    _ => Pat::Seq(words.into_iter().map(Pat::Word).collect()),
                }
            }
        }))
    }

    /// Output elements up to `end`.
    fn outs(&mut self, end: Option<char>) -> Result<Vec<Out>, String> {
        let mut out = vec![];
        loop {
            self.skip_ws();
            let Some(c) = self.peek() else {
                return match end {
                    Some(e) => Err(format!("missing '{e}'")),
                    None => Ok(out),
                };
            };
            if Some(c) == end {
                self.i += 1;
                return Ok(out);
            }
            out.extend(self.out()?);
        }
    }

    fn choice(&mut self, pick: Pick) -> Result<Out, String> {
        let mut alts = vec![];
        loop {
            self.skip_ws();
            match self.peek() {
                None => return Err("missing ']'".into()),
                Some(']') => {
                    self.i += 1;
                    return Ok(Out::Choice(self.id(), alts, pick));
                }
                Some('"') => {
                    self.i += 1;
                    alts.push(self.outs(Some('"'))?);
                }
                Some(_) => alts.push(self.out()?),
            }
        }
    }

    fn out(&mut self) -> Result<Vec<Out>, String> {
        let c = self.peek().unwrap();
        self.i += 1;
        Ok(match c {
            '[' => vec![self.choice(Pick::Cycle)?],
            '"' => self.outs(Some('"'))?,
            '~' => vec![Out::Concept(self.id(), self.take_while(|c| c.is_alphanumeric() || c == '_'))],
            '\\' => {
                let m = self.take_while(|c| c != '\\');
                self.i += 1;
                vec![Out::Text(format!("\\{m}\\"))]
            }
            '$' => {
                let name = self.take_while(|c| c.is_alphanumeric() || "_/-.".contains(c));
                if self.peek() != Some('=') {
                    return Ok(vec![Out::Var(name)]);
                }
                self.i += 1;
                let value = match self.peek() {
                    Some('"') => {
                        self.i += 1;
                        self.outs(Some('"'))?
                    }
                    _ => {
                        let v = self.take_while(|c| !c.is_whitespace());
                        if let Some(var) = v.strip_prefix('$') { vec![Out::Var(var.into())] } else { vec![Out::Text(v)] }
                    }
                };
                vec![Out::Set(name, value)]
            }
            '^' => {
                let name = self.take_while(|c| c.is_alphanumeric() || c == '_');
                match self.peek() {
                    Some('[') => {
                        self.i += 1;
                        vec![self.choice(match name.as_str() { "first" => Pick::First, _ => Pick::Random })?]
                    }
                    Some('(') => {
                        self.i += 1;
                        let args = self.take_while(|c| c != ')');
                        self.i += 1;
                        match name.as_str() {
                            "goto" | "gotoReactivate" => vec![Out::Goto(args.trim().into(), name == "gotoReactivate")],
                            _ => vec![Out::Text(format!("^{name}({args})"))],
                        }
                    }
                    _ => match name.as_str() {
                        "nextProposal" => vec![Out::Next],
                        "sameProposal" => vec![Out::Same],
                        _ => vec![Out::Text(format!("^{name}"))],
                    },
                }
            }
            _ => {
                self.i -= 1;
                let w = self.take_while(|c| !c.is_whitespace() && !"[]\"\\".contains(c));
                // NAOqi says `,` and `!` as words of their own ("Hello , world !").
                let mut out = vec![];
                for piece in w.split_inclusive([',', '!']) {
                    let (word, punct) = piece.strip_suffix([',', '!']).map_or((piece, None), |s| (s, piece.chars().last()));
                    if !word.is_empty() {
                        out.push(Out::Text(word.into()));
                    }
                    if let Some(p) = punct {
                        out.push(Out::Text(p.into()));
                    }
                }
                out
            }
        })
    }
}

/// The words of a text, punctuation dropped (apostrophes and hyphens kept).
fn words(text: &str) -> Vec<String> {
    text.split(|c: char| !(c.is_alphanumeric() || c == '\'' || c == '-')).filter(|w| !w.is_empty()).map(String::from).collect()
}

/// `(…)` at the start of `s`: its contents and the rest.
fn parenthesized(s: &str) -> Result<(&str, &str), String> {
    let s = s.trim_start();
    if !s.starts_with('(') {
        return Err("expected '('".into());
    }
    let (mut depth, mut quoted) = (0, false);
    for (i, c) in s.char_indices() {
        match c {
            '"' => quoted = !quoted,
            '(' if !quoted => depth += 1,
            ')' if !quoted => {
                depth -= 1;
                if depth == 0 {
                    return Ok((&s[1..i], &s[i + 1..]));
                }
            }
            _ => {}
        }
    }
    Err("missing ')'".into())
}

/// Parses a `.top` file. Errors carry the line number.
pub fn parse(text: &str) -> Result<Topic, (usize, String)> {
    let mut t = Topic::default();
    let mut ids = 0;
    let mut last: Vec<usize> = vec![];
    for (n, line) in text.lines().enumerate() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let err = |m: String| (n + 1, m);
        let (head, rest) = line.split_once(':').ok_or_else(|| err(format!("unexpected \"{line}\"")))?;
        fn chars<'a>(s: &str, ids: &'a mut usize) -> Chars<'a> {
            Chars { c: s.chars().collect(), i: 0, ids }
        }
        match head.trim() {
            "topic" => t.name = rest.trim().trim_start_matches('~').split('(').next().unwrap_or("").trim().to_string(),
            "language" => t.language = rest.trim().to_string(),
            "concept" => {
                let (name, body) = parenthesized(rest).map_err(err)?;
                let pats = chars(body, &mut ids).pats(None).map_err(err)?;
                let alts = match pats.as_slice() {
                    [Pat::Choice(items)] => items.clone(),
                    _ => vec![Pat::Seq(pats)],
                };
                t.concepts.insert(name.trim().to_string(), alts);
            }
            "dynamic" => {}
            h => {
                let level = match h {
                    "proposal" => 0,
                    _ => h.strip_prefix('u').and_then(|l| if l.is_empty() { Some(0) } else { l.parse().ok() }).ok_or_else(|| err(format!("unexpected \"{h}\"")))?,
                };
                let proposal = h == "proposal";
                let (input, pattern, body) = if proposal {
                    (Input::None, Pat::Seq(vec![]), rest)
                } else {
                    let (inside, body) = parenthesized(rest).map_err(err)?;
                    match inside.trim().strip_prefix("e:") {
                        Some(key) => (Input::Event(key.split_whitespace().next().unwrap_or("").to_string()), Pat::Seq(vec![]), body),
                        None => (Input::Words, Pat::Seq(chars(inside, &mut ids).pats(None).map_err(err)?), body),
                    }
                };
                let body = body.trim();
                let (label, body) = match body.strip_prefix('%') {
                    Some(b) => {
                        let (l, rest) = b.split_once(char::is_whitespace).unwrap_or((b, ""));
                        (Some(l.to_string()), rest)
                    }
                    None => (None, body),
                };
                let output = chars(body, &mut ids).outs(None).map_err(err)?;
                let parent = match level {
                    0 => None,
                    l => Some(*last.get(l - 1).ok_or_else(|| err(format!("u{l} without a rule above it")))?),
                };
                last.truncate(level);
                last.push(t.rules.len());
                t.rules.push(Rule { level, parent, proposal, input, pattern, label, output });
            }
        }
    }
    if t.name.is_empty() {
        return Err((1, "missing \"topic:\"".into()));
    }
    Ok(t)
}

// --- matching ---

type Ends = Vec<(usize, usize, Vec<(usize, usize)>)>;

/// Keeps the cheapest way to reach each end.
fn best_per_end(mut v: Ends) -> Ends {
    v.sort_by_key(|e| (e.0, e.1));
    v.dedup_by_key(|e| e.0);
    v
}

impl State {
    /// Concepts belong to the language: the first loaded topic that defines one wins (NAOqi also
    /// keeps them after their topic is unloaded).
    fn concept(&self, t: &Topic, name: &str) -> Vec<Pat> {
        match self.topics.iter().filter(|x| x.language == t.language).find_map(|x| x.concepts.get(name)) {
            Some(alts) => alts.clone(),
            None => self.dynamic.get(&(t.language.clone(), name.to_string())).map(|w| w.iter().map(|x| Pat::Seq(words(x).into_iter().map(Pat::Word).collect())).collect()).unwrap_or_default(),
        }
    }

    /// Every way `p` matches the lower-case words `w` from `at`: (end, extra words skipped,
    /// captured ranges).
    fn ends(&self, t: &Topic, p: &Pat, w: &[String], at: usize, depth: usize) -> Ends {
        if depth > 32 {
            return vec![];
        }
        match p {
            Pat::Word(x) => if w.get(at).is_some_and(|y| *y == x.to_lowercase()) { vec![(at + 1, 0, vec![])] } else { vec![] },
            Pat::Star => (at + 1..=w.len()).map(|e| (e, 0, vec![])).collect(),
            Pat::Opt(x) => best_per_end([vec![(at, 0, vec![])], self.ends(t, x, w, at, depth + 1)].concat()),
            Pat::Choice(alts) => best_per_end(alts.iter().flat_map(|a| self.ends(t, a, w, at, depth + 1)).collect()),
            Pat::Concept(name) => best_per_end(self.concept(t, name).iter().flat_map(|a| self.ends(t, a, w, at, depth + 1)).collect()),
            Pat::Capture(x) => self.ends(t, x, w, at, depth + 1).into_iter().map(|(e, c, caps)| (e, c, [vec![(at, e)], caps].concat())).collect(),
            Pat::Seq(items) => {
                let mut states: Ends = vec![(at, 0, vec![])];
                for item in items {
                    let mut next = vec![];
                    for (pos, cost, caps) in &states {
                        for skip in 0..=w.len() - pos {
                            for (e, c, cp) in self.ends(t, item, w, pos + skip, depth + 1) {
                                next.push((e, cost + skip + c, [caps.clone(), cp].concat()));
                            }
                        }
                    }
                    states = best_per_end(next);
                }
                states
            }
        }
    }

    /// Words skipped (fewest first) and captures when `rule` matches the whole input.
    fn matches(&self, t: &Topic, rule: &Rule, w: &[String]) -> Option<(usize, Vec<String>)> {
        let lower: Vec<String> = w.iter().map(|x| x.to_lowercase()).collect();
        self.ends(t, &rule.pattern, &lower, 0, 0).into_iter().map(|(e, c, caps)| (c + w.len() - e, caps)).min_by_key(|m| m.0).map(|(c, caps)| (c, caps.iter().map(|&(a, b)| w[a..b].join(" ")).collect()))
    }

    fn topic(&self, name: &str) -> Option<Arc<Topic>> {
        let lang = code(self.language.as_deref().unwrap_or("English"));
        self.topics.iter().find(|t| t.name == name && t.language == lang).cloned()
    }

    /// Candidate groups in tie-break order: the scope's subrules, the focused topic, the others.
    fn groups(&self) -> Vec<Vec<(Arc<Topic>, usize)>> {
        let mut groups = vec![];
        if let Some((name, r)) = &self.scope {
            if let Some(t) = self.active.contains(name).then(|| self.topic(name)).flatten() {
                groups.push((0..t.rules.len()).filter(|&i| t.rules[i].parent == Some(*r)).map(|i| (t.clone(), i)).collect());
            }
        }
        let mut order: Vec<&String> = self.focus.iter().filter(|f| self.active.contains(f)).collect();
        order.extend(self.active.iter().filter(|a| Some(*a) != self.focus.as_ref()));
        for name in order {
            if let Some(t) = self.topic(name) {
                groups.push((0..t.rules.len()).filter(|&i| t.rules[i].level == 0 && !t.rules[i].proposal).map(|i| (t.clone(), i)).collect());
            }
        }
        groups
    }

    /// The best rule for a typed input, or for an ALMemory event: fewest extra words first, then
    /// the earliest group, then at random.
    fn pick(&self, input: Result<&[String], &str>) -> Option<(Arc<Topic>, usize, Vec<String>)> {
        let mut found = vec![];
        for (g, group) in self.groups().into_iter().enumerate() {
            for (t, r) in group {
                let m = match (input, &t.rules[r].input) {
                    (Ok(w), Input::Words) => self.matches(&t, &t.rules[r], w),
                    (Err(key), Input::Event(k)) if k == key => Some((0, vec![])),
                    _ => None,
                };
                if let Some((cost, caps)) = m {
                    found.push(((cost, g), t.clone(), r, caps));
                }
            }
        }
        let min = found.iter().map(|f| f.0).min()?;
        let mut best: Vec<_> = found.into_iter().filter(|f| f.0 == min).collect();
        let (_, t, r, caps) = best.swap_remove(random(best.len()));
        Some((t, r, caps))
    }
}

fn random(n: usize) -> usize {
    use std::sync::atomic::{AtomicUsize, Ordering};
    static SEED: AtomicUsize = AtomicUsize::new(0);
    let nanos = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.subsec_nanos() as usize);
    (nanos ^ SEED.fetch_add(0x9e37_79b9, Ordering::Relaxed)) % n.max(1)
}

// --- answering ---

struct Run<'a> {
    st: &'a mut State,
    t: Arc<Topic>,
    caps: Vec<String>,
    vars: HashMap<String, String>,
    memory: &'a dyn Fn(&str) -> Option<String>,
    part: Vec<String>,
    sets: Vec<Step>,
    steps: Vec<Step>,
}

impl Run<'_> {
    fn progress(&mut self) -> &mut Progress {
        self.st.progress.entry((self.t.name.clone(), self.t.language.clone())).or_default()
    }

    fn flush(&mut self, last: bool) {
        if !self.part.is_empty() {
            let text = self.part.join(" ");
            self.steps.push(Step::Say(if last { with_last(&text) } else { text }));
            self.part.clear();
        }
        self.steps.append(&mut self.sets);
    }

    /// Says rule `r`: a proposal is then said, and its subrules are in scope.
    fn rule(&mut self, r: usize, tagged: bool) -> Option<()> {
        let rule = self.t.rules[r].clone();
        if rule.proposal {
            self.progress().said.insert(r);
            self.progress().last_proposal = Some(r);
        }
        self.st.scope = Some((self.t.name.clone(), r));
        if let (true, Some(l)) = (tagged, &rule.label) {
            self.part.push(format!("$Dialog/Tag={l}"));
        }
        self.outs(&rule.output)
    }

    fn goto(&mut self, target: Option<usize>, reactivate: bool) -> Option<()> {
        let r = target?;
        if self.t.rules[r].proposal && !reactivate && self.progress().said.contains(&r) {
            return None;
        }
        // A labelled target is said on its own, after its `$Dialog/Tag=` marker.
        if self.t.rules[r].label.is_some() {
            self.flush(false);
        }
        self.rule(r, true)
    }

    fn value(&self, name: &str) -> Option<String> {
        match name.parse::<usize>() {
            Ok(i) => self.caps.get(i.checked_sub(1)?).cloned(),
            Err(_) => self.vars.get(name).cloned().or_else(|| (self.memory)(name)),
        }
    }

    fn outs(&mut self, outs: &[Out]) -> Option<()> {
        for o in outs {
            match o {
                Out::Text(s) => self.part.push(s.clone()),
                Out::Var(n) => {
                    let v = self.value(n)?;
                    self.part.push(v);
                }
                Out::Set(n, v) => {
                    let mut sub = Run { st: &mut *self.st, t: self.t.clone(), caps: self.caps.clone(), vars: self.vars.clone(), memory: self.memory, part: vec![], sets: vec![], steps: vec![] };
                    sub.outs(v)?;
                    let v = sub.part.join(" ");
                    self.vars.insert(n.clone(), v.clone());
                    if self.st.io.get(&self.t.name).is_some_and(|io| io.contains(n)) {
                        self.sets.push(Step::Set(format!("{}{n}", self.t.name), v.clone()));
                    }
                    self.sets.push(Step::Set(n.clone(), v));
                }
                Out::Concept(id, name) => {
                    let alts = self.st.concept(&self.t, name);
                    let n = self.progress().counters.entry(*id).or_default();
                    let alt = alts.get(*n % alts.len().max(1))?;
                    *n += 1;
                    self.part.push(display(alt));
                }
                Out::Choice(id, alts, pick) => match pick {
                    Pick::First => {
                        let (part, vars, sets) = (self.part.clone(), self.vars.clone(), self.sets.len());
                        let ok = alts.iter().any(|a| {
                            if self.outs(a).is_some() {
                                return true;
                            }
                            (self.part, self.vars) = (part.clone(), vars.clone());
                            self.sets.truncate(sets);
                            false
                        });
                        ok.then_some(())?;
                    }
                    _ => {
                        let i = match pick {
                            Pick::Random => random(alts.len()),
                            _ => {
                                let n = self.progress().counters.entry(*id).or_default();
                                *n += 1;
                                (*n - 1) % alts.len().max(1)
                            }
                        };
                        self.outs(alts.get(i)?)?;
                    }
                },
                // A goto with nowhere to go adds nothing (measured: "Oh, I feel sorry for you.
                // ^nextProposal" with no proposal left says its text).
                Out::Goto(label, reactivate) => {
                    let target = self.t.rules.iter().position(|r| r.label.as_deref() == Some(label));
                    self.goto(target, *reactivate);
                }
                Out::Next => {
                    let from = self.progress().last_proposal.map_or(0, |p| p + 1);
                    let said = self.progress().said.clone();
                    let target = (from..self.t.rules.len()).find(|&i| self.t.rules[i].proposal && !said.contains(&i));
                    self.goto(target, false);
                }
                Out::Same => {
                    let target = self.progress().last_proposal;
                    self.goto(target, true);
                }
            }
        }
        Some(())
    }
}

fn display(p: &Pat) -> String {
    match p {
        Pat::Word(w) => w.clone(),
        Pat::Seq(items) => items.iter().map(display).collect::<Vec<_>>().join(" "),
        Pat::Choice(alts) => alts.first().map(display).unwrap_or_default(),
        Pat::Opt(x) | Pat::Capture(x) => display(x),
        Pat::Concept(_) | Pat::Star => String::new(),
    }
}

/// Characters inside `\markup\` and `^call(…)`.
fn masked(c: &[char]) -> Vec<bool> {
    let mut m = vec![false; c.len()];
    let mut i = 0;
    while i < c.len() {
        let end = match c[i] {
            '\\' => c[i + 1..].iter().position(|&x| x == '\\').map(|p| i + 1 + p),
            '^' => {
                let j = i + 1 + c[i + 1..].iter().take_while(|x| x.is_alphanumeric() || **x == '_').count();
                if c.get(j) == Some(&'(') { c[j..].iter().position(|&x| x == ')').map(|p| j + p) } else { None }
            }
            _ => None,
        };
        match end {
            Some(e) => {
                m[i..=e].fill(true);
                i = e + 1;
            }
            None => i += 1,
        }
    }
    m
}

/// NAOqi marks the end of an answer with `\last\` after its last letter or digit outside
/// markup (never the very first character), before trailing punctuation: "question \last\?".
fn with_last(text: &str) -> String {
    let c: Vec<char> = text.chars().collect();
    let m = masked(&c);
    match (1..c.len()).rev().find(|&i| !m[i] && c[i].is_alphanumeric()) {
        Some(at) => format!("{} \\last\\{}", c[..=at].iter().collect::<String>(), c[at + 1..].iter().collect::<String>()),
        None => text.to_string(),
    }
}

/// The answer text without `\markup\` and `^calls(…)`, spaces kept (`Dialog/Answered`).
fn strip(text: &str) -> String {
    let c: Vec<char> = text.chars().collect();
    let m = masked(&c);
    c.iter().zip(m).filter(|(_, m)| !m).map(|(x, _)| x).collect()
}

fn text_of(v: &Value) -> Option<String> {
    Some(match v.plain() {
        Value::Str(s) => s.clone(),
        Value::Int(i) => i.to_string(),
        Value::UInt(u) => u.to_string(),
        Value::Float(f) => f.to_string(),
        Value::Double(f) => f.to_string(),
        Value::Bool(b) => b.to_string(),
        _ => return None,
    })
}

impl Dialog {
    fn robot(&self) -> Option<Arc<Robotd>> {
        self.robot.lock().unwrap().upgrade()
    }

    /// Loads a `.top` file (replacing the same topic and language); returns the topic name.
    pub fn load(&self, path: &Path) -> Result<String, String> {
        let text = std::fs::read_to_string(path).map_err(|e| format!("\tloadTopic::ALDialog\n\tCannot read {}: {e}", path.display()))?;
        let t = parse(&text).map_err(|(line, m)| format!("\tloadTopic::ALDialog\n\t{}  {m} at line {line}", path.display()))?;
        let name = t.name.clone();
        let mut st = self.st.lock().unwrap();
        st.progress.remove(&(t.name.clone(), t.language.clone()));
        match st.topics.iter().position(|x| x.name == t.name && x.language == t.language) {
            Some(i) => st.topics[i] = Arc::new(t),
            None => st.topics.push(Arc::new(t)),
        }
        Ok(name)
    }

    /// A Dialog box's `.dlg` (`multilanguage:<name>` then `<code>:<file>.top` lines): loads every
    /// language's topic and registers the box ports.
    pub fn load_box(&self, dlg: &Path, ports: Vec<String>) -> Result<String, String> {
        let text = std::fs::read_to_string(dlg).map_err(|e| format!("Cannot read {}: {e}", dlg.display()))?;
        let mut name = None;
        for line in text.lines().skip(1) {
            if let Some((_, file)) = line.trim().split_once(':') {
                name = Some(self.load(&dlg.with_file_name(file.trim()))?);
            }
        }
        let name = name.ok_or_else(|| format!("{} lists no topic", dlg.display()))?;
        self.st.lock().unwrap().io.insert(name.clone(), ports);
        Ok(name)
    }

    pub fn unload(&self, name: &str) -> Result<(), String> {
        let mut st = self.st.lock().unwrap();
        if !st.topics.iter().any(|t| t.name == name) {
            return Err(format!("\tALDialog::Dialog\n\tTopic {name} not found"));
        }
        st.topics.retain(|t| t.name != name);
        st.progress.retain(|k, _| k.0 != name);
        st.active.retain(|a| a != name);
        st.io.remove(name);
        Ok(())
    }

    pub fn activate(&self, name: &str) -> Result<(), String> {
        let mut st = self.st.lock().unwrap();
        if !st.topics.iter().any(|t| t.name == name) {
            return Err(format!("\tloadTopic::ALDialog\n\tTopic {name} not found"));
        }
        if !st.active.iter().any(|a| a == name) {
            st.active.push(name.to_string());
        }
        Ok(())
    }

    pub fn deactivate(&self, name: &str) {
        self.st.lock().unwrap().active.retain(|a| a != name);
    }

    /// The answer of the active topics to `key` (typed input words, or an ALMemory event).
    fn answer(&self, d: &Robotd, input: Result<&[String], &str>) -> Option<Answer> {
        let mut st = self.st.lock().unwrap();
        let (t, r, caps) = st.pick(input)?;
        let (progress, scope) = (st.progress.clone(), st.scope.clone());
        let memory = |k: &str| d.get_data(k).ok().and_then(|v| text_of(&v));
        let mut run = Run { st: &mut st, t: t.clone(), caps, vars: HashMap::new(), memory: &memory, part: vec![], sets: vec![], steps: vec![] };
        let ok = run.rule(r, false).is_some();
        run.flush(true);
        let steps = std::mem::take(&mut run.steps);
        if !ok || !steps.iter().any(|s| matches!(s, Step::Say(_))) {
            // An undefined variable, or nothing to say (a goto to a proposal already said): NAOqi
            // stays silent.
            (st.progress, st.scope) = (progress, scope);
            return None;
        }
        st.focus = Some(t.name.clone());
        let text = steps.iter().filter_map(|s| if let Step::Say(t) = s { Some(strip(t)) } else { None }).collect::<Vec<_>>().join(" ").trim().to_string();
        Some(Answer { steps, text })
    }

    /// An ALMemory change: `u:(e:<key>)` rules of the active topics answer. The dialog's own keys
    /// do not (`input` answers `Dialog/NotUnderstood` itself).
    /// shortcut: NAOqi answers `e:Dialog/LastInput` at the next input when the key was read.
    pub fn memory_changed(&self, key: &str) {
        if ["Dialog/LastInput", "Dialog/Answered", "Dialog/LastAnswer", "Dialog/NotUnderstood"].contains(&key) {
            return;
        }
        let Some(d) = self.robot() else { return };
        let listens = {
            let st = self.st.lock().unwrap();
            st.active.iter().filter_map(|a| st.topic(a)).any(|t| t.rules.iter().any(|r| r.input == Input::Event(key.to_string())))
        };
        if let Some(a) = listens.then(|| self.answer(&d, Err(key))).flatten() {
            tokio::spawn(async move { speak(&d, a).await });
        }
    }
}

/// Says each part through ALAnimatedSpeech (as NAOqi's dialog does) and writes the variables,
/// in order, then reports the answer.
async fn speak(d: &Arc<Robotd>, a: Answer) {
    for s in a.steps {
        match s {
            Step::Say(t) => {
                let _ = d.node.call("ALAnimatedSpeech", "say", vec![t.into()]).await;
            }
            Step::Set(k, v) => d.set_data(&k, v.into()),
        }
    }
    d.set_data("Dialog/Answered", a.text.as_str().into());
    d.set_data("Dialog/LastAnswer", a.text.as_str().into());
    d.node.emit("ALDialog", "answered", vec![a.text.into()]);
}

/// Typed speech: `ALDialog.forceInput` and `ChorenSim.hear`. Returns once the answer is said; an
/// input no rule matches raises `Dialog/NotUnderstood` (empty), which `e:` rules answer.
pub async fn input(d: Arc<Robotd>, text: String) {
    d.set_data("Dialog/LastInput", text.as_str().into());
    d.node.emit("ALDialog", "lastInput", vec![text.as_str().into()]);
    let answer = d.dialog.answer(&d, Ok(&words(&text))).or_else(|| {
        d.set_data("Dialog/NotUnderstood", "".into());
        d.dialog.answer(&d, Err("Dialog/NotUnderstood"))
    });
    if let Some(a) = answer {
        speak(&d, a).await;
    }
}

fn s(a: &[Value], i: usize) -> Result<String, String> {
    a.get(i).and_then(Value::as_str).map(str::to_string).ok_or_else(|| format!("argument {} must be a string", i + 1))
}

pub fn register(d: &Arc<Robotd>) {
    *d.dialog.robot.lock().unwrap() = Arc::downgrade(d);
    let ctx = || Arc::downgrade(d);
    let get = |w: &Weak<Robotd>| w.upgrade().ok_or_else(|| "robot is shutting down".to_string());
    let names = |v: Vec<String>| Ok(Value::List(v.into_iter().map(Value::Str).collect()));
    let noop = |_: Vec<Value>| Ok(Value::Void);
    d.node.register(
        ServiceBuilder::new("ALDialog")
            .method("loadTopic", "(s)", "s", {
                let w = ctx();
                move |a| Ok(get(&w)?.dialog.load(Path::new(&s(&a, 0)?))?.into())
            })
            .method("unloadTopic", "(s)", "v", {
                let w = ctx();
                move |a| get(&w)?.dialog.unload(&s(&a, 0)?).map(|_| Value::Void)
            })
            .method("activateTopic", "(s)", "v", {
                let w = ctx();
                move |a| get(&w)?.dialog.activate(&s(&a, 0)?).map(|_| Value::Void)
            })
            .method("deactivateTopic", "(s)", "v", {
                let w = ctx();
                move |a| {
                    get(&w)?.dialog.deactivate(&s(&a, 0)?);
                    Ok(Value::Void)
                }
            })
            .method("getLoadedTopics", "(s)", "[s]", {
                let w = ctx();
                move |a| {
                    let (d, lang) = (get(&w)?, code(&s(&a, 0)?));
                    let st = d.dialog.st.lock().unwrap();
                    names(st.topics.iter().filter(|t| t.language == lang).map(|t| t.name.clone()).collect())
                }
            })
            .method("getAllLoadedTopics", "()", "[s]", {
                let w = ctx();
                move |_| {
                    let d = get(&w)?;
                    let st = d.dialog.st.lock().unwrap();
                    let mut all: Vec<String> = st.topics.iter().map(|t| t.name.clone()).collect();
                    all.dedup();
                    names(all)
                }
            })
            .method("getActivatedTopics", "()", "[s]", {
                let w = ctx();
                move |_| names(get(&w)?.dialog.st.lock().unwrap().active.clone())
            })
            .method_async("forceInput", "(s)", "v", {
                let w = ctx();
                move |a| {
                    let w = w.clone();
                    async move {
                        input(get(&w)?, s(&a, 0)?).await;
                        Ok(Value::Void)
                    }
                }
            })
            // The dialog speaks the TTS language: NAOqi sets ALTextToSpeech's and keeps its errors.
            .method_async("setLanguage", "(s)", "v", {
                let w = ctx();
                move |a| {
                    let w = w.clone();
                    async move {
                        let (d, name) = (get(&w)?, s(&a, 0)?);
                        d.node.call("ALTextToSpeech", "setLanguage", vec![name.as_str().into()]).await.map_err(|e| format!("\tsetLanguage::ALDialog\n{e}"))?;
                        d.dialog.st.lock().unwrap().language = Some(name);
                        Ok(Value::Void)
                    }
                }
            })
            .method("getLanguage", "()", "s", {
                let w = ctx();
                move |_| Ok(get(&w)?.dialog.st.lock().unwrap().language.clone().unwrap_or_else(|| "English".into()).into())
            })
            .method("setFocus", "(s)", "v", {
                let w = ctx();
                move |a| {
                    get(&w)?.dialog.st.lock().unwrap().focus = Some(s(&a, 0)?);
                    Ok(Value::Void)
                }
            })
            .method("getFocus", "()", "s", {
                let w = ctx();
                move |_| Ok(get(&w)?.dialog.st.lock().unwrap().focus.clone().unwrap_or_default().into())
            })
            .method("setConcept", "(ss[s])", "v", set_concept(ctx()))
            .method("setConcept", "(ss[s]b)", "v", set_concept(ctx()))
            .method("getConcept", "(ss)", "[s]", {
                let w = ctx();
                move |a| {
                    let d = get(&w)?;
                    let st = d.dialog.st.lock().unwrap();
                    names(st.dynamic.get(&(code(&s(&a, 1)?), s(&a, 0)?)).cloned().unwrap_or_default())
                }
            })
            // Without speech recognition, subscribers change nothing: typed input always answers.
            .method("subscribe", "(s)", "v", noop)
            .method("subscribe", "(sif)", "v", noop)
            .method("unsubscribe", "(s)", "v", noop)
            .signal("answered", "(s)")
            .signal("lastInput", "(s)"),
    );
}

fn set_concept(w: Weak<Robotd>) -> impl Fn(Vec<Value>) -> Result<Value, String> + Send + Sync + 'static {
    move |a| {
        let d = w.upgrade().ok_or("robot is shutting down")?;
        let words = a.get(2).map(|v| v.items().iter().filter_map(|x| x.as_str().map(String::from)).collect()).unwrap_or_default();
        d.dialog.st.lock().unwrap().dynamic.insert((code(&s(&a, 1)?), s(&a, 0)?), words);
        Ok(Value::Void)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn engine(top: &str) -> State {
        let t = parse(top).unwrap();
        let name = t.name.clone();
        State { topics: vec![Arc::new(t)], active: vec![name], ..Default::default() }
    }

    /// Steps of the answer to `input`, with `memory` as ALMemory.
    fn say(st: &mut State, input: &str, memory: &HashMap<&str, &str>) -> Option<Vec<Step>> {
        let (t, r, caps) = st.pick(Ok(&words(input)))?;
        let mem = |k: &str| memory.get(k).map(|v| v.to_string());
        let mut run = Run { st, t, caps, vars: HashMap::new(), memory: &mem, part: vec![], sets: vec![], steps: vec![] };
        run.rule(r, false)?;
        run.flush(true);
        run.steps.iter().any(|s| matches!(s, Step::Say(_))).then_some(run.steps)
    }

    fn said(st: &mut State, input: &str) -> String {
        let steps = say(st, input, &HashMap::new()).unwrap_or_default();
        steps.iter().filter_map(|s| if let Step::Say(t) = s { Some(strip(t)) } else { None }).collect::<Vec<_>>().join(" ").trim().to_string()
    }

    /// Choregraphe's NAOqi: extra words are tolerated anywhere, the rule with fewest wins; missing
    /// and different words are not; `*` needs a word; case and punctuation do not matter.
    #[test]
    fn matching_tolerates_extra_words_only() {
        let mut st = engine("topic: ~a()\nlanguage: enu\nu:(what is your name) N\nu:(hello) H\nu:(hello there) HT\nu:(star * end) S\n");
        for (input, answer) in [("what is your name please", "N"), ("please what is your nice name", "N"), ("hello you there", "HT"), ("there hello", "H"), ("HELLO!", "H"), ("star a b end", "S")] {
            assert_eq!(said(&mut st, input), answer, "{input}");
        }
        for input in ["what your name", "what is my name", "star end", "banana"] {
            assert!(st.pick(Ok(&words(input))).is_none(), "{input}");
        }
    }

    #[test]
    fn concepts_captures_and_variables() {
        let mut st = engine("topic: ~a()\nlanguage: enu\nconcept:(color) [red blue \"light green\"]\nu:(my name is _*) nice to meet you $1\nu:(set _~color) ok $col=$1 done\nu:(what color) $col\nu:(e:Ev) event $Ev\n");
        assert_eq!(said(&mut st, "my name is Bob Smith"), "nice to meet you Bob Smith", "captures keep the words as typed");
        let steps = say(&mut st, "set light green", &HashMap::new()).unwrap();
        assert_eq!(steps, vec![Step::Say("ok done \\last\\".into()), Step::Set("col".into(), "light green".into())]);
        assert!(say(&mut st, "what color", &HashMap::new()).is_none(), "an undefined variable says nothing");
        assert_eq!(say(&mut st, "what color", &[("col", "red")].into()).unwrap(), vec![Step::Say("red \\last\\".into())]);
        let (t, r, _) = st.pick(Err("Ev")).unwrap();
        assert_eq!(t.rules[r].output.len(), 2);
    }

    /// Outputs as NAOqi hands them to ALAnimatedSpeech: `,`/`!` split off, `\last\` before the
    /// final punctuation, a goto says the target in its own part tagged with its label.
    #[test]
    fn outputs_are_shaped_like_naoqi() {
        let mut st = engine("topic: ~s()\nlanguage: enu\nu:(a1) a\nu:(a4) Hello, world!\nu:(a7) fine! ^gotoReactivate(px)\nu:(a11) ^start(animations/Stand/Gestures/Hey_1)\nu:(a13) Hi \\rspd=50\\ there\nu:(pick) [a b c]\nproposal: %px question?\n");
        let says = |st: &mut State, i: &str| say(st, i, &HashMap::new()).unwrap();
        assert_eq!(says(&mut st, "a1"), vec![Step::Say("a".into())]);
        assert_eq!(says(&mut st, "a4"), vec![Step::Say("Hello , world \\last\\ !".into())]);
        assert_eq!(says(&mut st, "a7"), vec![Step::Say("fine !".into()), Step::Say("$Dialog/Tag=px question \\last\\?".into())]);
        assert_eq!(says(&mut st, "a11"), vec![Step::Say("^start(animations/Stand/Gestures/Hey_1)".into())]);
        assert_eq!(says(&mut st, "a13"), vec![Step::Say("Hi \\rspd=50\\ there \\last\\".into())]);
        assert_eq!(said(&mut st, "a4"), "Hello , world  !");
        let picks: Vec<String> = (0..4).map(|_| said(&mut st, "pick")).collect();
        assert_eq!(picks, ["a", "b", "c", "a"]);
    }

    /// Proposals are said once (`^goto` then fails, `^gotoReactivate` does not); the last answer's
    /// subrules win ties; `^nextProposal` takes the next proposal not yet said.
    #[test]
    fn proposals_and_subrule_scope() {
        let mut st = engine("topic: ~p()\nlanguage: enu\nu:(yes) top yes\nu:(go one) ^goto(one)\nu:(again one) ^gotoReactivate(one)\nu:(next) ^nextProposal\nu:(other) other\nproposal: %one P1\n    u1:(yes) Y1\n        u2:(really) R2\nproposal: %two P2\n");
        assert_eq!(said(&mut st, "go one"), "$Dialog/Tag=one P1");
        assert_eq!(said(&mut st, "banana"), "");
        assert_eq!(said(&mut st, "yes"), "Y1");
        assert_eq!(said(&mut st, "really"), "R2");
        assert_eq!(said(&mut st, "yes"), "top yes");
        assert!(say(&mut st, "go one", &HashMap::new()).is_none());
        assert_eq!(said(&mut st, "again one"), "$Dialog/Tag=one P1");
        assert_eq!(said(&mut st, "next"), "$Dialog/Tag=two P2");
        assert!(say(&mut st, "next", &HashMap::new()).is_none());
        assert_eq!(said(&mut st, "again one"), "$Dialog/Tag=one P1");
        assert_eq!(said(&mut st, "other"), "other");
        assert_eq!(said(&mut st, "yes"), "top yes", "another answer leaves the proposal's scope");
        let mut st = engine("topic: ~q()\nlanguage: enu\nu:(sad) Oh, I feel sorry for you. ^nextProposal\n");
        assert_eq!(said(&mut st, "sad"), "Oh , I feel sorry for you .", "no proposal left: the text is still said");
    }

    /// Measured: a subrule only wins a tie; an exact top-level rule beats it.
    #[test]
    fn scope_breaks_ties_only() {
        let mut st = engine("topic: ~k()\nlanguage: enu\nu:(ask) question?\n    u1:(yes) sub yes\n    u1:(maybe) sub maybe\nu:(yes please) top yes please\nu:(maybe) top maybe\n");
        for (input, answer) in [("ask", "question ?"), ("yes please", "top yes please"), ("ask", "question ?"), ("maybe", "sub maybe"), ("maybe", "top maybe")] {
            assert_eq!(said(&mut st, input), answer, "{input}");
        }
    }

    /// Measured: `g2`'s `eat ~fruit` used `g1`'s fruits, `g1` being loaded first.
    #[test]
    fn concepts_are_shared_by_the_language() {
        let mut st = engine("topic: ~g1()\nlanguage: enu\nconcept:(fruit) [apple pear]\nu:(~fruit) ~fruit\n");
        let g2 = parse("topic: ~g2()\nlanguage: enu\nconcept:(fruit) [banana kiwi]\nu:(eat ~fruit) yum ~fruit\n").unwrap();
        st.topics.push(Arc::new(g2));
        st.active.push("g2".into());
        assert_eq!(said(&mut st, "eat apple"), "yum apple");
        assert!(st.pick(Ok(&words("eat banana"))).is_none());
    }

    #[test]
    fn parse_errors_name_the_line() {
        assert_eq!(parse("topic: ~x()\nlanguage: enu\nu:(a b\n").unwrap_err().0, 3);
        assert_eq!(parse("topic: ~x()\n  u1:(a) b\n").unwrap_err().0, 2);
    }
}
