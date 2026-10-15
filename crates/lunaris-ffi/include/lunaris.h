/* Lunaris engine: C interface (lunaris_ffi.dll / liblunaris_ffi.so / liblunaris_ffi.dylib).
 *
 * Everything the engine can do is a "tool" that takes and returns JSON (see docs/AI_GUIDE.md and `tools/list`). This library lets
 * any language that can call C create an engine in its own process and use those tools: no sockets, no child process.
 *
 *   LunarisEngine* e = lunaris_engine_new(1, "C:/projects/mygame");
 *   char* r = lunaris_call(e, "spawn", "{\"components\":{\"Transform\":{\"pos\":[0,1,0]}}}");
 *   puts(r);                       // {"result":{"ok":true,"undoable":true,"uid":1}}
 *   lunaris_string_free(r);
 *   lunaris_engine_free(e);
 *
 * Rules
 *  - Every char* returned by this library must be passed to lunaris_string_free exactly once.
 *  - All text is UTF-8. JSON in, JSON out.
 *  - An engine may be used from several threads; calls are serialised by a lock inside the library.
 *  - Functions never throw or unwind into the caller. A failure is an {"error": ...} reply.
 */
#ifndef LUNARIS_H
#define LUNARIS_H

#include <stdint.h>

#ifdef __cplusplus
extern "C" {
#endif

typedef struct LunarisEngine LunarisEngine;

/* The engine version, for example "0.0.1". Static: do not free. */
const char* lunaris_version(void);

/* Create an engine. `seed` makes the simulation reproducible. `root_dir` is the workspace folder where scenes, textures and models
 * are read and written (NULL = the current folder). Returns NULL if `root_dir` is not valid UTF-8. */
LunarisEngine* lunaris_engine_new(uint64_t seed, const char* root_dir);

/* Destroy an engine (NULL is fine). Do not use the pointer afterwards. */
void lunaris_engine_free(LunarisEngine* engine);

/* Run one tool. `args_json` is a JSON object (NULL or "" means {}). The reply is
 *   {"result": ...}                                   on success, or
 *   {"error": {"code": -32602, "message": "...", "data": {"code": "unknown_component", "path": "...", "hint": "..."}}}
 * `data.code` is the engine's stable error code, `data.path` points into your arguments and `data.hint` says how to fix it.
 * Returns NULL only when `engine` is NULL. Free the reply with lunaris_string_free. */
char* lunaris_call(LunarisEngine* engine, const char* tool, const char* args_json);

/* Handle one JSON-RPC 2.0 message: the same protocol as `lunaris serve`, including the MCP methods (initialize, tools/list,
 * tools/call). Returns the reply, or NULL when no reply is due (a notification) or `engine` is NULL. */
char* lunaris_rpc(LunarisEngine* engine, const char* request_json);

/* Give back a string returned by lunaris_call or lunaris_rpc (NULL is fine). */
void lunaris_string_free(char* s);

#ifdef __cplusplus
}
#endif

#endif /* LUNARIS_H */
