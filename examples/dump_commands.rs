//! Runs the built WebAssembly component and writes the engine commands its
//! `execute` call emits to a JSON file, so the list can be replayed against a
//! real SolidCraft engine (`solidcraft-cli run` or the MCP `batch` with a
//! `path`) while the app's own plugin host is not available yet.
//!
//! ```sh
//! cargo build --release --target wasm32-wasip2
//! cargo run --example dump_commands -- default.json
//! ```

use serde_json::{json, Value};
use wasmtime::component::{Component, HasSelf, Linker, ResourceTable};
use wasmtime::{Config, Engine, Store};
use wasmtime_wasi::{WasiCtx, WasiCtxBuilder, WasiCtxView, WasiView};

wasmtime::component::bindgen!({
    world: "plugin",
    path: "wit/solidcraft-plugin.wit",
});

/// Records what the plugin asked the host to do.
struct Recorder {
    commands: Vec<(String, Value)>,
    table: ResourceTable,
    wasi: WasiCtx,
}

impl WasiView for Recorder {
    fn ctx(&mut self) -> WasiCtxView<'_> {
        WasiCtxView {
            ctx: &mut self.wasi,
            table: &mut self.table,
        }
    }
}

impl PluginImports for Recorder {
    fn run(&mut self, command: String, args: String) -> Result<String, String> {
        let args: Value = serde_json::from_str(&args).unwrap_or_else(|_| json!({}));
        self.commands.push((command, args));
        Ok(json!({ "ok": true }).to_string())
    }

    fn log(&mut self, message: String) {
        println!("plugin log: {message}");
    }

    fn progress(&mut self, _ratio: f64) {}
}

fn main() {
    // Any `key=value` arguments override the entry schema's defaults
    // (`teeth=30 module=3` and so on); the first plain argument is the
    // output file.
    let mut overrides = std::env::args().skip(1).filter(|a| a.contains('='));
    let out_path = std::env::args()
        .skip(1)
        .find(|a| !a.contains('=') && a != "--")
        .unwrap_or_else(|| "commands.json".to_string());

    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("target/wasm32-wasip2/release/solidcraft_plugin_gear.wasm");
    let bytes = std::fs::read(&path)
        .unwrap_or_else(|e| panic!("the component is missing ({path:?}): build it first: {e}"));

    let mut config = Config::new();
    config.wasm_component_model(true);
    let engine = Engine::new(&config).unwrap();
    let component = Component::from_binary(&engine, &bytes).unwrap();

    let mut store = Store::new(
        &engine,
        Recorder {
            commands: Vec::new(),
            table: ResourceTable::new(),
            wasi: WasiCtxBuilder::new().build(),
        },
    );
    let mut linker: Linker<Recorder> = Linker::new(&engine);
    wasmtime_wasi::p2::add_to_linker_sync(&mut linker).unwrap();
    Plugin::add_to_linker::<Recorder, HasSelf<Recorder>>(&mut linker, |state| state).unwrap();
    let instance = Plugin::instantiate(&mut store, &component, &linker).unwrap();

    let manifest: Value =
        serde_json::from_str(&instance.call_manifest(&mut store).unwrap()).unwrap();
    println!(
        "manifest: {} v{}, entry {:?}",
        manifest["id"], manifest["version"], manifest["entries"][0]["id"]
    );

    // The defaults from the manifest's entry schema, overridden by any
    // `key=value` arguments (`teeth=30 module=3` and so on).
    let mut params = serde_json::Map::new();
    for parameter in manifest["entries"][0]["parameters"].as_array().unwrap() {
        params.insert(
            parameter["name"].as_str().unwrap().to_string(),
            parameter["default"].clone(),
        );
    }
    for argument in overrides {
        if let Some((key, value)) = argument.split_once('=') {
            let value = value
                .parse::<i64>()
                .map(Value::from)
                .or_else(|_| value.parse::<f64>().map(Value::from))
                .unwrap_or(Value::String(value.to_string()));
            params.insert(key.to_string(), value);
        }
    }

    let summary: Value = serde_json::from_str(
        &instance
            .call_execute(&mut store, &Value::Object(params).to_string())
            .unwrap()
            .unwrap(),
    )
    .unwrap();
    println!(
        "summary: {}",
        serde_json::to_string_pretty(&summary).unwrap()
    );

    let steps: Vec<Value> = store
        .data()
        .commands
        .iter()
        .map(|(command, params)| json!({ "command": command, "params": params }))
        .collect();
    serde_json::to_writer_pretty(
        std::fs::File::create(&out_path).unwrap(),
        &json!({ "commands": steps }),
    )
    .unwrap();
    println!("{} engine commands -> {out_path}", steps.len());
}
