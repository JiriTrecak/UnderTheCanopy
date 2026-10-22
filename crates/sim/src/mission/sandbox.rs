//! The Lua sandbox: vendored Lua 5.4 through mlua, a fresh library-free state per run.
//!
//! No standard library is opened, so scripts reach nothing but the core language, the
//! `mission` table and the `scenes` table they fill: no filesystem, clock, randomness, metatables, coroutines, `pcall` or
//! table iteration. Lua's string-hash seed only affects iteration order, which is
//! therefore unobservable. Every run is bounded by an instruction count (deterministic:
//! it counts VM instructions, never time) and a string size.

use super::hook;
use mlua::{Lua, LuaOptions, MultiValue, StdLib, Table, Value, chunk::ChunkMode};
use std::cell::{Cell, RefCell};

/// VM instructions per run, top-level chunk included.
pub const INSTRUCTION_LIMIT: u32 = 20_000;
/// Bytes in any string held by the running function.
pub const STRING_LIMIT: usize = 65_536;
/// Script length in UTF-16 code units, as maps measure it.
pub const SCRIPT_LIMIT: usize = 64_000;
/// The chunk name: errors read `[string "mission.lua"]:3: …`.
const CHUNK: &str = "mission.lua";

/// A value crossing the sandbox boundary. Scripts and the API exchange only these.
#[derive(Debug, Clone, PartialEq)]
pub enum Scalar {
    Nil,
    Boolean(bool),
    Integer(i64),
    Number(f64),
    String(String),
}

impl Scalar {
    /// A number for a script: integral values become Lua integers, so `"x" .. n` reads
    /// `x3`, not `x3.0`.
    pub fn number(value: f64) -> Self {
        if value.fract() == 0.0 && value.abs() < 9.007_199_254_740_992e15 {
            Self::Integer(value as i64)
        } else {
            Self::Number(value)
        }
    }

    pub fn is_nil(&self) -> bool {
        matches!(self, Self::Nil)
    }
}

/// The result of a successful run.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Ran {
    /// VM instructions executed, top-level chunk included.
    pub instructions: u32,
    /// The scene the host asked for: `Some(true)` when its function returned, `Some(false)`
    /// when a wait ended the run first.
    pub scene: Option<bool>,
}

/// What a host error must read for the sandbox to treat it as a scene's wait, not a
/// failure: the scene's function stops there and the run succeeds.
pub const SUSPEND: &str = "\u{1}scene waits";

/// The other side of the sandbox: the API's calls, and the scene to play after the
/// callback.
pub trait Host {
    fn call(&mut self, name: &'static str, args: Vec<Scalar>) -> Result<Vec<Scalar>, String>;

    /// The function of the script's `scenes` table to call after the callback, asked once
    /// the callback returned; calls from then on come from the scene.
    fn scene(&mut self) -> Option<String> {
        None
    }
}

struct Calls<'a>(&'a mut dyn FnMut(&'static str, Vec<Scalar>) -> Result<Vec<Scalar>, String>);

impl Host for Calls<'_> {
    fn call(&mut self, name: &'static str, args: Vec<Scalar>) -> Result<Vec<Scalar>, String> {
        (self.0)(name, args)
    }
}

/// Runs `source`'s top level, then the global function `callback` if the script defines
/// one. Every `mission.<name>` call for a name in `api` goes to `host` with its arguments;
/// API calls from the top level are refused. Errors carry Lua's location prefix.
pub fn run(
    source: &str,
    callback: &str,
    api: &[&'static str],
    host: &mut dyn FnMut(&'static str, Vec<Scalar>) -> Result<Vec<Scalar>, String>,
) -> Result<Ran, String> {
    execute(source, Some(callback), api, &mut Calls(host))
}

/// [`run`] for a mission: the callback (none for a run that only plays the scene), then
/// the scene the host names, `scenes[name]()`, whose waits may end the run early.
pub fn execute(
    source: &str,
    callback: Option<&str>,
    api: &[&'static str],
    host: &mut dyn Host,
) -> Result<Ran, String> {
    let lua = state().map_err(|e| message(&e))?;
    hook::install(&lua).map_err(|e| message(&e))?;
    let active = Cell::new(false);
    let suspended = Cell::new(false);
    let failure = RefCell::new(None::<String>);
    let host = RefCell::new(host);
    let mut scene = None;
    let result = lua.scope(|scope| {
        let table = lua.create_table()?;
        for &name in api {
            let (active, suspended, failure, host) = (&active, &suspended, &failure, &host);
            let function = scope.create_function(move |lua, args: MultiValue| {
                let outcome = if active.get() {
                    args.into_iter()
                        .map(scalar)
                        .collect::<Result<Vec<_>, _>>()
                        .and_then(|args| host.borrow_mut().call(name, args))
                } else {
                    Err(
                        "Call the mission API inside on_start/on_tick, not at script top level"
                            .into(),
                    )
                };
                match outcome {
                    Ok(values) => values
                        .into_iter()
                        .map(|v| value(lua, v))
                        .collect::<mlua::Result<MultiValue>>(),
                    // Nothing in the sandbox can catch an error, so this one unwinds the
                    // scene's function straight back to the host.
                    Err(reason) if reason == SUSPEND => {
                        suspended.set(true);
                        Err(mlua::Error::runtime("scene waits"))
                    }
                    Err(reason) => {
                        let reason = format!("{}{reason}", location(lua));
                        *failure.borrow_mut() = Some(reason.clone());
                        Err(mlua::Error::runtime(reason))
                    }
                }
            })?;
            table.raw_set(name, function)?;
        }
        // A fresh environment: mlua always opens the base library into the real globals,
        // which the script must not reach. `scenes` holds the script's scene functions.
        let globals = lua.create_table()?;
        globals.raw_set("mission", table)?;
        globals.raw_set("scenes", lua.create_table()?)?;
        load(&lua, source, globals.clone())?.call::<()>(())?;
        if let Some(callback) = callback {
            match globals.raw_get::<Value>(callback)? {
                Value::Nil => {}
                Value::Function(function) => {
                    active.set(true);
                    function.call::<()>(())?;
                }
                _ => {
                    return Err(mlua::Error::runtime(format!(
                        "{callback} must be a function"
                    )));
                }
            }
        }
        let Some(name) = host.borrow_mut().scene() else {
            return Ok(());
        };
        let function = match globals.raw_get::<Value>("scenes")? {
            Value::Table(scenes) => scenes.raw_get::<Value>(name.as_str())?,
            _ => Value::Nil,
        };
        let Value::Function(function) = function else {
            return Err(mlua::Error::runtime(format!(
                "scenes.{name} must be a function"
            )));
        };
        active.set(true);
        match function.call::<()>(()) {
            Ok(()) => scene = Some(true),
            Err(_) if suspended.get() => scene = Some(false),
            Err(error) => return Err(error),
        }
        Ok(())
    });
    match result {
        Ok(()) => Ok(Ran {
            instructions: hook::instructions(&lua),
            scene,
        }),
        // An API refusal is reported as the API phrased it, not as mlua wrapped it.
        Err(error) => Err(failure.into_inner().unwrap_or_else(|| message(&error))),
    }
}

/// Compiles `source` without running it: syntax errors and size limits, as map loading
/// checks them.
pub fn validate(source: &str) -> Result<(), String> {
    let length = source.encode_utf16().count();
    if length == 0 || length > SCRIPT_LIMIT {
        return Err(format!(
            "Mission script must be 1 to {SCRIPT_LIMIT} characters"
        ));
    }
    let lua = state().map_err(|e| message(&e))?;
    let globals = lua.create_table().map_err(|e| message(&e))?;
    load(&lua, source, globals)
        .map(drop)
        .map_err(|e| message(&e))
}

fn state() -> mlua::Result<Lua> {
    Lua::new_with(StdLib::NONE, LuaOptions::default())
}

/// Text chunks only: precompiled bytecode is refused.
fn load(lua: &Lua, source: &str, globals: Table) -> mlua::Result<mlua::Function> {
    lua.load(source)
        .set_name(CHUNK)
        .set_mode(ChunkMode::Text)
        .set_environment(globals)
        .into_function()
}

fn scalar(value: Value) -> Result<Scalar, String> {
    Ok(match value {
        Value::Nil => Scalar::Nil,
        Value::Boolean(b) => Scalar::Boolean(b),
        Value::Integer(i) => Scalar::Integer(i),
        Value::Number(n) => Scalar::Number(n),
        Value::String(s) => Scalar::String(
            s.to_str()
                .map_err(|_| "Mission API strings must be valid UTF-8".to_string())?
                .to_string(),
        ),
        _ => return Err("Mission API accepts only strings, numbers and booleans".into()),
    })
}

fn value(lua: &Lua, scalar: Scalar) -> mlua::Result<Value> {
    Ok(match scalar {
        Scalar::Nil => Value::Nil,
        Scalar::Boolean(b) => Value::Boolean(b),
        Scalar::Integer(i) => Value::Integer(i),
        Scalar::Number(n) => Value::Number(n),
        Scalar::String(s) => Value::String(lua.create_string(s)?),
    })
}

/// Lua's `luaL_where(L, 1)` from inside an API function: the calling script line.
fn location(lua: &Lua) -> String {
    lua.inspect_stack(1, |debug| {
        let line = debug.current_line()?;
        let source = debug.source().short_src?.into_owned();
        Some(format!("{source}:{line}: "))
    })
    .flatten()
    .unwrap_or_default()
}

/// The script-facing text of an error, without mlua's traceback.
fn message(error: &mlua::Error) -> String {
    let text = match error {
        mlua::Error::RuntimeError(text) | mlua::Error::MemoryError(text) => text.clone(),
        mlua::Error::SyntaxError { message, .. } => message.clone(),
        mlua::Error::CallbackError { cause, .. } => return message(cause),
        mlua::Error::WithContext { cause, .. } => return message(cause),
        other => other.to_string(),
    };
    match text.find("\nstack traceback:") {
        Some(end) => text[..end].to_string(),
        None => text,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run_with(source: &str, callback: &str) -> (Result<Ran, String>, Vec<Vec<Scalar>>) {
        let mut calls = Vec::new();
        let result = run(source, callback, &["echo"], &mut |_, args| {
            calls.push(args.clone());
            Ok(args)
        });
        (result, calls)
    }

    #[test]
    fn no_library_is_reachable() {
        for name in [
            "print",
            "pairs",
            "next",
            "pcall",
            "error",
            "load",
            "require",
            "setmetatable",
            "getmetatable",
            "rawget",
            "tostring",
            "string",
            "table",
            "math",
            "os",
            "io",
            "debug",
            "coroutine",
            "utf8",
            "package",
            "_G",
        ] {
            let (result, calls) = run_with(
                &format!("function on_start() mission.echo({name} == nil) end"),
                "on_start",
            );
            result.unwrap();
            assert_eq!(calls, [vec![Scalar::Boolean(true)]], "{name}");
        }
        let (result, _) = run_with("function on_start() return ('x'):len() end", "on_start");
        assert!(result.unwrap_err().contains("attempt to index"));
    }

    #[test]
    fn scalars_cross_the_boundary_with_their_lua_types() {
        let (result, calls) = run_with(
            "function on_start() mission.echo(nil, true, 3, 2.5, 'é', 4.0) end",
            "on_start",
        );
        result.unwrap();
        assert_eq!(
            calls,
            [vec![
                Scalar::Nil,
                Scalar::Boolean(true),
                Scalar::Integer(3),
                Scalar::Number(2.5),
                Scalar::String("é".into()),
                Scalar::Number(4.0),
            ]]
        );
        let (result, _) = run_with("function on_start() mission.echo({}) end", "on_start");
        assert_eq!(
            result.unwrap_err(),
            "[string \"mission.lua\"]:1: Mission API accepts only strings, numbers and booleans"
        );
        let (result, _) = run_with("function on_start() mission.echo('\\255') end", "on_start");
        assert!(result.unwrap_err().ends_with("must be valid UTF-8"));
    }

    #[test]
    fn returns_reach_the_script() {
        let (result, calls) = run_with(
            "function on_start() local a, b = mission.echo(1, 'two') mission.echo(b .. a) end",
            "on_start",
        );
        result.unwrap();
        assert_eq!(calls[1], [Scalar::String("two1".into())]);
    }

    #[test]
    fn missing_and_invalid_callbacks() {
        assert!(run_with("x = 1", "on_tick").0.is_ok());
        assert_eq!(
            run_with("on_tick = 3", "on_tick").0.unwrap_err(),
            "on_tick must be a function"
        );
    }

    #[test]
    fn errors_name_the_script_line() {
        let (result, _) = run_with("function on_start()\n  missing()\nend", "on_start");
        assert_eq!(
            result.unwrap_err(),
            "[string \"mission.lua\"]:2: attempt to call a nil value (global 'missing')"
        );
        assert_eq!(
            validate("function on_start(").unwrap_err(),
            "[string \"mission.lua\"]:1: <name> or '...' expected near <eof>"
        );
    }

    #[test]
    fn validation_compiles_without_running() {
        validate("while true do end").unwrap();
        assert!(validate("").is_err());
        assert!(validate(&"-".repeat(SCRIPT_LIMIT + 1)).is_err());
        validate(&format!("--{}", "x".repeat(SCRIPT_LIMIT - 2))).unwrap();
        assert!(validate("\x1bLua").unwrap_err().contains("binary chunk"));
    }

    fn loop_of(count: u32) -> String {
        format!("function on_start() for i = 1, {count} do end end")
    }

    #[test]
    fn instructions_are_counted_exactly_and_bounded() {
        let small = run_with(&loop_of(10), "on_start").0.unwrap().instructions;
        let large = run_with(&loop_of(110), "on_start").0.unwrap().instructions;
        // One FORLOOP per iteration; the rest is fixed overhead.
        assert_eq!(large - small, 100);
        let most = 10 + (INSTRUCTION_LIMIT - small);
        assert_eq!(
            run_with(&loop_of(most), "on_start").0.unwrap().instructions,
            INSTRUCTION_LIMIT
        );
        assert_eq!(
            run_with(&loop_of(most + 1), "on_start").0.unwrap_err(),
            "Mission exceeded 20,000 instructions"
        );
        assert_eq!(
            run_with("while true do end", "on_start").0.unwrap_err(),
            "Mission exceeded 20,000 instructions"
        );
        // Raised from a nested function, the error names the calling line, as Lua does.
        assert_eq!(
            run_with(
                "local function spin() while true do end end\nfunction on_start()\n spin()\nend",
                "on_start"
            )
            .0
            .unwrap_err(),
            "[string \"mission.lua\"]:3: Mission exceeded 20,000 instructions"
        );
    }

    #[test]
    fn strings_are_bounded() {
        let doubling = |times: u32| {
            format!("function on_start() local s = 'x' for i = 1, {times} do s = s .. s end end")
        };
        run_with(&doubling(16), "on_start").0.unwrap();
        assert_eq!(
            run_with(&doubling(17), "on_start").0.unwrap_err(),
            "Mission string exceeds 64 KiB"
        );
        assert_eq!(
            run_with(
                "local s = 'x' for i = 1, 17 do s = s .. s end function on_start() end",
                "on_start"
            )
            .0
            .unwrap_err(),
            "Mission string exceeds 64 KiB"
        );
    }

    #[test]
    fn the_api_is_refused_at_top_level() {
        let (result, calls) = run_with("mission.echo(1)\nfunction on_start() end", "on_start");
        assert_eq!(
            result.unwrap_err(),
            "[string \"mission.lua\"]:1: Call the mission API inside on_start/on_tick, not at script top level"
        );
        assert!(calls.is_empty());
    }
}
