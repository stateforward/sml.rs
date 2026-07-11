#![recursion_limit = "512"]

extern crate proc_macro;

mod codegen;
mod composite_codegen;
#[cfg(feature = "graphviz")]
mod diagramgen;
mod orthogonal_codegen;
mod parser;
mod validation;

use syn::parse_macro_input;

/// Defines a state machine using sml.cpp-shaped transition-table syntax.
///
/// ```ignore
/// sml! {
///     Player {
///         *"empty"_s + event<OpenClose> / open_drawer = "open"_s,
///          "open"_s + event<OpenClose> / close_drawer = "empty"_s,
///     }
/// }
/// ```
#[proc_macro]
pub fn sml(input: proc_macro::TokenStream) -> proc_macro::TokenStream {
    let input = parse_macro_input!(input as parser::cpp::SmlDefinitions);
    if input.machines.len() > 1 {
        let machine_names = input
            .machines
            .iter()
            .filter_map(|machine| machine.name.as_ref())
            .collect::<Vec<_>>();
        let has_composite = input.machines.iter().any(|machine| {
            machine.transitions.iter().any(|transition| {
                transition
                    .in_state
                    .composite
                    .as_ref()
                    .or(transition.out_state.composite.as_ref())
                    .is_some_and(|reference| machine_names.contains(&reference))
            })
        });
        if has_composite {
            return match composite_codegen::generate_code(&input.machines) {
                Ok(code) => code.into(),
                Err(error) => error.to_compile_error().into(),
            };
        }
        let mut output = proc_macro2::TokenStream::new();
        for machine in input.machines {
            output.extend(proc_macro2::TokenStream::from(expand(machine)));
        }
        return output.into();
    }
    let machine = input
        .machines
        .into_iter()
        .next()
        .expect("parser requires a machine");
    if machine
        .transitions
        .iter()
        .filter(|transition| transition.in_state.start)
        .count()
        > 1
    {
        return match orthogonal_codegen::generate_code(&machine) {
            Ok(code) => code.into(),
            Err(error) => error.to_compile_error().into(),
        };
    }
    expand(machine)
}

fn expand(input: parser::state_machine::StateMachine) -> proc_macro::TokenStream {
    match parser::ParsedStateMachine::new(input) {
        // Generate code and hand the output tokens back to the compiler
        Ok(sm) => {
            #[cfg(feature = "graphviz")]
            {
                use std::hash::{Hash, Hasher};
                use std::io::Write;

                // Generate DOT syntax for the state machine.
                let diagram = diagramgen::generate_diagram(&sm);
                let diagram_name = if let Some(name) = &sm.name {
                    name.to_string()
                } else {
                    let mut diagram_hasher = std::collections::hash_map::DefaultHasher::new();
                    diagram.hash(&mut diagram_hasher);
                    format!("sml{:010x}", diagram_hasher.finish())
                };

                // Render SVG when Graphviz is available. Otherwise retain the
                // DOT source instead of making compilation depend on a host
                // executable.
                let svg_name = format!("sml_{diagram_name}.svg");
                let rendered = std::process::Command::new("dot")
                    .args(["-Tsvg", "-o", &svg_name])
                    .stdin(std::process::Stdio::piped())
                    .spawn()
                    .ok()
                    .map(|mut process| {
                        let wrote_input = process
                            .stdin
                            .as_mut()
                            .map(|stdin| stdin.write_all(diagram.as_bytes()).is_ok())
                            .unwrap_or(false);
                        wrote_input
                            && process
                                .wait()
                                .map(|status| status.success())
                                .unwrap_or(false)
                    })
                    .unwrap_or(false);

                if !rendered {
                    let _ = std::fs::remove_file(svg_name);
                    let dot_name = format!("sml_{diagram_name}.dot");
                    let dot_path = std::env::var_os("OUT_DIR")
                        .map(std::path::PathBuf::from)
                        .map(|directory| directory.join(&dot_name))
                        .unwrap_or_else(|| std::env::temp_dir().join(dot_name));
                    let _ = std::fs::write(dot_path, diagram.as_bytes());
                }
            }

            // Validate the parsed state machine before generating code.
            if let Err(e) = validation::validate(&sm) {
                return e.to_compile_error().into();
            }

            codegen::generate_code(&sm).into()
        }
        Err(error) => error.to_compile_error().into(),
    }
}
