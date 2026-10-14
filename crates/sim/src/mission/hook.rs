//! The instruction hook: the sandbox's only unsafe code.
//!
//! It must run on every VM instruction and read the running function's register window,
//! which mlua's safe hook API cannot do (and its per-instruction callback is several times
//! slower). Raising the error from a raw hook is how Lua itself reports `debug.sethook`
//! errors; the hook frame holds no values with destructors, so unwinding past it with
//! Lua's `longjmp` is sound.

#![allow(unsafe_code)]

use super::sandbox::{INSTRUCTION_LIMIT, STRING_LIMIT};
use mlua::{Lua, ffi};

/// Starts counting instructions on `lua`'s main thread from zero.
pub(super) fn install(lua: &Lua) -> mlua::Result<()> {
    // SAFETY: `exec_raw` hands us `lua`'s main thread; the extra space is a pointer-sized,
    // pointer-aligned block Lua reserves for the host (unused by mlua), so it holds a
    // `usize` counter.
    unsafe {
        lua.exec_raw::<()>((), |state| {
            ffi::lua_getextraspace(state).cast::<usize>().write(0);
            ffi::lua_sethook(state, Some(count), ffi::LUA_MASKCOUNT, 1);
        })
    }
}

/// Instructions executed since [`install`].
pub(super) fn instructions(lua: &Lua) -> u32 {
    let mut count = 0;
    // SAFETY: reads the counter written by `install`.
    let _ = unsafe {
        lua.exec_raw::<()>((), |state| {
            count = ffi::lua_getextraspace(state).cast::<usize>().read();
        })
    };
    u32::try_from(count).unwrap_or(u32::MAX)
}

/// Called before every instruction. Counts it, then checks the running function's
/// registers for an oversized string: repeated string doubling can exhaust memory in fewer
/// instructions than any coarser check would allow.
unsafe extern "C-unwind" fn count(state: *mut ffi::lua_State, _: *mut ffi::lua_Debug) {
    // SAFETY: Lua calls the hook with the running thread, which is the main thread (the
    // sandbox has no coroutines); its extra space holds the counter set up by `install`.
    unsafe {
        let counter = ffi::lua_getextraspace(state).cast::<usize>();
        let executed = counter.read() + 1;
        counter.write(executed);
        if executed > INSTRUCTION_LIMIT as usize {
            ffi::luaL_error(state, c"Mission exceeded 20,000 instructions".as_ptr());
            return;
        }
        for index in 1..=ffi::lua_gettop(state) {
            if ffi::lua_type(state, index) == ffi::LUA_TSTRING
                && ffi::lua_rawlen(state, index) > STRING_LIMIT
            {
                ffi::luaL_error(state, c"Mission string exceeds 64 KiB".as_ptr());
                return;
            }
        }
    }
}
