//! Integration tests that run the built WebAssembly component on the real,
//! merged plugin host: `craft-plugin` parses the manifest, `craft-plugin-wasm`
//! instantiates the component in the sandbox and calls the entry.
//!
//! It needs the release component first:
//!
//! ```sh
//! cargo build --release --target wasm32-unknown-unknown
//! cargo test
//! ```
//!
//! The stub `PluginHost` records every command it is given, which is exactly
//! how the plugin's emission is checked end to end without a running
//! SolidCraft.

use craft_plugin::{
    parse_manifest, CommandSchema, Effect, FileGrant, HostError, PluginHost, Result,
};
use serde_json::{json, Value};

/// The exact commands the plugin emits; the static manifest lists them all.
const COMMANDS: &[&str] = &[
    "sketch.circle.center",
    "sketch.create",
    "sketch.finish",
    "sketch.line",
    "solid.extrude",
];

/// A stub host: every engine command succeeds and is recorded, unless
/// `fail_at` makes the call at that index fail the way an engine would.
struct StubHost {
    commands: Vec<(String, Value)>,
    run_calls: usize,
    fail_at: Option<(usize, String)>,
    committed: bool,
    rolled_back: bool,
}

impl StubHost {
    fn new(fail_at: Option<(usize, String)>) -> Self {
        StubHost {
            commands: Vec::new(),
            run_calls: 0,
            fail_at,
            committed: false,
            rolled_back: false,
        }
    }
}

impl PluginHost for StubHost {
    fn command_catalog(&self) -> Vec<CommandSchema> {
        COMMANDS
            .iter()
            .map(|name| CommandSchema {
                name: (*name).to_string(),
                input_schema: json!({}),
                output_schema: json!({}),
                effect: Effect::Edit,
                group: None,
            })
            .collect()
    }

    fn run_command(&mut self, name: &str, args: &Value) -> Result<Value> {
        let index = self.run_calls;
        self.run_calls += 1;
        self.commands.push((name.to_string(), args.clone()));
        if let Some((fail_at, reason)) = &self.fail_at {
            if *fail_at == index {
                return Err(HostError::CommandFailed {
                    command: name.to_string(),
                    reason: reason.clone(),
                });
            }
        }
        Ok(json!({}))
    }

    fn begin_transaction(&mut self) -> Result<()> {
        Ok(())
    }

    fn commit_transaction(&mut self) -> Result<()> {
        self.committed = true;
        Ok(())
    }

    fn rollback_transaction(&mut self) -> Result<()> {
        self.rolled_back = true;
        Ok(())
    }

    fn granted_folders(&self) -> Vec<FileGrant> {
        Vec::new()
    }

    fn log(&mut self, message: &str) {
        println!("plugin log: {message}");
    }

    fn progress(&mut self, fraction: f32, message: &str) {
        println!("plugin progress: {fraction:.2} {message}");
    }
}

/// The static manifest, parsed by the real host's parser.
fn parsed_manifest() -> craft_plugin::Manifest {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("plugin.json");
    let bytes = std::fs::read(&path).unwrap_or_else(|e| panic!("plugin.json is missing: {e}"));
    parse_manifest(&bytes, "gear-generator")
        .unwrap_or_else(|e| panic!("the host refuses our manifest: {e:?}"))
}

/// The built release component. `cargo build --release --target wasm32-unknown-unknown`
/// puts the core module here, locally and in CI alike, and it is lifted into a
/// real component here. The unknown-unknown target is used because the host
/// refuses WASI imports, and the `wasm32-wasip2` target links the WASI CLI
/// suite through its reactor shim even from a `no_std` crate.
fn component_bytes() -> Vec<u8> {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("target/wasm32-unknown-unknown/release/solidcraft_plugin_gear.wasm");
    let module = std::fs::read(&path).unwrap_or_else(|e| {
        panic!(
            "the core module is missing: build it first with `cargo build --release --target wasm32-unknown-unknown` ({path:?}): {e}"
        )
    });
    wit_component::ComponentEncoder::default()
        .validate(true)
        .module(&module)
        .and_then(|mut encoder| encoder.encode())
        .unwrap_or_else(|e| panic!("the core module does not lift into a component: {e}"))
}

fn default_params() -> Value {
    json!({
        "teeth": 20, "module": 2.0, "pressure_angle": 20.0,
        "thickness": 8.0, "bore_diameter": 8.0, "name": "Gear"
    })
}

/// The manifest's menu slot names the entry the way the user sees it.
#[test]
fn the_entry_carries_a_menu_slot() {
    let manifest = parsed_manifest();
    let entry = &manifest.runtime.expect("runtime").entries[0];
    let slot = &entry.slots[0];
    let slot = serde_json::to_value(slot).unwrap_or_default();
    assert_eq!(slot["kind"], "menu");
    assert_eq!(slot["name"], "Создать шестерню…");
}

/// With the keyway on, the cut sketch and extrude follow the main extrude.
#[test]
fn the_keyway_cut_runs_after_the_gear() {
    let manifest = parsed_manifest();
    let mut params = default_params();
    params["keyway"] = json!(true);
    params["keyway_width"] = json!(4.0);
    params["keyway_depth"] = json!(2.0);
    params["keyway_angle"] = json!(30.0);
    let (host, result) = craft_plugin_wasm::run(
        StubHost::new(None),
        &manifest,
        &component_bytes(),
        "gear",
        &params,
    );
    let summary = result.unwrap_or_else(|e| panic!("the run failed: {e:?}"));
    assert_eq!(summary["commands_run"], 9);
    assert_eq!(summary["keyway"]["width_mm"], 4.0);
    assert_eq!(summary["keyway"]["angle_deg"], 30.0);

    // The last four commands: sketch.create, the closed slot line, finish,
    // the cut extrude.
    let ran = &host.commands;
    assert_eq!(ran[ran.len() - 4].0, "sketch.create");
    let slot = &ran[ran.len() - 3];
    assert_eq!(slot.0, "sketch.line");
    let points = slot.1["points"].as_array().expect("points");
    assert_eq!(points.len(), 4);
    assert_eq!(ran[ran.len() - 2].0, "sketch.finish");
    let cut = &ran[ran.len() - 1];
    assert_eq!(cut.0, "solid.extrude");
    assert_eq!(cut.1["operation"], "cut");
    assert_eq!(cut.1["through_all"], true);
    assert_eq!(cut.1["targets"], json!(["Gear"]));
    assert!(host.committed);
}

#[test]
fn the_merged_host_accepts_the_static_manifest() {
    let manifest = parsed_manifest();
    assert_eq!(manifest.id, "gear-generator");
    assert_eq!(manifest.version, "0.4.0");
    assert_eq!(manifest.capabilities, vec!["commands@0.1".to_string()]);
    let runtime = manifest.runtime.expect("the manifest declares a runtime");
    assert_eq!(runtime.module, "gear-generator.wasm");
    assert_eq!(runtime.entries.len(), 1);
    assert_eq!(runtime.entries[0].name, "gear");
    assert_eq!(runtime.entries[0].effect, Effect::Edit);
    // Every emitted command is declared, so the host's catalog intersection
    // lets all of them through.
    let declared: Vec<&String> = runtime.commands.iter().collect();
    assert_eq!(declared.len(), COMMANDS.len());
    for command in COMMANDS {
        assert!(
            runtime.commands.contains(*command),
            "{command} is not declared"
        );
    }
    // The package asks for the budget a 200-tooth gear needs; the host
    // ceilings are 100,000,000,000 fuel, 128 MiB and 30 s.
    assert_eq!(runtime.limits.fuel, 500_000_000);
    assert_eq!(runtime.limits.memory_mib, 64);
    assert_eq!(runtime.limits.deadline_ms, 30_000);
    // No permissions are requested at all.
    assert!(runtime.permissions.files.is_empty());
    assert!(!runtime.permissions.clock);
    assert!(runtime.permissions.command_groups.is_empty());
}

#[test]
fn the_merged_host_runs_the_component_end_to_end() {
    let manifest = parsed_manifest();
    let (host, result) = craft_plugin_wasm::run(
        StubHost::new(None),
        &manifest,
        &component_bytes(),
        "gear",
        &default_params(),
    );
    let summary = result.unwrap_or_else(|e| panic!("the run failed: {e:?}"));
    assert_eq!(summary["ok"], true);
    assert_eq!(summary["commands_run"], 5); // create, line, circle, finish, extrude
    assert_eq!(summary["tip_diameter_mm"], 44.0);
    assert_eq!(summary["bbox_mm"]["max"][2], 8.0);

    // The host saw the whole emission and committed the transaction.
    let ran: Vec<&(String, Value)> = host.commands.iter().collect();
    assert_eq!(ran.len(), 5);
    assert_eq!(ran[0].0, "sketch.create");
    assert_eq!(ran.last().unwrap().0, "solid.extrude");
    assert!(host.committed);
    assert!(!host.rolled_back);
    // The bore circle came through with its diameter.
    assert!(ran
        .iter()
        .any(|(c, a)| c == "sketch.circle.center" && a["diameter"] == 8.0));
    // The tooth profile is on the XY plane.
    assert_eq!(ran[0].1["plane"], "XY");
    // The whole outline runs through one closed line: 30 points per tooth.
    let outline = ran.iter().find(|(c, _)| c == "sketch.line").unwrap();
    assert_eq!(outline.1["points"].as_array().unwrap().len(), 600);
    assert_eq!(outline.1["closed"], true);
}

#[test]
fn bad_parameters_fail_before_any_command_and_roll_back() {
    let manifest = parsed_manifest();
    let (host, result) = craft_plugin_wasm::run(
        StubHost::new(None),
        &manifest,
        &component_bytes(),
        "gear",
        &json!({ "teeth": 5 }),
    );
    match result {
        Err(HostError::InvalidInput { field, reason }) => {
            assert_eq!(field, "params");
            assert!(
                reason.contains("`teeth` must be between 6 and 200"),
                "{reason}"
            );
        }
        other => panic!("expected InvalidInput, got {other:?}"),
    }
    assert!(host.commands.is_empty());
    assert!(host.rolled_back);
    assert!(!host.committed);
}

#[test]
fn an_unknown_entry_is_refused_with_the_exported_one_named() {
    let manifest = parsed_manifest();
    let (host, result) = craft_plugin_wasm::run(
        StubHost::new(None),
        &manifest,
        &component_bytes(),
        "toothbrush",
        &default_params(),
    );
    match result {
        Err(HostError::InvalidInput { field, reason }) => {
            assert_eq!(field, "entry");
            // The host refuses an entry the manifest does not declare, before
            // any guest code runs.
            assert!(reason.contains("unknown entry"), "{reason}");
        }
        other => panic!("expected InvalidInput, got {other:?}"),
    }
    // The host refused the entry before any code ran: no command ran and no
    // transaction was ever opened.
    assert!(host.commands.is_empty());
    assert!(!host.committed);
}

#[test]
fn a_failed_engine_command_names_the_plugin_step_and_rolls_back() {
    let manifest = parsed_manifest();
    let (host, result) = craft_plugin_wasm::run(
        StubHost::new(Some((2, "the sketch is locked".to_string()))),
        &manifest,
        &component_bytes(),
        "gear",
        &default_params(),
    );
    match result {
        Err(HostError::CommandFailed { command, reason }) => {
            // A failed `run` surfaces verbatim: the runtime reports the host's
            // own error for the failed command and rolls the transaction back,
            // whatever the guest then returns.
            assert_eq!(command, "sketch.circle.center");
            assert!(reason.contains("the sketch is locked"), "{reason}");
        }
        other => panic!("expected CommandFailed, got {other:?}"),
    }
    // The host saw only the commands before the failure.
    assert_eq!(host.run_calls, 3); // sketch.create, sketch.line, the failed circle
    assert!(host.rolled_back);
    assert!(!host.committed);
}

/// The input schema the static manifest declares matches the schema this crate
/// keeps next to the validator: same fields, ranges and defaults, so the form
/// the host draws cannot hand `execute` something the validator refuses.
#[test]
fn the_manifest_input_schema_matches_the_validator() {
    let manifest = parsed_manifest();
    let runtime = manifest.runtime.expect("runtime");
    let declared = &runtime.entries[0].input_schema;
    let checked = solidcraft_plugin_gear::contract::input_schema_json();
    assert_eq!(declared, &checked);

    // And the schema's defaults validate cleanly.
    let defaults: Value = declared["properties"]
        .as_object()
        .unwrap()
        .iter()
        .map(|(name, property)| (name.clone(), property["default"].clone()))
        .collect();
    let input: solidcraft_plugin_gear::contract::GearInput =
        serde_json::from_value(defaults).expect("the defaults are valid");
    input.validate().expect("the defaults pass validation");

    // The manifest's commands are all lowercase engine ids.
    for command in runtime.commands.iter() {
        assert!(
            command.bytes().all(|c| c.is_ascii_lowercase()
                || c.is_ascii_digit()
                || matches!(c, b'.' | b'_' | b'-')),
            "{command} is not a lowercase command id"
        );
    }
}
