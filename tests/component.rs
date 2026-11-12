//! An integration test that loads the built WebAssembly component into a real
//! wasmtime, stubs the host imports, and calls `manifest` and `execute`.
//!
//! It needs the release component first:
//!
//! ```sh
//! cargo build --release --target wasm32-wasip2
//! cargo test
//! ```
//!
//! The stub `run` records every command it is given, which is exactly how the
//! plugin's emission is checked end to end without a running SolidCraft.

use serde_json::{json, Value};
use wasmtime::component::{Component, HasSelf, Linker, ResourceTable};
use wasmtime::{Config, Engine, Store};
use wasmtime_wasi::{WasiCtx, WasiCtxBuilder, WasiCtxView, WasiView};

wasmtime::component::bindgen!({
    world: "plugin",
    path: "wit/solidcraft-plugin.wit",
});

/// What the stub host saw, and how `run` answers.
struct HostState {
    commands: Vec<(String, String)>,
    /// How many `run` calls have come in so far.
    run_calls: usize,
    /// When set, the `run` call at this index fails with this message.
    fail_at: Option<(usize, String)>,
    table: ResourceTable,
    wasi: WasiCtx,
}

impl WasiView for HostState {
    fn ctx(&mut self) -> WasiCtxView<'_> {
        WasiCtxView {
            ctx: &mut self.wasi,
            table: &mut self.table,
        }
    }
}

impl PluginImports for HostState {
    fn run(&mut self, command: String, args: String) -> Result<String, String> {
        let index = self.run_calls;
        self.run_calls += 1;
        self.commands.push((command, args));
        if let Some((fail_at, message)) = &self.fail_at {
            if *fail_at == index {
                return Err(message.clone());
            }
        }
        Ok(json!({ "ok": true }).to_string())
    }

    fn log(&mut self, message: String) {
        self.commands.push(("#log".to_string(), message));
    }

    fn progress(&mut self, ratio: f64) {
        self.commands
            .push(("#progress".to_string(), format!("{ratio}")));
    }
}

/// The built release component. `cargo build --release --target wasm32-wasip2`
/// puts it here, locally and in CI alike.
fn component_path() -> std::path::PathBuf {
    std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("target/wasm32-wasip2/release/solidcraft_plugin_gear.wasm")
}

/// Load the component into a fresh wasmtime with the stub host.
fn setup(state: HostState) -> (Store<HostState>, Plugin) {
    let path = component_path();
    let bytes = std::fs::read(&path).unwrap_or_else(|e| panic!("the component is missing: build it first with `cargo build --release --target wasm32-wasip2` ({path:?}): {e}"));

    let mut config = Config::new();
    config.wasm_component_model(true);
    let engine = Engine::new(&config).unwrap();
    let component = Component::from_binary(&engine, &bytes).unwrap();

    let mut store = Store::new(&engine, state);
    let mut linker: Linker<HostState> = Linker::new(&engine);
    // The plugin imports no WASI slice; offering it is harmless when nothing
    // is imported, and covers the cdylib's own minimal imports when any.
    wasmtime_wasi::p2::add_to_linker_sync(&mut linker).unwrap();
    Plugin::add_to_linker::<HostState, HasSelf<HostState>>(&mut linker, |state| state).unwrap();
    let instance = Plugin::instantiate(&mut store, &component, &linker).unwrap();
    (store, instance)
}

fn default_params() -> String {
    json!({
        "teeth": 20, "module": 2.0, "pressure_angle": 20.0,
        "thickness": 8.0, "bore_diameter": 8.0, "name": "Gear"
    })
    .to_string()
}

#[test]
fn manifest_describes_the_plugin() {
    let (mut store, instance) = setup(HostState {
        commands: Vec::new(),
        run_calls: 0,
        fail_at: None,
        table: ResourceTable::new(),
        wasi: WasiCtxBuilder::new().build(),
    });
    let manifest: Value =
        serde_json::from_str(&instance.call_manifest(&mut store).unwrap()).unwrap();
    assert_eq!(manifest["id"], "gear-generator");
    assert_eq!(manifest["kind"], "command");
    assert_eq!(manifest["fallback"], "recorded_script");
    assert!(manifest["permissions"].as_array().unwrap().is_empty());
    let entry = &manifest["entries"][0];
    assert_eq!(entry["id"], "gear");
    assert_eq!(entry["parameters"][0]["name"], "teeth");
    assert_eq!(entry["parameters"][0]["default"], 20);
}

#[test]
fn execute_runs_every_engine_command() {
    let (mut store, instance) = setup(HostState {
        commands: Vec::new(),
        run_calls: 0,
        fail_at: None,
        table: ResourceTable::new(),
        wasi: WasiCtxBuilder::new().build(),
    });
    let summary: Value = serde_json::from_str(
        &instance
            .call_execute(&mut store, &default_params())
            .unwrap()
            .unwrap(),
    )
    .unwrap();
    assert_eq!(summary["ok"], true);
    assert_eq!(summary["commands_run"], 84); // 1 + 4*20 + bore + finish + extrude
    assert_eq!(summary["tip_diameter_mm"], 44.0);
    assert_eq!(summary["bbox_mm"]["max"][2], 8.0);

    let host = store.data();
    let ran: Vec<&(String, String)> = host
        .commands
        .iter()
        .filter(|(c, _)| !c.starts_with('#'))
        .collect();
    assert_eq!(ran[0].0, "sketch.create");
    assert_eq!(ran.last().unwrap().0, "solid.extrude");
    assert!(ran
        .iter()
        .any(|(c, a)| c == "sketch.circle.center" && a.contains("8")));
    // The tooth profile is on the XY plane.
    assert!(ran[0].1.contains("\"XY\""));
    // A flank polyline runs through its points in one call.
    let flank = ran.iter().find(|(c, _)| c == "sketch.line").unwrap();
    let points: Value = serde_json::from_str(flank.1.as_str()).unwrap();
    assert_eq!(points["points"].as_array().unwrap().len(), 14); // radial piece + 12-segment involute
}

#[test]
fn execute_refuses_invalid_parameters() {
    let (mut store, instance) = setup(HostState {
        commands: Vec::new(),
        run_calls: 0,
        fail_at: None,
        table: ResourceTable::new(),
        wasi: WasiCtxBuilder::new().build(),
    });
    let error = instance
        .call_execute(&mut store, &json!({ "teeth": 5 }).to_string())
        .unwrap()
        .unwrap_err();
    assert!(
        error.contains("`teeth` must be between 6 and 200"),
        "{error}"
    );
    // Nothing ran: the validation happens before any command.
    assert!(store.data().commands.is_empty());
}

#[test]
fn a_failed_command_fails_execute_with_the_step_named() {
    let (mut store, instance) = setup(HostState {
        commands: Vec::new(),
        run_calls: 0,
        fail_at: Some((2, "the sketch is locked".to_string())),
        table: ResourceTable::new(),
        wasi: WasiCtxBuilder::new().build(),
    });
    let error = instance
        .call_execute(&mut store, &default_params())
        .unwrap()
        .unwrap_err();
    assert!(error.contains("step 2 (sketch.line) failed"), "{error}");
    // The host saw only the commands before the failure, plus the log calls.
    let ran = store
        .data()
        .commands
        .iter()
        .filter(|(c, _)| !c.starts_with('#'))
        .count();
    assert_eq!(ran, 3); // sketch.create, flank 1, the failed flank 2
}
