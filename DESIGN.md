# Brandon: design

Brandon is a 2D game engine and editor written in Rust. The goal is a more polished alternative to Godot: easy to learn and pleasant to use, but able to make complex, beautiful 2D games.

This document records the core decisions. It is meant to be short and to change as we learn. Anything under **Open questions** is not decided yet.

**Where we are:** steps 1 to 3 of the [roadmap](#roadmap) are done, apart from hot reload. The runtime core runs a demo game, and the editor can open, change, save and play its scenes. Things described here but not built yet are marked *(planned)*.

## What Brandon does better than Godot

1. **One simple scene model.** Godot has hundreds of node types in deep inheritance chains (`Node → CanvasItem → Node2D → CollisionObject2D → PhysicsBody2D → CharacterBody2D`), and "nodes" and "scenes" are two overlapping ideas with instancing, inheritance and editable children layered on top. Brandon keeps the tree but has one building block and a short, flat list of features.
2. **One language: Rust.** No second scripting language, no glue layer, no stringly-typed lookups. Game code is a normal Rust crate with full IDE support.
3. **An editor that shows only what is needed.** Godot's editor shows every panel and every property at once. Brandon's editor starts with three areas and reveals more only when you ask for it.
4. **Beautiful by default.** 2D lighting, HDR, bloom and good text rendering are built in and switched on with sensible defaults, not assembled from plugins.

## Scene model

The user sees a tree. Under the hood, storage can be dense and cache-friendly, but that is an implementation detail.

- **Node** is the only building block. Every node has a name, a transform (position, rotation, scale, z-order), a visibility flag and children. Nothing else.
- **Parts** give a node its abilities. A node holds at most one of each part from a small, flat list. Built so far: `Sprite`, `Text`, `Camera`, `Shape` (overlap detection) and `Overlay` (draws the node and its children fixed to the window, for scores and menus). *(Planned)*: `Body` (physics), `Light`, `Particles`, `TileMap`, `Sound` and UI parts. There is no inheritance. A physics character is one node with `Body` and `Shape`, not three nested nodes.
- **Behaviours** are the user's own Rust structs, attached to nodes the same way as parts. A node can have several. A behaviour marked `#[behaviour]` can be saved in scene files, and its fields are what the inspector will show.
- **Scenes** are files holding a node tree. Placing one scene inside another creates a linked instance. You can override the instance's properties but not restructure its children. There are no inherited scenes and no "editable children".
- **Typed references instead of strings** *(planned)*. A behaviour field like `target: NodeRef<Player>` is set by drag and drop in the editor and checked when the scene loads. Events are typed Rust values, not named signals. Looking a node up by name works today, but it is not meant to be the main way.
- **While a behaviour runs, it holds its own node.** `update` gets `node: &mut Node` for the node it is attached to and reaches everything else through `ctx.scene`. This is what lets a behaviour change its node and the rest of the scene in the same line without fighting the borrow checker.

What a game looks like:

```rust
use brandon::prelude::*;

#[behaviour]
#[derive(Default)]
struct Player {
    speed: f32,
}

impl Behaviour for Player {
    fn update(&mut self, node: &mut Node, ctx: &mut Ctx) {
        let dir = ctx.input.axis2d(Key::A, Key::D, Key::W, Key::S);
        node.transform.position += dir * self.speed * ctx.dt();
    }
}

fn main() {
    Game::new()
        .register::<Player>()
        .start_scene("scenes/main.scene")
        .run();
}
```

Coordinates: `x` grows to the right and `y` grows downwards, like pixels on a screen and like Godot. One unit is one pixel at normal zoom. Without a camera, `(0, 0)` is the top-left corner of the window.

Scene files are RON, written to be read and edited by hand: defaults are left out, colours are hex strings, and mistakes are reported with a line number and a hint (for example, an unknown behaviour name lists the registered ones).

## Language and hot reload

- Rust is the only language for game code. There is no visual scripting and no embedded scripting language.
- A game is a normal Cargo crate that depends on `brandon`.
- For now, the editor's Play button saves the scene and runs the game with `cargo run` in its own window.
- *(Planned)* In the editor, the game crate is built as a dynamic library and hot-reloaded: the editor saves the scene state, swaps the library and restores the state, so you keep your place while changing code. This needs the editor and game to be built with the same Rust compiler, which the editor will check.
- Shipped games link everything statically. There is no dynamic loading at runtime.
- Code is edited in the user's own IDE (RustRover, VS Code, etc.). The editor opens files there and does not ship its own code editor.

## Graphics

- **Vulkan underneath**, through `ash` with `gpu-allocator` for memory. Brandon owns its renderer, so there is no general-purpose graphics abstraction in between.
- Target Vulkan 1.2 plus the dynamic rendering extension, which keeps the code simpler and still runs on MoltenVK.
- Built so far: sprites batched by texture, premultiplied alpha (no dark fringes on soft edges), an sRGB-correct pipeline, smooth or pixel-art scaling per sprite, and text from a glyph atlas that is rasterised at the size it appears on screen.
- *(Planned)*: an HDR render target, 2D lights with normal maps and soft shadows, post-processing (bloom, colour grading) and particles.
- Shaders are written in WGSL and compiled to SPIR-V with `naga` (pure Rust, so no C++ toolchain is needed). *(Planned)*: users can write their own.

## Platforms

- Windows and Linux natively, macOS through MoltenVK (bundled with the editor and with exported games).
- **No web build.** Browsers do not support Vulkan, so this follows from the Vulkan decision. Android is possible later because it supports Vulkan.

## Editor

Principles:

- **Three areas by default:** scene tree on the left, viewport in the centre, inspector on the right. Everything else (assets, console, animation, profiler) is a tool window that opens on demand and folds away to a thin edge strip when not in use *(tool windows planned)*.
- **The inspector only shows what the node has.** Parts and behaviours appear as collapsible sections. New parts are added with one "Add" button and a search box, not by browsing a list of every possible property.
- **Search everything from one shortcut:** nodes, scenes and commands today; assets and settings *(planned)*.
- **Keyboard-first**, with sensible defaults and nothing to configure before the first run.
- Calm visual design: few colours, clear typography, no clutter. The JetBrains IDEs are a reference for how to stay powerful while showing little, but Brandon has its own look.

Technology: the editor is a Rust app built on the same engine crates, so the viewport is the real renderer. Its UI is `egui` with Brandon's own theme, drawn by Brandon's renderer (see open question 2).

The editor does not load the game's code. Behaviours it has no code for are kept as their scene-file data, so it can open, change and save any game's scenes. Their settings are edited as that data: numbers, true/false and text get proper fields, anything else is edited as text. Once hot reload exists, the editor can show the real Rust types instead.

## Crates we lean on

| Need | Crate |
| --- | --- |
| Windows and input | `winit` |
| Vulkan | `ash`, `gpu-allocator` |
| Maths | `glam` |
| Physics *(planned)* | `rapier2d` |
| Audio *(planned)* | `kira` |
| Text shaping | `cosmic-text` |
| Scene files | `serde` + `ron` (readable and friendly to version control) |
| Shaders | `naga` |
| Images | `image` |

## Workspace layout

```
crates/
  brandon/         the crate users depend on: Game, the game loop, the prelude
  brandon_core/    maths, colour, time, input, random numbers, asset lookup
  brandon_scene/   nodes, parts, behaviours, scene files
  brandon_render/  Vulkan renderer and text
  brandon_macros/  #[behaviour] and #[data]
  brandon_editor/  the editor app
examples/
  hello/           the smallest game: a scene built in code
  starfall/        a small complete game built from scene files
```

## Not in the first versions

3D, web, visual scripting, other scripting languages, a built-in code editor, networking.

## Roadmap

1. **This document.** Done.
2. **Runtime core and a demo game.** Window, Vulkan sprite renderer, text, camera, input, asset loading and the scene model, shown working in a small playable demo. Done: see `examples/starfall`.
3. **Editor foundation.** Scene tree, inspector, viewport with move, rotate and scale handles, saving and loading scenes, and hot reload of game code. Done, apart from hot reload: for now Play builds and runs the game in its own window. Hot reload is the next thing to add.
4. **Visual polish and learning material.** Lights and shadows, custom shaders, particles, post-processing, a getting-started tutorial and examples.

## Open questions

1. **License.** The repository is GPLv3, which means every game built with Brandon must also be released under the GPL. Most Rust engines (Bevy, macroquad, Fyrox) use MIT or Apache-2.0 so that games can be sold closed-source. Is GPLv3 intended?
2. **Editor UI toolkit.** Option A: `egui` with a custom theme, drawn by Brandon's renderer. It is fast to build with, but it is hard to make look truly polished. Option B: Brandon's own retained UI, built on its UI parts, so the editor uses the engine itself (as Godot does). It looks better and tests the engine, but takes much longer. A possible path is to start with A and move to B. The first editor uses A, and the egui code is kept apart from the document model so it can be replaced.
3. **Names.** "Parts" and "Behaviours" are placeholders.
4. **Minimum hardware.** Vulkan 1.2 rules out some GPUs older than about 2016. Is that acceptable?
