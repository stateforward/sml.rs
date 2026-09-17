//! Procedural macro implementation for the `stateforward-sml` state-machine DSL.

#![recursion_limit = "512"]
#![forbid(unsafe_code)]
#![deny(warnings)]
#![deny(
    elided_lifetimes_in_paths,
    missing_docs,
    rust_2018_idioms,
    unsafe_op_in_unsafe_fn,
    unused_must_use
)]
#![deny(
    clippy::all,
    clippy::pedantic,
    clippy::nursery,
    clippy::allow_attributes_without_reason,
    clippy::dbg_macro,
    clippy::mem_forget,
    clippy::todo,
    clippy::unimplemented,
    clippy::unseparated_literal_suffix
)]
// The generator is intentionally organized around large, explicit token
// builders and compatibility-preserving output order. These style lints do
// not improve generated-code safety and would obscure the checks below.
#![allow(
    clippy::explicit_iter_loop,
    clippy::items_after_statements,
    clippy::manual_let_else,
    clippy::map_unwrap_or,
    clippy::match_same_arms,
    clippy::missing_const_for_fn,
    clippy::needless_pass_by_value,
    clippy::option_if_let_else,
    clippy::or_fun_call,
    clippy::redundant_clone,
    clippy::redundant_closure_for_method_calls,
    clippy::redundant_pub_crate,
    clippy::ref_option,
    clippy::semicolon_if_nothing_returned,
    clippy::similar_names,
    clippy::single_match_else,
    clippy::struct_field_names,
    clippy::too_many_lines,
    clippy::uninlined_format_args,
    clippy::unnecessary_semicolon,
    clippy::unnecessary_wraps,
    clippy::use_self,
    clippy::wildcard_imports,
    reason = "these generator-internal style exceptions preserve explicit token-building structure"
)]
#![cfg_attr(
    not(test),
    deny(
        clippy::as_conversions,
        clippy::panic,
        clippy::unwrap_used,
        clippy::expect_used,
        clippy::indexing_slicing,
        clippy::arithmetic_side_effects,
        clippy::unreachable,
        clippy::cast_possible_truncation,
        clippy::cast_possible_wrap,
        clippy::cast_precision_loss,
        clippy::cast_sign_loss,
        clippy::exit,
        clippy::float_arithmetic,
        clippy::float_cmp,
        clippy::get_unwrap,
        clippy::infinite_loop,
        clippy::integer_division,
        clippy::integer_division_remainder_used,
        clippy::large_stack_arrays,
        clippy::let_underscore_must_use,
        clippy::lossy_float_literal,
        clippy::mixed_read_write_in_expression,
        clippy::modulo_arithmetic,
        clippy::option_env_unwrap,
        clippy::panicking_overflow_checks,
        clippy::panic_in_result_fn,
        clippy::rc_buffer,
        clippy::rc_mutex,
        clippy::string_slice,
        clippy::transmute_ptr_to_ptr,
        clippy::transmute_undefined_repr,
        clippy::uninit_assumed_init,
        clippy::unwrap_in_result
    )
)]

mod codegen;
mod composite_codegen;
#[cfg(feature = "graphviz")]
mod diagramgen;
mod event_codegen;
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
            if let Some(machine) = input
                .machines
                .iter()
                .find(|machine| !machine.event_generics.params.is_empty())
            {
                return syn::Error::new(
                    machine.name.as_ref().map_or_else(
                        proc_macro2::Span::call_site,
                        proc_macro2::Ident::span,
                    ),
                    "generic event declarations are currently supported by flat `sml!` tables; composite tables do not yet have dispatch-scoped generic event enums",
                )
                .to_compile_error()
                .into();
            }
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
    let Some(machine) = input.machines.into_iter().next() else {
        return syn::Error::new(proc_macro2::Span::call_site(), "parser requires a machine")
            .to_compile_error()
            .into();
    };
    if machine
        .transitions
        .iter()
        .filter(|transition| transition.in_state.start)
        .count()
        > 1
    {
        if !machine.event_generics.params.is_empty() {
            return syn::Error::new(
                machine.name.as_ref().map_or_else(
                    proc_macro2::Span::call_site,
                    proc_macro2::Ident::span,
                ),
                "generic event declarations are currently supported by flat `sml!` tables; orthogonal tables do not yet have dispatch-scoped generic event enums",
            )
            .to_compile_error()
            .into();
        }
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
                    let _removed = std::fs::remove_file(svg_name).is_ok();
                    let dot_name = format!("sml_{diagram_name}.dot");
                    let dot_path = std::env::var_os("OUT_DIR")
                        .map(std::path::PathBuf::from)
                        .map(|directory| directory.join(&dot_name))
                        .unwrap_or_else(|| std::env::temp_dir().join(dot_name));
                    let _written = std::fs::write(dot_path, diagram.as_bytes()).is_ok();
                }
            }

            // Validate the parsed state machine before generating code.
            if let Err(e) = validation::validate(&sm) {
                return e.to_compile_error().into();
            }

            match codegen::generate_code(&sm) {
                Ok(code) => code.into(),
                Err(error) => error.to_compile_error().into(),
            }
        }
        Err(error) => error.to_compile_error().into(),
    }
}
