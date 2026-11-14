# solidcraft-plugin-gear

![A 20-tooth involute spur gear created by the plugin](docs/gear-iso.png)

A [SolidCraft](https://github.com/tangency-labs/solidcraft) plugin that creates a
parametric involute spur gear at the origin of a design.

The plugin is a WebAssembly component against the plugin contract the merged
SolidCraft host serves: world `craft:plugin@0.1.0`, capability
`commands@0.1` (phase 1 of
[RFC 0002](https://github.com/tangency-labs/solidcraft/blob/main/docs/rfcs/0002-plugins.md),
merged in [#15112](https://github.com/tangency-labs/solidcraft/pull/15112)). It
changes the model only through the engine commands the app already serves over
MCP (`sketch.line`, `sketch.circle.center`, `solid.extrude`), so every edit is
undoable and journaled, and a design the plugin made **opens without the plugin
installed**.

## Status

SolidCraft 0.41.0 ships the phase 1 wasm host (PR #15112), and this plugin
(version 0.4.0) runs in it: install the release zip, and
**Plugins › Создать шестерню…** opens the parameter dialog. The plugin's static
manifest is parsed by `craft-plugin`, and the tests run the component through
the real `craft-plugin-wasm` sandbox (fresh
instance, fuel, transaction commit and rollback). The geometry and the emitted
commands are verified against the current engine through its MCP surface.

The plugin's UI is in Russian: the menu entry, the field labels, units and
descriptions all come from the manifest's schema, which the host draws.

## Use it from the CLI

The installed SolidCraft ships `solidcraft-cli.exe`, which runs the same engine
the app does. You can drive the plugin's wasm component yourself and replay its
commands into a real design file:

```sh
rustup target add wasm32-unknown-unknown
cargo build --release --target wasm32-unknown-unknown

# Run the component on the real plugin host, dump the engine commands for any parameters:
cargo run --example dump_commands -- gear.json teeth=31 module=3 thickness=12 bore_diameter=10 name=Pinion

# Replay them with the app's own engine and save an openable design:
"C:\Program Files\SolidCraft\solidcraft-cli.exe" run gear.json --save gear.sc
```

Open `gear.sc` in SolidCraft: the gear is there, as native sketch and extrude
features in the timeline, fully undoable. `solidcraft-cli snapshot --in gear.sc
--out shot.png --view iso` renders it without opening the app.

## Install

1. Download `gear-generator-<version>.zip` from the project's
   [releases](../../releases) — from the **Assets** section, not the
   "Source code (zip)" link, which holds the repository, not the plugin.
2. In SolidCraft open **Preferences › Plugins** and use **Install plugin…**,
   or unpack the zip into the plugins folder:

   | System | Plugins folder |
   |---|---|
   | Windows | `%LOCALAPPDATA%\SolidCraft\plugins\gear-generator\` |
   | macOS | `~/Library/Application Support/SolidCraft/plugins/gear-generator/` |
   | Linux | `~/.local/share/solidcraft/plugins/gear-generator/` |

   The folder must hold `plugin.json` and `gear-generator.wasm`.
3. Restart SolidCraft if you unpacked by hand. The plugin appears under
   Preferences › Plugins, and its entry in the Plugins menu.

Installing a newer version over an older one asks for confirmation, because the
plugin id is the same.

The one line **Install plugin… → `pro-d-e-v/solidcraft-plugin-gear`** will
install the latest release once phase 4 of
[RFC 0002](https://github.com/tangency-labs/solidcraft/blob/main/docs/rfcs/0002-plugins.md)
lands (install from a GitHub release, proposed in
[#1221](https://github.com/tangency-labs/solidcraft/issues/1221#issuecomment-6862497847));
it is not in SolidCraft 0.41.0, so the steps above are the way in today.

## Parameters

| Parameter | Range | Default | Meaning |
|---|---|---|---|
| `teeth` | 6 – 200 | 20 | Number of teeth |
| `module` | 0.5 – 20 mm | 2 mm | Pitch diameter = module × teeth |
| `pressure_angle` | 10° – 30° | 20° | At the pitch circle (14.5°, 20° and 25° are the common values) |
| `thickness` | 1 – 300 mm | 8 mm | Face width (extrude distance) |
| `bore_diameter` | 0 – 400 mm | 8 mm | Centre bore, 0 for none; must leave a rim |
| `keyway` | on/off | off | Cut a keyway slot in the bore (needs a bore) |
| `keyway_width` | 0.5 mm – bore diameter | 4 mm | Slot width |
| `keyway_depth` | 0.5 mm – bore radius | 2 mm | How far the slot reaches past the bore edge; its corners must stay under the root circle |
| `keyway_angle` | −360° – 360° | 0° | Where the slot points (0° is +X) |
| `name` | 1 – 64 chars | `Gear` | Name of the sketch, the extrude and the body |

The gear is centred on the origin on the XY plane and extruded towards +Z.
Above the base circle each flank is a piecewise involute approximation; below
the base circle (gears under about 17 teeth) it is a straight radial line, the
standard simplified profile. The number of pieces falls as the tooth count
rises (10 per flank up to 90 teeth, 7 up to 150, 5 above), keeping the whole
outline near 3000 points so that a 200-tooth gear still builds inside the
plugin deadline. Root fillets and undercut trimming are not modelled.

The keyway is a rectangle from the bore's centre out to `bore radius + depth`,
`width` wide, rotated by `keyway_angle`, cut through the whole face width. The
cut names the gear body as its target, so it never touches other bodies in the
design.

## What the plugin emits

One `sketch.create`, one closed `sketch.line` holding the entire gear outline
as a single polyline, the bore as `sketch.circle.center`, `sketch.finish` and
one `solid.extrude` — five engine calls for any tooth count, so 200 teeth cost
no more host calls than 6. The extrude does not name profiles, so the bore is
the outline's hole. With the keyway on, four more follow: a second sketch, the
closed slot rectangle, `sketch.finish` and a cut-through-all extrude. The
manifest asks for 500,000,000 fuel, 64 MiB and a 30-second deadline, all within
the host's ceilings. The summary the plugin returns carries the tooth count,
module, pressure angle, pitch/base/tip/root diameters, thickness, bore, keyway,
name, a volume estimate, the bounding box and the number of commands run.

## Build from source

```sh
rustup target add wasm32-unknown-unknown
cargo build --release --target wasm32-unknown-unknown
# target/wasm32-unknown-unknown/release/solidcraft_plugin_gear.wasm is a core
# module carrying component metadata; lift it into the component:
wasm-tools component new target/wasm32-unknown-unknown/release/solidcraft_plugin_gear.wasm \
  -o gear-generator/gear-generator.wasm
```

Then stage the folder:

```sh
mkdir -p gear-generator
cp plugin.json README.md gear-generator/
wasm-tools component new target/wasm32-unknown-unknown/release/solidcraft_plugin_gear.wasm \
  -o gear-generator/gear-generator.wasm
```

Run the tests on the host target:

```sh
cargo test
cargo clippy --all-targets -- -D warnings
```

## Repository layout

| Path | What it is |
|---|---|
| `plugin.json` | The manifest: metadata, the `commands@0.1` capability, the runtime block (module, entries with input schemas, declared commands) |
| `wit/plugin.wit` | The plugin world `craft:plugin@0.1.0` the merged host serves |
| `src/gear.rs` | Involute geometry: radii, the closed outline, area and volume estimates |
| `src/contract.rs` | The entry's input schema and the input validation |
| `src/commands.rs` | The engine command list the plugin emits and the result summary |
| `src/lib.rs` | The WIT bindings and the `execute` loop |
| `src/tests.rs` | Unit tests, including volume checks against values the real engine measured |
| `tests/host.rs` | The component run through the real `craft-plugin`/`craft-plugin-wasm` host |

## License

[MIT](LICENSE).
