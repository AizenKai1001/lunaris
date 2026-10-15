/* Lunaris native plugin interface, version 1.
 *
 * A plugin is a shared library (.dll / .so / .dylib) that exports two functions. A graph system names it:
 *
 *   {"name": "seek", "plugin": "plugins/swarm.dll", "each": ["Transform", "Velocity"],
 *    "fields": ["Transform.pos[0]", "Transform.pos[2]", "Velocity.lin[0]", "Velocity.lin[2]"],
 *    "writes": ["Velocity.lin[0]", "Velocity.lin[2]"],
 *    "res": ["target.x", "target.z"], "parallel": true}
 *
 * Every tick the engine collects the listed fields of up to 256 entities at a time into arrays of doubles and calls
 * `lunaris_update` once per batch. Whatever the plugin leaves in the arrays named by "writes" is written back (converted to the
 * field's type; non-finite numbers are refused and the system records an error). Setting despawn[i] = 1 deletes entity i.
 *
 * Rules that keep the simulation reproducible: the plugin must be a pure function of its inputs (the arrays, dt, tick, seed, the
 * resources and rand()): no clock, no threads, no global or static state, no random numbers of its own. Then every run, on every
 * thread count, gives the same result bit for bit. With "parallel": true the engine may call the function from several threads at
 * once (on different batches), so it must also be reentrant.
 *
 * Loading native code is as powerful as running a program: it is off unless the host passes --allow-plugins (or sets
 * LUNARIS_ALLOW_PLUGINS=1, or calls Engine::allow_plugins), and the library must be inside the workspace folder.
 */
#ifndef LUNARIS_PLUGIN_H
#define LUNARIS_PLUGIN_H

#include <stdint.h>

#ifdef __cplusplus
extern "C" {
#endif

#define LUNARIS_PLUGIN_ABI 1

typedef struct LunarisBatch LunarisBatch;

struct LunarisBatch {
    uint32_t abi;            /* LUNARIS_PLUGIN_ABI */
    uint32_t struct_size;    /* sizeof(LunarisBatch) as the engine knows it */
    uint32_t count;          /* entities in this batch (at most 256) */
    uint32_t nfields;        /* number of arrays in `fields` */
    uint32_t nres;           /* number of values in `res` */
    uint32_t reserved;
    uint64_t tick;           /* the tick being simulated */
    double   dt;             /* seconds per tick */
    uint64_t seed;           /* the simulation seed */
    const uint64_t* uids;    /* count entity ids */
    double** fields;         /* nfields arrays of `count` doubles, in the order of "fields"; write only the ones named in "writes" */
    const double* res;       /* nres resource values, in the order of "res" (read once per tick) */
    uint8_t* despawn;        /* count bytes, zero on entry; set to 1 to despawn that entity */
    void* user;              /* reserved */
    /* The engine's deterministic per-entity random number in [0, 1) for (seed, tick, entity, salt). */
    double (*rand)(const LunarisBatch* batch, uint32_t row, uint64_t salt);
};

/* Exported by every plugin: must return LUNARIS_PLUGIN_ABI. */
uint32_t lunaris_plugin_abi(void);

/* Exported by every plugin (the name can differ if the system says {"export": "name"}). Return 0 for success; any other value
 * is recorded as the system's error and the batch's writes are dropped. */
int32_t lunaris_update(LunarisBatch* batch);

#ifdef __cplusplus
}
#endif

#endif /* LUNARIS_PLUGIN_H */
