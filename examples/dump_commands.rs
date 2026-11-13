//! Runs the built WebAssembly component on the real plugin host
//! (`craft-plugin` + `craft-plugin-wasm`, from the SolidCraft repository) and
//! writes the engine commands its `gear` entry emits to a JSON file, so the
//! list can be replayed against a real SolidCraft engine (`solidcraft-cli run`
//! or the MCP `batch` with a `path`).
//!
//! ```sh
//! cargo build --release --target wasm32-unknown-unknown
//! cargo run --example dump_commands -- default.json
//! cargo run --example dump_commands -- teeth=30 module=3 gear-30.json
//! ```

use craft_plugin::{parse_manifest, CommandSchema, Effect, FileGrant, PluginHost, Result};
use serde_json::{json, Value};

/// A stub host: every engine command succeeds and is recorded.
struct Recorder {
    commands: Vec<(String, Value)>,
}

impl PluginHost for Recorder {
    fn command_catalog(&self) -> Vec<CommandSchema> {
        // The manifest's declared commands, all of which the catalog serves.
        std::fs::read(manifest_path())
            .ok()
            .and_then(|bytes| parse_manifest(&bytes, "gear-generator").ok())
            .map(|manifest| {
                manifest
                    .runtime
                    .expect("runtime")
                    .commands
                    .iter()
                    .map(|name| CommandSchema {
                        name: name.clone(),
                        input_schema: json!({}),
                        output_schema: json!({}),
                        effect: Effect::Edit,
                        group: None,
                    })
                    .collect()
            })
            .unwrap_or_default()
    }

    fn run_command(&mut self, name: &str, args: &Value) -> Result<Value> {
        self.commands.push((name.to_string(), args.clone()));
        Ok(json!({}))
    }

    fn begin_transaction(&mut self) -> Result<()> {
        Ok(())
    }

    fn commit_transaction(&mut self) -> Result<()> {
        Ok(())
    }

    fn rollback_transaction(&mut self) -> Result<()> {
        Ok(())
    }

    fn granted_folders(&self) -> Vec<FileGrant> {
        Vec::new()
    }

    fn log(&mut self, message: &str) {
        println!("plugin log: {message}");
    }

    fn progress(&mut self, _fraction: f32, _message: &str) {}
}

fn manifest_path() -> std::path::PathBuf {
    std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("plugin.json")
}

fn main() {
    // Any `key=value` arguments override the entry schema's defaults
    // (`teeth=30 module=3` and so on); the first plain argument is the
    // output file.
    let overrides = std::env::args().skip(1).filter(|a| a.contains('='));
    let out_path = std::env::args()
        .skip(1)
        .find(|a| !a.contains('=') && a != "--")
        .unwrap_or_else(|| "commands.json".to_string());

    let manifest_bytes =
        std::fs::read(manifest_path()).unwrap_or_else(|e| panic!("plugin.json is missing: {e}"));
    let manifest = parse_manifest(&manifest_bytes, "gear-generator")
        .unwrap_or_else(|e| panic!("the host refuses our manifest: {e:?}"));
    println!(
        "manifest: {} v{}, entry {:?}, capabilities {:?}",
        manifest.id,
        manifest.version,
        manifest.runtime.as_ref().expect("runtime").entries[0].name,
        manifest.capabilities
    );

    let wasm_path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("target/wasm32-unknown-unknown/release/solidcraft_plugin_gear.wasm");
    let module = std::fs::read(&wasm_path).unwrap_or_else(|e| {
        panic!("the core module is missing ({wasm_path:?}): build it first: {e}")
    });
    let bytes = wit_component::ComponentEncoder::default()
        .validate(true)
        .module(&module)
        .and_then(|mut encoder| encoder.encode())
        .unwrap_or_else(|e| panic!("the core module does not lift into a component: {e}"));

    // The defaults from the manifest's entry schema, overridden by any
    // `key=value` arguments (`teeth=30 module=3` and so on).
    let schema = &manifest.runtime.as_ref().expect("runtime").entries[0].input_schema;
    let mut params = serde_json::Map::new();
    for (name, property) in schema["properties"].as_object().expect("schema properties") {
        params.insert(name.clone(), property["default"].clone());
    }
    for argument in overrides {
        if let Some((key, value)) = argument.split_once('=') {
            let value = value
                .parse::<i64>()
                .map(Value::from)
                .or_else(|_| value.parse::<f64>().map(Value::from))
                .or_else(|_| match value.trim().to_ascii_lowercase().as_str() {
                    "true" => Ok(Value::Bool(true)),
                    "false" => Ok(Value::Bool(false)),
                    _ => Err(()),
                })
                .unwrap_or(Value::String(value.to_string()));
            params.insert(key.to_string(), value);
        }
    }

    let (host, result) = craft_plugin_wasm::run(
        Recorder {
            commands: Vec::new(),
        },
        &manifest,
        &bytes,
        "gear",
        &Value::Object(params),
    );
    let summary = result.unwrap_or_else(|e| panic!("the run failed: {e:?}"));
    println!(
        "summary: {}",
        serde_json::to_string_pretty(&summary).unwrap()
    );

    let steps: Vec<Value> = host
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
