//! The plan's sandbox: its own Lua state, apart from the mission script's, kept for the
//! whole run. The plan's chunk runs as one coroutine that the host resumes once a tick;
//! every step that takes time yields it (`prelude.lua`).
//!
//! What it reaches: the core language, `ipairs`, `select`, `type`, `tostring`, `tonumber`,
//! `error`, `assert`, `string`, `table` and `math` without its random numbers, and the
//! plan's API. No clock, randomness, files, `pairs`/`next` (string-key order depends on a
//! per-state seed) or coroutines of its own, so a plan decides the same on every run.

use mlua::{
    Function, HookTriggers, Lua, LuaOptions, MultiValue, StdLib, Table, Thread, Value, Variadic,
    VmState, chunk::ChunkMode, thread::ThreadStatus,
};
use std::{cell::Cell, rc::Rc};

/// The plan's chunk name: errors read `autoplay.lua:12: …`.
pub const CHUNK: &str = "autoplay.lua";
/// VM instructions a plan may run between two waits.
pub const INSTRUCTION_LIMIT: u32 = 2_000_000;
/// Longest plan, in bytes.
pub const SOURCE_LIMIT: usize = 64_000;

const PRELUDE: &str = include_str!("prelude.lua");
const GLOBALS: [&str; 8] = [
    "assert", "error", "ipairs", "select", "tonumber", "tostring", "type", "rawequal",
];

/// The host side of one resume: `name` and its arguments, its answer.
pub type Host<'a> = dyn FnMut(&Lua, &str, Variadic<Value>) -> mlua::Result<MultiValue> + 'a;

pub struct Plan {
    lua: Lua,
    thread: Thread,
    host: Table,
    used: Rc<Cell<u32>>,
}

impl Plan {
    /// Compiles `source` and readies its coroutine; nothing runs yet.
    pub fn new(source: &str) -> Result<Self, String> {
        if source.len() > SOURCE_LIMIT {
            return Err(format!("{CHUNK}: longer than {SOURCE_LIMIT} bytes"));
        }
        let built = || -> mlua::Result<Self> {
            let lua = Lua::new_with(
                StdLib::COROUTINE | StdLib::MATH | StdLib::STRING | StdLib::TABLE,
                LuaOptions::default(),
            )?;
            let globals = lua.globals();
            let env = lua.create_table()?;
            for name in GLOBALS {
                env.raw_set(name, globals.raw_get::<Value>(name)?)?;
            }
            for library in ["string", "table", "math"] {
                let source: Table = globals.raw_get(library)?;
                let copy = lua.create_table()?;
                for pair in source.pairs::<Value, Value>() {
                    let (key, value) = pair?;
                    copy.raw_set(key, value)?;
                }
                if library == "math" {
                    copy.raw_set("random", Value::Nil)?;
                    copy.raw_set("randomseed", Value::Nil)?;
                }
                env.raw_set(library, copy)?;
            }
            let coroutine: Table = globals.raw_get("coroutine")?;
            let yield_: Function = coroutine.raw_get("yield")?;
            let host = lua.create_table()?;
            lua.load(PRELUDE)
                .set_name("autoplay prelude")
                .set_mode(ChunkMode::Text)
                .set_environment(env.clone())
                .into_function()?
                .call::<()>((host.clone(), yield_))?;
            let plan = lua
                .load(source)
                .set_name(CHUNK)
                .set_mode(ChunkMode::Text)
                .set_environment(env)
                .into_function()?;
            let thread = lua.create_thread(plan)?;
            let used = Rc::new(Cell::new(0u32));
            let counter = used.clone();
            thread.set_hook(
                HookTriggers::new().every_nth_instruction(1000),
                move |_, _| {
                    counter.set(counter.get() + 1000);
                    if counter.get() > INSTRUCTION_LIMIT {
                        return Err(mlua::Error::runtime(
                            "the plan ran two million instructions without waiting",
                        ));
                    }
                    Ok(VmState::Continue)
                },
            )?;
            Ok(Self {
                lua,
                thread,
                host,
                used,
            })
        };
        built().map_err(|e| message(&e))
    }

    /// Runs the plan until its next wait, answering its calls with `host`. True once the
    /// plan has returned.
    pub fn resume(&self, host: &mut Host<'_>) -> Result<bool, String> {
        if self.thread.status() != ThreadStatus::Resumable {
            return Ok(true);
        }
        self.used.set(0);
        let host = std::cell::RefCell::new(host);
        let result = self.lua.scope(|scope| {
            let call = scope.create_function(|lua, mut args: Variadic<Value>| {
                let name = match args.first() {
                    Some(Value::String(s)) => s.to_str()?.to_string(),
                    _ => return Err(mlua::Error::runtime("the plan's host needs a name")),
                };
                args.remove(0);
                let mut host = host
                    .try_borrow_mut()
                    .map_err(|_| mlua::Error::RecursiveMutCallback)?;
                (*host)(lua, &name, args).map_err(|e| located(lua, e))
            })?;
            self.host.raw_set("call", call)?;
            let resumed = self.thread.resume::<()>(());
            self.host.raw_set("call", Value::Nil)?;
            resumed
        });
        match result {
            Ok(()) => Ok(self.thread.status() != ThreadStatus::Resumable),
            Err(error) => Err(message(&error)),
        }
    }
}

/// An API error with the plan's line in front, as the mission script's errors read.
fn located(lua: &Lua, error: mlua::Error) -> mlua::Error {
    let text = message(&error);
    if text.contains(CHUNK) {
        return error;
    }
    let line = (1..12).find_map(|level| {
        lua.inspect_stack(level, |debug| {
            let source = debug.source().short_src?.into_owned();
            let line = debug.current_line()?;
            source.contains(CHUNK).then(|| format!("{CHUNK}:{line}: "))
        })
        .flatten()
    });
    mlua::Error::runtime(format!("{}{text}", line.unwrap_or_default()))
}

/// The plan-facing text of an error, without mlua's traceback.
pub fn message(error: &mlua::Error) -> String {
    let text = match error {
        mlua::Error::RuntimeError(text) | mlua::Error::MemoryError(text) => text.clone(),
        mlua::Error::SyntaxError { message, .. } => message.clone(),
        mlua::Error::CallbackError { cause, .. } => return message(cause),
        mlua::Error::WithContext { cause, .. } => return message(cause),
        other => other.to_string(),
    };
    let text = match text.find("\nstack traceback:") {
        Some(end) => text[..end].to_string(),
        None => text,
    };
    text.replace("[string \"autoplay.lua\"]", CHUNK)
}
