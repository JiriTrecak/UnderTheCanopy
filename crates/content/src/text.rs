//! The text library (D96): every player-facing string, keyed, in each package's
//! `text/<language>/*.json` (flat maps of key → string; files and folders only organise).
//! Packages layer in load order, so a later package's key overrides an earlier one's.
//!
//! A string is plain text with the interface's markup, `{name}` placeholders and ICU
//! plurals: `{count, plural, one {# hero} other {# heroes}}` (`#` is the number;
//! `=0 {…}` matches a number exactly). [`Text::format`] fills them when the string is
//! shown; nothing stores formatted text.

use crate::{error::Error, package::Package};
use serde_json::Value;
use std::collections::BTreeMap;

/// The source language every key exists in.
pub const SOURCE_LANGUAGE: &str = "en";

/// The folder of a language's text in a package (`text/en`).
pub fn folder(language: &str) -> String {
    format!("text/{language}")
}

/// A value filling a placeholder.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Arg<'a> {
    Text(&'a str),
    Number(f64),
}

impl<'a> From<&'a str> for Arg<'a> {
    fn from(text: &'a str) -> Self {
        Arg::Text(text)
    }
}

impl<'a> From<&'a String> for Arg<'a> {
    fn from(text: &'a String) -> Self {
        Arg::Text(text)
    }
}

macro_rules! number_arg {
    ($($t:ty),*) => {$(
        impl From<$t> for Arg<'_> {
            fn from(n: $t) -> Self {
                Arg::Number(n as f64)
            }
        }
    )*};
}
number_arg!(u8, u16, u32, u64, usize, i32, i64, f32, f64);

/// One language's strings, resolved across packages.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Text {
    pub language: String,
    entries: BTreeMap<String, String>,
    /// The file that last set each key (for the editor and errors).
    files: BTreeMap<String, String>,
}

impl Text {
    /// A library holding `entries` (tests and tools).
    pub fn from_entries(
        language: &str,
        entries: impl IntoIterator<Item = (String, String)>,
    ) -> Self {
        Self {
            language: language.to_string(),
            entries: entries.into_iter().collect(),
            files: BTreeMap::new(),
        }
    }

    /// Reads `language` from `packages` in load order; a later package overrides a key,
    /// two files of one package setting the same key is an error.
    pub fn read(packages: &[Package], language: &str, errors: &mut Vec<Error>) -> Self {
        let mut text = Self {
            language: language.to_string(),
            ..Self::default()
        };
        text.layer(packages, errors);
        text
    }

    /// Reads `packages` on top of this library (their keys override).
    pub fn layer(&mut self, packages: &[Package], errors: &mut Vec<Error>) {
        let dir = folder(&self.language);
        let text = self;
        for package in packages {
            let mut mine: BTreeMap<String, String> = BTreeMap::new();
            for (path, value) in documents(package, &dir, errors) {
                let file = package.label(&path);
                let Value::Object(map) = value else {
                    errors.push(Error::new(
                        &file,
                        "",
                        "a text file is an object of key → string",
                    ));
                    continue;
                };
                for (key, value) in map {
                    let Value::String(string) = value else {
                        errors.push(Error::new(&file, &key, "a text entry is a string"));
                        continue;
                    };
                    if let Err(e) = check_key(&key) {
                        errors.push(Error::new(&file, &key, e));
                        continue;
                    }
                    if let Err(e) = check_string(&string) {
                        errors.push(Error::new(&file, &key, e));
                    }
                    if let Some(other) = mine.get(&key) {
                        errors.push(Error::new(
                            &file,
                            &key,
                            format!("this key is also set in {other}"),
                        ));
                        continue;
                    }
                    mine.insert(key.clone(), file.clone());
                    text.files.insert(key.clone(), file.clone());
                    text.entries.insert(key, string);
                }
            }
        }
    }

    /// The string for `key`, when the library has it.
    pub fn get(&self, key: &str) -> Option<&str> {
        self.entries.get(key).map(String::as_str)
    }

    /// The string for `key`, or the key itself where it is missing (visible, never empty).
    pub fn text<'a>(&'a self, key: &'a str) -> &'a str {
        self.get(key).unwrap_or(key)
    }

    /// The string for `key` with its placeholders filled from `args`.
    pub fn format(&self, key: &str, args: &[(&str, Arg)]) -> String {
        match self.get(key) {
            Some(string) => format(string, args),
            None => key.to_string(),
        }
    }

    /// The file that set `key`.
    pub fn file(&self, key: &str) -> Option<&str> {
        self.files.get(key).map(String::as_str)
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Every key and string, in key order.
    pub fn entries(&self) -> impl Iterator<Item = (&str, &str)> {
        self.entries.iter().map(|(k, v)| (k.as_str(), v.as_str()))
    }
}

/// A message by key with its arguments, carried as a string where the simulation keeps
/// text (facts, refusals): saves and replays hold no English, and each player reads it in
/// their own language ([`Text::message`]). A string that is not an encoded message is
/// shown as it is (older saves).
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Message {
    #[serde(rename = "k")]
    pub key: String,
    #[serde(rename = "a", default, skip_serializing_if = "Vec::is_empty")]
    pub args: Vec<(String, MessageArg)>,
}

/// An argument of a [`Message`]: plain text, a number, or an element whose name the
/// reader's text library gives (`<id>.name`).
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum MessageArg {
    Text(String),
    Number(f64),
    Name(String),
}

impl From<&str> for MessageArg {
    fn from(text: &str) -> Self {
        Self::Text(text.to_string())
    }
}

impl From<String> for MessageArg {
    fn from(text: String) -> Self {
        Self::Text(text)
    }
}

impl From<&String> for MessageArg {
    fn from(text: &String) -> Self {
        Self::Text(text.clone())
    }
}

macro_rules! number_message_arg {
    ($($t:ty),*) => {$(
        impl From<$t> for MessageArg {
            fn from(n: $t) -> Self {
                Self::Number(n as f64)
            }
        }
    )*};
}
number_message_arg!(u8, u16, u32, u64, usize, i32, i64, f32, f64);

/// The element `id`'s name as a message argument.
pub fn name(id: impl std::fmt::Display) -> MessageArg {
    MessageArg::Name(id.to_string())
}

/// What starts an encoded message.
const MESSAGE_MARK: char = '\u{1}';

impl Message {
    pub fn new(key: &str) -> Self {
        Self {
            key: key.to_string(),
            args: Vec::new(),
        }
    }

    pub fn with(mut self, name: &str, arg: impl Into<MessageArg>) -> Self {
        self.args.push((name.to_string(), arg.into()));
        self
    }

    /// The message as a string the simulation can keep.
    pub fn encode(&self) -> String {
        let json = serde_json::to_string(self).unwrap_or_default();
        format!("{MESSAGE_MARK}{json}")
    }

    /// A string [`encode`](Self::encode)d, if it is one.
    pub fn decode(string: &str) -> Option<Self> {
        serde_json::from_str(string.strip_prefix(MESSAGE_MARK)?).ok()
    }
}

impl Text {
    /// Text the simulation keeps, as this library reads it: a key, an encoded
    /// [`Message`], or anything else as it is (D96).
    pub fn say(&self, text: &str) -> String {
        self.get(text)
            .map_or_else(|| self.message(text), str::to_string)
    }

    /// A message as this library reads it: an encoded [`Message`] formatted with its
    /// arguments (names looked up), anything else as it is.
    pub fn message(&self, string: &str) -> String {
        let Some(m) = Message::decode(string) else {
            return string.to_string();
        };
        let owned: Vec<(String, String, Option<f64>)> = m
            .args
            .iter()
            .map(|(n, a)| match a {
                MessageArg::Text(t) => (n.clone(), t.clone(), None),
                MessageArg::Number(x) => (n.clone(), String::new(), Some(*x)),
                MessageArg::Name(id) => (
                    n.clone(),
                    self.get(&format!("{id}.name"))
                        .map_or_else(|| id.clone(), str::to_string),
                    None,
                ),
            })
            .collect();
        let args: Vec<(&str, Arg)> = owned
            .iter()
            .map(|(n, t, x)| (n.as_str(), x.map_or(Arg::Text(t.as_str()), Arg::Number)))
            .collect();
        self.format(&m.key, &args)
    }
}

/// Every `.json` document beneath `dir` in `package`, in path order.
fn documents(package: &Package, dir: &str, errors: &mut Vec<Error>) -> Vec<(String, Value)> {
    let mut out = Vec::new();
    if !package.is_dir(dir) {
        return out;
    }
    let mut stack = vec![dir.to_string()];
    while let Some(dir) = stack.pop() {
        let mut listed = match package.entries(&dir) {
            Ok(listed) => listed,
            Err(e) => {
                errors.push(Error::new(&package.label(&dir), "", e.to_string()));
                continue;
            }
        };
        listed.retain(|(name, _)| !name.starts_with('.'));
        listed.sort();
        for (name, is_dir) in listed.into_iter().rev() {
            let path = format!("{dir}/{name}");
            if is_dir {
                stack.push(path);
            } else if name.ends_with(".json") {
                match package
                    .read_text(&path)
                    .map_err(|e| e.to_string())
                    .and_then(|t| serde_json::from_str(&t).map_err(|e| e.to_string()))
                {
                    Ok(value) => out.push((path, value)),
                    Err(e) => errors.push(Error::new(&package.label(&path), "", e)),
                }
            }
        }
    }
    out.sort_by(|a, b| a.0.cmp(&b.0));
    out
}

/// A key is words of letters, digits, dashes and underscores, joined by dots (content keys
/// mirror their element's id exactly, `command.cancelUpgrade.name`).
pub fn check_key(key: &str) -> Result<(), String> {
    let ok = !key.is_empty()
        && key.split('.').all(|part| {
            !part.is_empty()
                && part
                    .chars()
                    .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
        });
    if ok {
        Ok(())
    } else {
        Err("a key is words of letters, digits, dashes and underscores, joined by dots".into())
    }
}

/// Whether a field's string reads as a library key rather than text: words joined by dots,
/// no spaces (`campaign.vanguard.prologue.title`).
pub fn looks_like_key(string: &str) -> bool {
    string.contains('.') && check_key(string).is_ok()
}

/// A string's placeholders parse.
pub fn check_string(string: &str) -> Result<(), String> {
    parse(string).map(|_| ())
}

/// The placeholders a string uses, in order (`{name}` and plural selectors).
pub fn placeholders(string: &str) -> Vec<String> {
    let mut out = Vec::new();
    if let Ok(parts) = parse(string) {
        collect(&parts, &mut out);
    }
    out
}

fn collect(parts: &[Part], out: &mut Vec<String>) {
    for part in parts {
        match part {
            Part::Literal(_) | Part::Number => {}
            Part::Arg(name) => {
                if !out.contains(name) {
                    out.push(name.clone());
                }
            }
            Part::Plural(name, branches) => {
                if !out.contains(name) {
                    out.push(name.clone());
                }
                for (_, branch) in branches {
                    collect(branch, out);
                }
            }
        }
    }
}

/// Fills `string`'s placeholders from `args`; a missing argument stays as `{name}`.
pub fn format(string: &str, args: &[(&str, Arg)]) -> String {
    match parse(string) {
        Ok(parts) => {
            let mut out = String::with_capacity(string.len());
            render(&parts, args, None, &mut out);
            out
        }
        Err(_) => string.to_string(),
    }
}

#[derive(Debug, Clone, PartialEq)]
enum Part {
    Literal(String),
    Arg(String),
    /// `#` inside a plural branch.
    Number,
    /// Selector and its branches (`one`, `other`, `=0`, …).
    Plural(String, Vec<(String, Vec<Part>)>),
}

fn parse(string: &str) -> Result<Vec<Part>, String> {
    let chars: Vec<char> = string.chars().collect();
    let mut at = 0;
    let parts = parse_parts(&chars, &mut at, false)?;
    if at < chars.len() {
        return Err("an unmatched `}`".into());
    }
    Ok(parts)
}

fn parse_parts(chars: &[char], at: &mut usize, in_branch: bool) -> Result<Vec<Part>, String> {
    let mut parts = Vec::new();
    let mut literal = String::new();
    while *at < chars.len() {
        match chars[*at] {
            '{' => {
                if !literal.is_empty() {
                    parts.push(Part::Literal(std::mem::take(&mut literal)));
                }
                *at += 1;
                parts.push(parse_placeholder(chars, at)?);
            }
            '}' => {
                if in_branch {
                    break;
                }
                return Err("an unmatched `}`".into());
            }
            '#' if in_branch => {
                if !literal.is_empty() {
                    parts.push(Part::Literal(std::mem::take(&mut literal)));
                }
                parts.push(Part::Number);
                *at += 1;
            }
            c => {
                literal.push(c);
                *at += 1;
            }
        }
    }
    if !literal.is_empty() {
        parts.push(Part::Literal(literal));
    }
    Ok(parts)
}

fn word(chars: &[char], at: &mut usize) -> String {
    while *at < chars.len() && chars[*at] == ' ' {
        *at += 1;
    }
    let mut out = String::new();
    while *at < chars.len() && !matches!(chars[*at], ',' | '{' | '}' | ' ') {
        out.push(chars[*at]);
        *at += 1;
    }
    while *at < chars.len() && chars[*at] == ' ' {
        *at += 1;
    }
    out
}

fn parse_placeholder(chars: &[char], at: &mut usize) -> Result<Part, String> {
    let name = word(chars, at);
    if name.is_empty() || !name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_') {
        return Err(format!(
            "a placeholder needs a name (`{{name}}`), found `{name}`"
        ));
    }
    match chars.get(*at) {
        Some('}') => {
            *at += 1;
            Ok(Part::Arg(name))
        }
        Some(',') => {
            *at += 1;
            let kind = word(chars, at);
            if kind != "plural" {
                return Err(format!("`{{{name}, {kind}}}`: only `plural` is supported"));
            }
            if chars.get(*at) != Some(&',') {
                return Err(format!("`{{{name}, plural}}` needs its branches"));
            }
            *at += 1;
            let mut branches = Vec::new();
            loop {
                let selector = word(chars, at);
                if selector.is_empty() {
                    break;
                }
                if chars.get(*at) != Some(&'{') {
                    return Err(format!("plural branch `{selector}` needs `{{…}}`"));
                }
                *at += 1;
                let branch = parse_parts(chars, at, true)?;
                if chars.get(*at) != Some(&'}') {
                    return Err(format!("plural branch `{selector}` is not closed"));
                }
                *at += 1;
                branches.push((selector, branch));
            }
            if chars.get(*at) != Some(&'}') {
                return Err(format!("`{{{name}, plural, …}}` is not closed"));
            }
            *at += 1;
            if !branches.iter().any(|(s, _)| s == "other") {
                return Err(format!("`{{{name}, plural, …}}` needs an `other` branch"));
            }
            Ok(Part::Plural(name, branches))
        }
        _ => Err(format!("`{{{name}` is not closed")),
    }
}

fn number_text(n: f64) -> String {
    if n.fract() == 0.0 && n.abs() < 1e15 {
        format!("{}", n as i64)
    } else {
        format!("{n}")
    }
}

/// English plural categories (the source language; translations add their rules).
fn category(n: f64) -> &'static str {
    if n == 1.0 { "one" } else { "other" }
}

fn render(parts: &[Part], args: &[(&str, Arg)], number: Option<f64>, out: &mut String) {
    let arg = |name: &str| args.iter().find(|(n, _)| *n == name).map(|(_, a)| *a);
    for part in parts {
        match part {
            Part::Literal(text) => out.push_str(text),
            Part::Number => out.push_str(&number.map(number_text).unwrap_or_else(|| "#".into())),
            Part::Arg(name) => match arg(name) {
                Some(Arg::Text(text)) => out.push_str(text),
                Some(Arg::Number(n)) => out.push_str(&number_text(n)),
                None => {
                    out.push('{');
                    out.push_str(name);
                    out.push('}');
                }
            },
            Part::Plural(name, branches) => {
                let n = match arg(name) {
                    Some(Arg::Number(n)) => n,
                    Some(Arg::Text(t)) => t.parse().unwrap_or(0.0),
                    None => 0.0,
                };
                let exact = format!("={}", number_text(n));
                let branch = branches
                    .iter()
                    .find(|(s, _)| *s == exact)
                    .or_else(|| branches.iter().find(|(s, _)| s == category(n)))
                    .or_else(|| branches.iter().find(|(s, _)| s == "other"));
                if let Some((_, branch)) = branch {
                    render(branch, args, Some(n), out);
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn placeholders_and_plurals_format() {
        let s = "{name} has {count, plural, =0 {no heroes} one {# hero} other {# heroes}}.";
        assert_eq!(
            format(s, &[("name", "Aurelith".into()), ("count", 1.into())]),
            "Aurelith has 1 hero."
        );
        assert_eq!(
            format(s, &[("name", "Kesh".into()), ("count", 3.into())]),
            "Kesh has 3 heroes."
        );
        assert_eq!(
            format(s, &[("name", "Tiv".into()), ("count", 0.into())]),
            "Tiv has no heroes."
        );
        assert_eq!(format("{missing} stays", &[]), "{missing} stays");
        assert_eq!(
            placeholders(s),
            vec!["name".to_string(), "count".to_string()]
        );
    }

    #[test]
    fn markup_and_hash_outside_plurals_stay_literal() {
        let s = "[color=#fcd34d]Mission complete[/color] #1";
        assert_eq!(format(s, &[]), s);
    }

    #[test]
    fn messages_travel_as_strings_and_read_by_key() {
        let text = Text::from_entries(
            "en",
            [
                (
                    "message.build-within".to_string(),
                    "Build within {cells} cells of {name}".to_string(),
                ),
                ("building.x.name".to_string(), "Hall".to_string()),
            ],
        );
        let m = Message::new("message.build-within")
            .with("cells", 12u32)
            .with("name", name("building.x"))
            .encode();
        assert_eq!(text.message(&m), "Build within 12 cells of Hall");
        assert_eq!(text.message("Plain old text"), "Plain old text");
    }

    #[test]
    fn broken_strings_are_reported() {
        assert!(check_string("{name").is_err());
        assert!(check_string("{n, plural, one {x}}").is_err());
        assert!(check_string("{n, select, a {x} other {y}}").is_err());
        assert!(check_string("a } b").is_err());
        assert!(check_key("unit.ants.warrior.name").is_ok());
        assert!(check_key("unit..ants").is_err());
        assert!(check_key("unit ants").is_err());
    }
}
