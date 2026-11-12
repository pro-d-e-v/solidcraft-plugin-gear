//! `gear-generator`: a SolidCraft plugin that creates an involute spur gear.
//!
//! The plugin is a WebAssembly component against the `solidcraft:plugin@1.0.0`
//! world (RFC 0002, section 2). It changes the model only through the engine
//! commands that MCP and the control channel already serve, so every edit is
//! undoable and journaled, and the design opens without the plugin installed.
//!
//! The geometry and command building live in `gear`, `commands` and
//! `contract`, which are plain Rust and are unit tested on the host target.
//! Only this module needs the WIT bindings, so it is compiled just for wasm.
// The bindings call into `gear` and friends only under wasm; on the host the
// modules are exercised by the tests instead, so unused code is expected.
#![cfg_attr(not(target_arch = "wasm32"), allow(dead_code))]

mod commands;
mod contract;
mod gear;

#[cfg(test)]
mod tests;

#[cfg(target_arch = "wasm32")]
mod bindings {
    wit_bindgen::generate!({
        world: "plugin",
        path: "wit/solidcraft-plugin.wit",
    });

    use crate::commands;
    use crate::contract::{GearInput, PLUGIN_ID};

    struct GearGenerator;

    impl Guest for GearGenerator {
        /// The plugin's contract as JSON.
        fn manifest() -> String {
            crate::contract::manifest_json().to_string()
        }

        /// Validate the input, then run one engine command per call, in order.
        fn execute(params: String) -> Result<String, String> {
            let input: GearInput = serde_json::from_str(&params)
                .map_err(|e| format!("the parameters are not valid: {e}"))?;
            let spec = input.validate()?;

            log(&format!(
                "{PLUGIN_ID}: building a {}-tooth gear, module {} mm",
                spec.teeth, spec.module_mm
            ));

            let command_list = commands::build_commands(&spec);
            let count = command_list.len();
            for (index, command) in command_list.iter().enumerate() {
                let id = command["command"].as_str().unwrap_or_default().to_string();
                let args = command["params"].to_string();
                // `run` runs one engine command; a failure fails this execute
                // call, and the host leaves the design untouched (RFC 0002,
                // section 3: a trap gets a fresh instance next call).
                let result = run(&id, &args);
                if let Err(message) = result {
                    return Err(format!("step {index} ({id}) failed: {message}"));
                }
                if index % 16 == 0 || index + 1 == count {
                    progress(index as f64 / count.max(1) as f64);
                }
            }
            progress(1.0);
            log(&format!("{PLUGIN_ID}: {count} commands ran"));

            Ok(commands::summary_json(&spec, count).to_string())
        }
    }

    export!(GearGenerator);
}
