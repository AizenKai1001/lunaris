//! The engine as a C library. Anything that can call C (C, C++, C#, Go, Zig, Java, Python's ctypes, Lua's FFI, ...) can create an
//! engine and drive it with the same JSON the stdio server speaks, inside its own process: no pipes, no sockets.
//!
//! ```text
//! LunarisEngine* e = lunaris_engine_new(1, "C:/projects/mygame");
//! char* reply = lunaris_call(e, "spawn", "{\"components\":{\"Transform\":{}}}");   // {"result": {...}} or {"error": {...}}
//! lunaris_string_free(reply);
//! lunaris_engine_free(e);
//! ```
//!
//! Rules: every `char*` the library returns must be given back to `lunaris_string_free`; an engine may be used from any thread, calls
//! are serialised by a lock; all text is UTF-8; functions never unwind into the caller (a panic becomes an error reply).
use lunaris_api::{safe_handle, Engine};
use lunaris_json::{parse, Json};
use std::ffi::{c_char, CStr, CString};
use std::panic::{catch_unwind, AssertUnwindSafe};
use std::ptr;
use std::sync::Mutex;

/// An engine behind a lock. The C side only ever sees a pointer to it.
pub struct LunarisEngine(Mutex<Engine>);

fn to_c(text: String) -> *mut c_char {
    // an interior NUL cannot be represented; JSON text never contains one (it is escaped), so this only guards the impossible
    CString::new(text.replace('\0', "")).map(CString::into_raw).unwrap_or(ptr::null_mut())
}

/// # Safety
/// `p` must be null or point to a NUL-terminated string that stays valid for `'a`.
unsafe fn text_arg<'a>(p: *const c_char) -> Option<&'a str> {
    if p.is_null() {
        None
    } else {
        // SAFETY: caller contract.
        unsafe { CStr::from_ptr(p) }.to_str().ok()
    }
}

fn error_reply(code: i64, message: &str) -> String {
    Json::obj([("error", Json::obj([("code", Json::from(code)), ("message", Json::str(message))]))]).to_string_lossy()
}

/// The version of the engine, such as `0.0.1`. The pointer is static: do not free it.
#[no_mangle]
pub extern "C" fn lunaris_version() -> *const c_char {
    static TEXT: &str = concat!(env!("CARGO_PKG_VERSION"), "\0");
    TEXT.as_ptr().cast()
}

/// Create an engine with a seed of your choice and a workspace folder (where scenes, textures and models are read and written;
/// null means the current folder). Returns null if the folder text is not valid UTF-8.
///
/// # Safety
/// `root_dir` must be null or a valid NUL-terminated string.
#[no_mangle]
pub unsafe extern "C" fn lunaris_engine_new(seed: u64, root_dir: *const c_char) -> *mut LunarisEngine {
    let root = if root_dir.is_null() {
        std::path::PathBuf::from(".")
    } else {
        // SAFETY: caller contract.
        match unsafe { text_arg(root_dir) } {
            Some(s) => std::path::PathBuf::from(s),
            None => return ptr::null_mut(),
        }
    };
    catch_unwind(|| Box::into_raw(Box::new(LunarisEngine(Mutex::new(Engine::new(seed, root)))))).unwrap_or(ptr::null_mut())
}

/// Destroy an engine. Passing null does nothing. Do not use the pointer afterwards.
///
/// # Safety
/// `engine` must be null or a pointer returned by `lunaris_engine_new` that has not been freed, with no call in flight.
#[no_mangle]
pub unsafe extern "C" fn lunaris_engine_free(engine: *mut LunarisEngine) {
    if !engine.is_null() {
        // SAFETY: caller contract: it came from Box::into_raw and is freed once.
        drop(unsafe { Box::from_raw(engine) });
    }
}

/// Handle one JSON-RPC 2.0 message (the same protocol as `lunaris serve`, including MCP methods such as `tools/list` and
/// `tools/call`) and return the reply text, or null for a notification (no reply is due) or when the engine pointer is null.
///
/// # Safety
/// `engine` must be a live engine pointer and `request_json` a valid NUL-terminated string.
#[no_mangle]
pub unsafe extern "C" fn lunaris_rpc(engine: *mut LunarisEngine, request_json: *const c_char) -> *mut c_char {
    if engine.is_null() {
        return ptr::null_mut();
    }
    // SAFETY: caller contract.
    let Some(text) = (unsafe { text_arg(request_json) }) else { return to_c(error_reply(-32600, "the request is null or not valid UTF-8")) };
    // SAFETY: caller contract: the pointer is live.
    let handle = unsafe { &*engine };
    catch_unwind(AssertUnwindSafe(|| {
        let reply = match parse(text) {
            Err(e) => Some(Json::obj([("jsonrpc", Json::str("2.0")), ("id", Json::Null), ("error", Json::obj([("code", Json::from(-32700)), ("message", Json::Str(format!("parse error: {e}")))]))])),
            Ok(msg) => {
                let mut engine = handle.0.lock().unwrap_or_else(|p| p.into_inner());
                safe_handle(&mut engine, &msg)
            }
        };
        match reply {
            Some(r) => to_c(r.to_string_compact().unwrap_or_else(|_| r.to_string_lossy())),
            None => ptr::null_mut(),
        }
    }))
    .unwrap_or_else(|_| to_c(error_reply(-32603, "internal error")))
}

/// Run one tool. `args_json` is a JSON object (null or empty means `{}`). The reply is `{"result": ...}` or
/// `{"error": {"code": .., "message": .., "data": {"code": "unknown_component", "path": .., "hint": ..}}}`: the `data` fields are
/// the engine's own stable error code, the path into your arguments and a hint for fixing it.
///
/// # Safety
/// `engine` must be a live engine pointer; `tool` a valid NUL-terminated string; `args_json` null or a valid string.
#[no_mangle]
pub unsafe extern "C" fn lunaris_call(engine: *mut LunarisEngine, tool: *const c_char, args_json: *const c_char) -> *mut c_char {
    // SAFETY: caller contract.
    let (Some(tool), args) = (unsafe { text_arg(tool) }, unsafe { text_arg(args_json) }) else { return to_c(error_reply(-32600, "the tool name is null or not valid UTF-8")) };
    if engine.is_null() {
        return ptr::null_mut();
    }
    let params = match args {
        None => Json::Obj(Vec::new()),
        Some(a) if a.trim().is_empty() => Json::Obj(Vec::new()),
        Some(a) => match parse(a) {
            Ok(v) => v,
            Err(e) => return to_c(error_reply(-32602, &format!("the arguments are not valid JSON: {e}"))),
        },
    };
    let request = Json::obj([("jsonrpc", Json::str("2.0")), ("id", Json::from(1)), ("method", Json::Str(format!("lunaris.{tool}"))), ("params", params)]);
    // SAFETY: caller contract.
    let handle = unsafe { &*engine };
    catch_unwind(AssertUnwindSafe(|| {
        let reply = {
            let mut engine = handle.0.lock().unwrap_or_else(|p| p.into_inner());
            safe_handle(&mut engine, &request)
        };
        let out = match reply {
            Some(r) => match (r.get("result"), r.get("error")) {
                (Some(result), _) => Json::obj([("result", result.clone())]),
                (None, Some(err)) => Json::obj([("error", err.clone())]),
                _ => Json::obj([("error", Json::obj([("code", Json::from(-32603)), ("message", Json::str("the engine gave no result"))]))]),
            },
            None => Json::obj([("error", Json::obj([("code", Json::from(-32603)), ("message", Json::str("the engine gave no reply"))]))]),
        };
        to_c(out.to_string_compact().unwrap_or_else(|_| out.to_string_lossy()))
    }))
    .unwrap_or_else(|_| to_c(error_reply(-32603, "internal error")))
}

/// Give back a string that `lunaris_rpc` or `lunaris_call` returned. Null does nothing.
///
/// # Safety
/// `s` must be null or a pointer returned by this library that has not been freed.
#[no_mangle]
pub unsafe extern "C" fn lunaris_string_free(s: *mut c_char) {
    if !s.is_null() {
        // SAFETY: caller contract: it came from CString::into_raw.
        drop(unsafe { CString::from_raw(s) });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn call(e: *mut LunarisEngine, tool: &str, args: &str) -> Json {
        let (t, a) = (CString::new(tool).unwrap(), CString::new(args).unwrap());
        // SAFETY: valid strings and a live engine.
        let p = unsafe { lunaris_call(e, t.as_ptr(), a.as_ptr()) };
        assert!(!p.is_null());
        // SAFETY: p is a string this library just returned.
        let text = unsafe { CStr::from_ptr(p) }.to_str().unwrap().to_string();
        // SAFETY: given back exactly once.
        unsafe { lunaris_string_free(p) };
        parse(&text).unwrap()
    }

    fn new_engine() -> *mut LunarisEngine {
        let root = CString::new(std::env::temp_dir().to_str().unwrap()).unwrap();
        // SAFETY: valid string.
        let e = unsafe { lunaris_engine_new(7, root.as_ptr()) };
        assert!(!e.is_null());
        e
    }

    #[test]
    fn an_engine_can_be_driven_through_the_c_functions() {
        let e = new_engine();
        let r = call(e, "spawn", r#"{"components":{"Name":{"value":"ffi"},"Transform":{"pos":[1,2,3]}}}"#);
        assert_eq!(r.get("result").and_then(|r| r.get("ok")), Some(&Json::Bool(true)), "{}", r.to_string_lossy());
        let q = call(e, "query", r#"{"select":["Transform"]}"#);
        assert_eq!(q.get("result").and_then(|r| r.get("total")).and_then(|t| t.as_u64()), Some(1));
        // null or empty arguments mean {}
        assert!(call(e, "describe", "").get("result").is_some());
        let step = call(e, "step", r#"{"ticks":10}"#);
        assert_eq!(step.get("result").and_then(|r| r.get("tick")).and_then(|t| t.as_u64()), Some(10));
        // SAFETY: live engine, freed once.
        unsafe { lunaris_engine_free(e) };
    }

    #[test]
    fn errors_carry_the_engines_code_path_and_hint() {
        let e = new_engine();
        let r = call(e, "spawn", r#"{"components":{"Transfrom":{}}}"#);
        let data = r.get("error").and_then(|e| e.get("data")).expect("error data");
        assert_eq!(data.get("code").and_then(|c| c.as_str()), Some("unknown_component"));
        assert!(data.get("hint").and_then(|h| h.as_str()).unwrap().contains("Transform"));
        let bad = call(e, "spawn", "{not json");
        assert_eq!(bad.get("error").and_then(|e| e.get("code")).and_then(|c| c.as_i64()), Some(-32602));
        let unknown = call(e, "no_such_tool", "{}");
        assert!(unknown.get("error").is_some());
        // SAFETY: live engine, freed once.
        unsafe { lunaris_engine_free(e) };
    }

    #[test]
    fn json_rpc_messages_work_and_notifications_get_no_reply() {
        let e = new_engine();
        let req = CString::new(r#"{"jsonrpc":"2.0","id":9,"method":"tools/list"}"#).unwrap();
        // SAFETY: live engine, valid string.
        let p = unsafe { lunaris_rpc(e, req.as_ptr()) };
        assert!(!p.is_null());
        // SAFETY: a string this library returned.
        let text = unsafe { CStr::from_ptr(p) }.to_str().unwrap().to_string();
        // SAFETY: freed once.
        unsafe { lunaris_string_free(p) };
        assert!(text.contains("\"id\":9") && text.contains("spawn"), "{text}");
        let note = CString::new(r#"{"jsonrpc":"2.0","method":"notifications/initialized"}"#).unwrap();
        // SAFETY: live engine, valid string.
        assert!(unsafe { lunaris_rpc(e, note.as_ptr()) }.is_null(), "a notification has no reply");
        let junk = CString::new("not json at all").unwrap();
        // SAFETY: live engine, valid string.
        let p = unsafe { lunaris_rpc(e, junk.as_ptr()) };
        // SAFETY: a string this library returned.
        let text = unsafe { CStr::from_ptr(p) }.to_str().unwrap().to_string();
        // SAFETY: freed once.
        unsafe { lunaris_string_free(p) };
        assert!(text.contains("-32700"), "{text}");
        // null engine and null pointers are refused, not crashes
        // SAFETY: null is allowed by the contract.
        unsafe {
            assert!(lunaris_rpc(ptr::null_mut(), req.as_ptr()).is_null());
            lunaris_engine_free(ptr::null_mut());
            lunaris_string_free(ptr::null_mut());
        }
        // SAFETY: live engine, freed once.
        unsafe { lunaris_engine_free(e) };
    }

    #[test]
    fn many_threads_can_share_one_engine_and_the_version_is_reported() {
        let e = new_engine();
        let addr = e as usize;
        let handles: Vec<_> = (0..4)
            .map(|_| {
                std::thread::spawn(move || {
                    for _ in 0..25 {
                        call(addr as *mut LunarisEngine, "spawn", r#"{"components":{"Transform":{}}}"#);
                    }
                })
            })
            .collect();
        handles.into_iter().for_each(|h| h.join().unwrap());
        assert_eq!(call(e, "query", "{}").get("result").and_then(|r| r.get("total")).and_then(|t| t.as_u64()), Some(100));
        // SAFETY: the static version string is valid.
        let v = unsafe { CStr::from_ptr(lunaris_version()) }.to_str().unwrap();
        assert_eq!(v, lunaris_api::VERSION);
        // SAFETY: live engine, freed once.
        unsafe { lunaris_engine_free(e) };
    }
}
