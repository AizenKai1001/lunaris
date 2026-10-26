# Prefabs: entity templates

A **prefab** is a named template for an entity and its children. Anything that appears more than once in a game (a tree, a bullet, an enemy, a pickup, a whole room) is a prefab: you describe it once and spawn it as often as you like, from a tool call, from a scene file or from a graph system. An AI agent defines a prefab instead of repeating a long `spawn` call, and gameplay code asks for "a Bullet at this place" in one statement.

```json
{ "components": { "Name": {"value": "Tree"}, "Transform": {"pos": [0, 0, 0]}, "Collider": {"shape": 0, "size": [1.2, 1.2, 1.2]} },
  "children": [
    { "name": "Trunk", "components": { "LocalTransform": {"pos": [0, 1, 0], "scale": [0.3, 2, 0.3]}, "MeshRenderer": {"mesh": "cylinder", "color": [0.45, 0.3, 0.2, 1]} } },
    { "name": "Crown", "components": { "LocalTransform": {"pos": [0, 2.6, 0], "scale": [1.8, 1.8, 1.8]}, "MeshRenderer": {"mesh": "sphere", "color": [0.2, 0.6, 0.25, 1]} } } ] }
```

- **`components`**: the root entity's components, keyed by component name. Partial values use the component's defaults, as in `spawn`.
- **`children`**: nodes spawned attached to the entity above them (a `Parent` component is added for you). A child with a `LocalTransform` and no `Transform` gets a `Transform` of the same value, so it already stands in the right place before the first tick; the hierarchy system keeps it there as the parent moves. Children can have children.
- **`prefab`**: inherit another prefab: its components and children come first, your own `components` replace its fields one by one, your own `children` follow. Works at the top (a "Pine" that is a "Tree" with one more part) and for a child node (`{"prefab": "Branch", "components": {"LocalTransform": {...}}}`).
- **`name`**: shorthand for `Name.value`.
- A prefab nests at most 8 levels deep (this also stops a prefab that contains itself) and one spawn makes at most 4,096 entities.

A prefab is a **template, not a link**: changing it later does not change entities already spawned, and a saved scene contains the plain entities.

## Defining and spawning

| Tool | What it does |
|---|---|
| `define_prefab {name, components?, children?, prefab?}` | Add or replace a prefab. Checked at once: every component must exist, nested prefabs must exist, depth and size must be in bounds (the error says which). Undoable. |
| `remove_prefab {name}` | Delete the definition (entities already spawned stay). Undoable. |
| `prefabs {name?}` | List them with how many entities one spawn makes; with `name`, the definition and the entities it expands to. |
| `spawn_prefab {prefab, at?, overrides?, count?}` | Spawn it: root and children with consecutive uids in one all-or-nothing, undoable step. `at` is `[x, y, z]` for the root's `Transform.pos`; `overrides` replaces fields of the root's components (`{"Name": {"value": "Boss"}}`); `count` makes several copies at the same place. Returns the root uids. |

Prefabs are part of the engine state: they are saved with the scene, recorded and replayed, included in the state hash, and `describe` lists their names.

## In a scene file

```json
{ "lunaris": 1,
  "prefabs": { "Tree": { ... } },
  "instances": [ {"prefab": "Tree", "at": [4, 0, 4]}, {"prefab": "Tree", "at": [-4, 0, 6], "overrides": {"Name": {"value": "Old oak"}}} ],
  "entities": [ ... ] }
```

`instances` are expanded when the scene loads (after `entities`, so their uids come after the scene's own); saving the scene writes the entities themselves and keeps `prefabs`. `examples/orchard.lunaris.json` places forty trees this way.

## From a graph system

```json
{ "name": "gun", "each": ["Gun"], "do": [
    { "if": { "gt": [{"res": "input.space"}, 0.5] }, "then": [
        { "spawn_prefab": "Bullet", "at": [{"get": "Transform.pos[0]"}, 1, {"get": "Transform.pos[2]"}], "velocity": [0, 0, -30] } ] } ] }
```

`spawn_prefab` takes `at` and `velocity` (three expressions each: the root's `Transform.pos` and `Velocity.lin`) and `with` (`{"Health": {"hp": {"add": [{"get": "Gun.level"}, 3]}}}`, expressions for the fields of the root's components: a scalar, or an array of expressions for an array field such as `Decal.normal` or `Transform.rot`: `{"Decal": {"normal": [0, 1, 0]}}`, at most 64 elements, replacing the whole field). Like `spawn` it is queued and applied after the system has visited every entity, in the order of the issuing entities' uids, so the result is the same on any thread count; a replay re-derives the spawns, nothing extra is recorded. A missing prefab is the system's `last_error` (the world keeps running). `examples/orchard.lunaris.json` has a spawner that rains balls onto the trees.

## Why this exists

The big engines have the same idea (Unity prefabs, Unreal Blueprint classes and level instances); see the vault note on the two engines. Lunaris keeps it small and exact: plain JSON, deterministic uids, one undo step, and the same text is what a person edits, a model writes and a recording replays.
