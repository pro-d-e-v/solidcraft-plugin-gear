//! `gear-generator`: a SolidCraft plugin that creates an involute spur gear.
//!
//! The plugin is a WebAssembly component against the `craft:plugin@0.1.0`
//! world (the `commands@0.1` capability the merged plugin host serves). It
//! changes the model only through the engine commands that MCP and the control
//! channel already serve, so every edit is undoable and journaled, and the
//! design opens without the plugin installed.
//!
//! The geometry and command building live in `gear`, `commands` and
//! `contract`, which are plain Rust and are unit tested on the host target.
//! Only this module needs the WIT bindings, so it is compiled just for wasm.
// The bindings call into `gear` and friends only under wasm; on the host the
// modules are exercised by the tests instead, so unused code is expected.
#![cfg_attr(not(target_arch = "wasm32"), allow(dead_code))]
// The host refuses WASI imports, so the component imports nothing beyond the
// WIT: it builds `no_std` on `wasm32-unknown-unknown`, where `wasm32-wasip2`
// would drag the whole WASI CLI suite in through its reactor shim. The plain
// modules only need alloc (`core::f64` and `format!` cover the rest).
#![cfg_attr(all(target_arch = "wasm32", not(test)), no_std)]

extern crate alloc;

mod commands;
pub mod contract;
mod gear;

#[cfg(test)]
mod tests;

#[cfg(target_arch = "wasm32")]
mod bindings {
    wit_bindgen::generate!({
        world: "command-plugin",
        path: "wit/plugin.wit",
    });

    use crate::commands;
    use crate::contract::{GearInput, ENTRY_NAME};
    use alloc::{format, string::String, string::ToString};
    use craft::plugin::log::Level;
    use craft::plugin::types::{CommandFailure, FieldFailure, PluginError};

    /// A host failure as one readable line: the guest wraps engine failures so
    /// the user's error names the plugin's step, not the raw host error.
    fn host_error_text(error: PluginError) -> String {
        match error {
            PluginError::CommandFailed(failure) => {
                format!("command {} failed: {}", failure.command, failure.reason)
            }
            PluginError::PermissionDenied(permission) => format!("permission denied: {permission}"),
            PluginError::LimitExceeded(limit) => format!("the plugin's limit was hit: {limit}"),
            PluginError::InvalidManifest(failure) => {
                format!(
                    "the manifest is wrong ({}): {}",
                    failure.field, failure.reason
                )
            }
            PluginError::InvalidInput(failure) => {
                format!("the input is wrong ({}): {}", failure.field, failure.reason)
            }
            PluginError::Trap(reason) => format!("the plugin trapped: {reason}"),
            PluginError::Cancelled => "the plugin was cancelled".to_string(),
        }
    }

    struct GearGenerator;

    impl exports::craft::plugin::entry::Guest for GearGenerator {
        /// Validate the input, then run one engine command per call, in order.
        fn execute(entry: String, params: String) -> Result<String, PluginError> {
            if entry != ENTRY_NAME {
                return Err(PluginError::InvalidInput(FieldFailure {
                    field: "entry".to_string(),
                    reason: format!(
                        "unknown entry `{entry}`: this component exports `{ENTRY_NAME}`"
                    ),
                }));
            }
            let input: GearInput = serde_json::from_str(&params).map_err(|error| {
                PluginError::InvalidInput(FieldFailure {
                    field: "params".to_string(),
                    reason: format!("the parameters are not valid: {error}"),
                })
            })?;
            let spec = input.validate().map_err(|message| {
                PluginError::InvalidInput(FieldFailure {
                    field: "params".to_string(),
                    reason: message,
                })
            })?;

            craft::plugin::log::log(
                Level::Info,
                &format!(
                    "{}: building a {}-tooth gear, module {} mm",
                    crate::contract::PLUGIN_ID,
                    spec.teeth,
                    spec.module_mm
                ),
            );

            let command_list = commands::build_commands(&spec);
            let count = command_list.len();
            for (index, command) in command_list.iter().enumerate() {
                // A cancelled run fails the call; the host rolls the
                // transaction back and the design is untouched.
                if index % 16 == 0 && craft::plugin::control::cancelled() {
                    return Err(PluginError::Cancelled);
                }
                let id = command["command"].as_str().unwrap_or_default().to_string();
                let args = command["params"].to_string();
                // `run` runs one engine command; a failure fails this execute
                // call, and the host rolls the transaction back, so the design
                // is untouched.
                if let Err(error) = craft::plugin::commands::run(&id, &args) {
                    return Err(PluginError::CommandFailed(CommandFailure {
                        command: id,
                        reason: format!("step {index}: {}", host_error_text(error)),
                    }));
                }
                if index % 16 == 0 || index + 1 == count {
                    craft::plugin::control::progress(
                        index as f32 / count.max(1) as f32,
                        &format!("gear tooth sketch {index} of {count}"),
                    );
                }
            }
            craft::plugin::control::progress(1.0, "the gear is built");
            craft::plugin::log::log(
                Level::Info,
                &format!("{}: {count} commands ran", crate::contract::PLUGIN_ID),
            );

            Ok(commands::summary_json(&spec, count).to_string())
        }
    }

    export!(GearGenerator);
}
