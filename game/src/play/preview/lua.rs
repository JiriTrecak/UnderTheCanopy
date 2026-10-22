//! What a scene preview reads from a mission's Lua without running it: the scenes, a
//! scene's body, the calls it makes with literal arguments, what the script does just
//! before it plays a scene, and which variables hold a unit's position.
//!
//! Missions write their functions at the top level, each closed by an `end` in the first
//! column (docs/campaign.md "Scenes"); that is all this reading relies on.

/// A value a script sets with a literal expression (or one a preview can answer).
#[derive(Debug, Clone, PartialEq)]
pub enum Value {
    Nil,
    Boolean(bool),
    Integer(i64),
    Number(f64),
    Text(String),
    /// `mission.alive("<placement>")`.
    Alive(String),
    /// A condition a preview takes as met (`mission.in_region(…)`).
    Met,
    /// `mission.tick()`.
    Tick,
}

/// What a script does in the lines that lead to `play("<scene>")`, and the units the
/// condition of their block needs gone.
#[derive(Debug, Default, Clone, PartialEq)]
pub struct Lead {
    pub sets: Vec<(String, Value)>,
    pub spawns: Vec<String>,
    pub gone: Vec<String>,
}

/// A variable the script sets from a unit's position (`local x, y =
/// mission.position("queen")` then `mission.set("qx", x)`).
#[derive(Debug, Clone, PartialEq)]
pub struct PositionVariable {
    pub variable: String,
    pub unit: String,
    /// 0: x, 1: y.
    pub axis: usize,
}

/// The line without its comment.
fn code(line: &str) -> &str {
    let mut quote = None;
    let bytes = line.as_bytes();
    for (i, &c) in bytes.iter().enumerate() {
        match quote {
            Some(q) if c == q && (i == 0 || bytes[i - 1] != b'\\') => quote = None,
            Some(_) => {}
            None if c == b'"' || c == b'\'' => quote = Some(c),
            None if c == b'-' && bytes.get(i + 1) == Some(&b'-') => return &line[..i],
            None => {}
        }
    }
    line
}

/// The scene functions, in the order the script defines them.
pub fn scenes(source: &str) -> Vec<String> {
    source
        .lines()
        .filter_map(|line| {
            let name = line
                .strip_prefix("function scenes.")
                .and_then(|rest| rest.split_once('('))
                .map(|(name, _)| name)
                .or_else(|| {
                    line.strip_prefix("scenes.")
                        .and_then(|rest| rest.split_once('='))
                        .filter(|(_, value)| value.trim_start().starts_with("function"))
                        .map(|(name, _)| name)
                })?
                .trim();
            (!name.is_empty()).then(|| name.to_owned())
        })
        .collect()
}

/// The body of a top-level function: `on_start` or `scenes.<name>`.
pub fn body<'a>(source: &'a str, function: &str) -> Option<&'a str> {
    let named = |line: &str| {
        line.strip_prefix("function ")
            .and_then(|rest| rest.strip_prefix(function))
            .is_some_and(|rest| rest.trim_start().starts_with('('))
            || line
                .strip_prefix(function)
                .and_then(|rest| rest.trim_start().strip_prefix('='))
                .is_some_and(|rest| rest.trim_start().starts_with("function"))
    };
    let start = line_starts(source).find(|&(_, line)| named(line))?.0;
    let rest = &source[start..];
    let header = rest.find('\n').map_or(rest.len(), |n| n + 1);
    let end = line_starts(&rest[header..])
        .find(|&(_, line)| line.trim_end() == "end")
        .map_or(rest.len(), |(at, _)| header + at);
    Some(&rest[header..end])
}

/// Each line with its byte offset.
fn line_starts(text: &str) -> impl Iterator<Item = (usize, &str)> {
    let mut at = 0;
    text.split_inclusive('\n').map(move |line| {
        let start = at;
        at += line.len();
        (start, line.trim_end_matches(['\n', '\r']))
    })
}

/// The first argument of every call to `function` in `text` that is a string literal, in
/// order (`calls(body, "mission.get")`).
pub fn calls(text: &str, function: &str) -> Vec<String> {
    let mut found = Vec::new();
    for line in text.lines().map(code) {
        let mut rest = line;
        while let Some(at) = rest.find(function) {
            let before = rest[..at].chars().next_back();
            let after = &rest[at + function.len()..];
            rest = after;
            // `play` is not the end of `mission.display`, nor `get` of `get_x`.
            if before.is_some_and(|c| c.is_alphanumeric() || c == '_') {
                continue;
            }
            let Some(args) = after.trim_start().strip_prefix('(') else {
                continue;
            };
            if let Some(text) = literal(args.trim_start()) {
                found.push(text.to_owned());
            }
        }
    }
    found
}

/// The string literal `text` starts with.
fn literal(text: &str) -> Option<&str> {
    let quote = text.chars().next().filter(|c| *c == '"' || *c == '\'')?;
    let inner = &text[1..];
    inner.find(quote).map(|end| &inner[..end])
}

/// Every string literal in `text`.
pub fn literals(text: &str) -> Vec<String> {
    let mut found = Vec::new();
    for line in text.lines().map(code) {
        let mut rest = line;
        while let Some(at) = rest.find(['"', '\'']) {
            let Some(text) = literal(&rest[at..]) else {
                break;
            };
            found.push(text.to_owned());
            rest = &rest[at + text.len() + 2..];
        }
    }
    found
}

fn indent(line: &str) -> usize {
    line.len() - line.trim_start().len()
}

/// What the script sets and spawns in the lines that lead to each `play("<scene>")`: the
/// lines before it in its block, nested ones included.
pub fn lead(source: &str, scene: &str) -> Lead {
    let lines: Vec<&str> = source.lines().map(code).collect();
    let mut lead = Lead::default();
    for (at, line) in lines.iter().enumerate() {
        if !calls(line, "play").iter().any(|s| s == scene) {
            continue;
        }
        let depth = indent(line);
        for before in lines[..at].iter().rev() {
            if before.trim().is_empty() {
                continue;
            }
            if indent(before) < depth {
                // The block's condition: units it needs gone (`not mission.alive("…")`),
                // when nothing else (`or`) could have met it.
                let condition = before.trim();
                if (condition.starts_with("if ") || condition.starts_with("elseif "))
                    && !condition.contains(" or ")
                {
                    for gone in condition.split("not ").skip(1) {
                        if let Some(args) = gone.trim_start().strip_prefix("mission.alive(")
                            && let Some(unit) = literal(args.trim_start())
                            && !lead.gone.iter().any(|g| g == unit)
                        {
                            lead.gone.push(unit.to_owned());
                        }
                    }
                }
                break;
            }
            let call = before.trim();
            if let Some(args) = call.strip_prefix("mission.set(")
                && let Some(variable) = literal(args)
            {
                let value = args[variable.len() + 2..]
                    .trim_start()
                    .strip_prefix(',')
                    .and_then(|v| v.trim().strip_suffix(')'))
                    .and_then(value);
                if let Some(value) = value {
                    lead.sets.insert(0, (variable.to_owned(), value));
                }
            }
            lead.spawns.splice(0..0, calls(call, "mission.spawn"));
        }
    }
    lead
}

/// A value written in Lua, when a preview can answer it.
fn value(text: &str) -> Option<Value> {
    let text = text.trim();
    Some(match text {
        "nil" => Value::Nil,
        "true" => Value::Boolean(true),
        "false" => Value::Boolean(false),
        "mission.tick()" => Value::Tick,
        _ if text.starts_with("mission.in_region(") => Value::Met,
        _ => {
            if let Some(args) = text.strip_prefix("mission.alive(") {
                return literal(args.trim_start()).map(|unit| Value::Alive(unit.to_owned()));
            }
            if let Some(text) = literal(text) {
                return Some(Value::Text(text.to_owned()));
            }
            if let Ok(n) = text.parse::<i64>() {
                return Some(Value::Integer(n));
            }
            return text.parse::<f64>().ok().map(Value::Number);
        }
    })
}

/// The variables the script sets from a unit's position.
pub fn position_variables(source: &str) -> Vec<PositionVariable> {
    let mut locals: Vec<(String, String, usize)> = Vec::new();
    let mut found: Vec<PositionVariable> = Vec::new();
    for line in source.lines().map(code) {
        let line = line.trim();
        if let Some((names, call)) = line
            .strip_prefix("local ")
            .and_then(|rest| rest.split_once('='))
            && let Some(args) = call.trim().strip_prefix("mission.position(")
            && let Some(unit) = literal(args.trim_start())
        {
            for (axis, name) in names.split(',').map(str::trim).enumerate().take(2) {
                locals.retain(|(n, ..)| n != name);
                locals.push((name.to_owned(), unit.to_owned(), axis));
            }
            continue;
        }
        if let Some(args) = line.strip_prefix("mission.set(")
            && let Some(variable) = literal(args)
            && let Some(name) = args[variable.len() + 2..]
                .trim_start()
                .strip_prefix(',')
                .and_then(|v| v.trim().strip_suffix(')'))
            && let Some((_, unit, axis)) = locals.iter().rev().find(|(n, ..)| n == name.trim())
            && !found
                .iter()
                .any(|p| p.variable == variable && p.unit == *unit)
        {
            found.push(PositionVariable {
                variable: variable.to_owned(),
                unit: unit.clone(),
                axis: *axis,
            });
        }
    }
    found
}

#[cfg(test)]
mod tests {
    use super::*;

    const SCRIPT: &str = r#"
local function play(name)
  mission.set("ok", mission.alive("guide"))
  local x, y = mission.position("hero")
  mission.set("hx", x)
  mission.set("hy", y)
  mission.play(name)
end

function on_start()
  mission.fade_out(0) -- black first
  play("opening")
end

function on_tick()
  if not mission.get("met") and mission.in_region("hero", "camp") and not mission.alive("gate") then
    mission.set("met", true)
    mission.set("near", mission.in_region("hero", "fire"))
    mission.set("talk", nil)
    mission.spawn("door")
    if mission.alive("guide") then
      mission.set("count", 2)
    end
    mission.play("meeting", false)
  end
end

function scenes.opening()
  mission.speak("hero", "Here -- at last.", nil, "Hero")
end

function scenes.meeting()
  mission.cinematic(true)
  if mission.get("ok") then
    mission.move("guide", mission.get("hx") + 2, 10)
  end
  mission.spawn("stranger")
  mission.speak("stranger", "Who goes there?", nil, "Stranger")
end
"#;

    #[test]
    fn reads_scenes_and_bodies() {
        assert_eq!(scenes(SCRIPT), ["opening", "meeting"]);
        let opening = body(SCRIPT, "on_start").unwrap();
        assert_eq!(calls(opening, "play"), ["opening"]);
        let meeting = body(SCRIPT, "scenes.meeting").unwrap();
        assert!(meeting.contains("Who goes there?"));
        assert!(!meeting.contains("function"));
        assert_eq!(calls(meeting, "mission.get"), ["ok", "hx"]);
        assert_eq!(calls(meeting, "mission.spawn"), ["stranger"]);
        assert!(literals(meeting).iter().any(|l| l == "guide"));
        // A comment marker inside a string is text.
        let opening = body(SCRIPT, "scenes.opening").unwrap();
        assert_eq!(literals(opening), ["hero", "Here -- at last.", "Hero"]);
    }

    #[test]
    fn reads_what_leads_to_a_scene() {
        let lead = lead(SCRIPT, "meeting");
        assert_eq!(
            lead.sets,
            [
                ("met".to_owned(), Value::Boolean(true)),
                ("near".to_owned(), Value::Met),
                ("talk".to_owned(), Value::Nil),
                ("count".to_owned(), Value::Integer(2)),
            ]
        );
        assert_eq!(lead.spawns, ["door"]);
        assert_eq!(lead.gone, ["gate"]);
        assert_eq!(
            position_variables(SCRIPT),
            [
                PositionVariable {
                    variable: "hx".into(),
                    unit: "hero".into(),
                    axis: 0
                },
                PositionVariable {
                    variable: "hy".into(),
                    unit: "hero".into(),
                    axis: 1
                },
            ]
        );
    }
}
