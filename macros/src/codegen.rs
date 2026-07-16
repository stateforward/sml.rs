// Move guards to return a Result

use crate::parser::event::EventKind;
use crate::parser::transition::visit_guards;
use crate::parser::{lifetimes::Lifetimes, transition::EvalAction, AsyncIdent, ParsedStateMachine};
use proc_macro2::{Ident, Span, TokenStream};
use quote::{format_ident, quote};
use std::collections::HashSet;
use syn::visit::Visit;
use syn::{parse_quote, Type};

#[derive(Default)]
struct SourceIdents(HashSet<String>);

impl<'ast> Visit<'ast> for SourceIdents {
    fn visit_ident(&mut self, ident: &'ast Ident) {
        self.0.insert(ident.to_string());
    }
}

fn source_idents(sm: &ParsedStateMachine, event_generics: &syn::Generics) -> SourceIdents {
    let mut idents = SourceIdents::default();
    idents.visit_generics(event_generics);
    for data_type in sm
        .state_data
        .data_types
        .values()
        .chain(sm.event_data.data_types.values())
    {
        idents.visit_type(data_type);
    }
    if let Some(temporary_context) = &sm.temporary_context_type {
        idents.visit_type(temporary_context);
    }
    if let Some(error_type) = &sm.fixed_error_type {
        idents.visit_type(error_type);
    }
    idents
}

fn fresh_type_ident(reserved: &mut SourceIdents, base: &str) -> Ident {
    for suffix in 0_u32.. {
        let name = if suffix == 0 {
            base.to_owned()
        } else {
            format!("{base}{suffix}")
        };
        if reserved.0.insert(name.clone()) {
            return Ident::new(&name, Span::call_site());
        }
    }
    unreachable!("finite generic parameter list always leaves a fresh identifier")
}

pub fn generate_code(sm: &ParsedStateMachine) -> proc_macro2::TokenStream {
    let (sm_name, sm_name_span) = sm
        .name
        .as_ref()
        .map(|name| (name.to_string(), name.span()))
        .unwrap_or_else(|| (String::new(), Span::call_site()));
    let states_type_name = format_ident!("{sm_name}States", span = sm_name_span);
    let events_type_name = format_ident!("{sm_name}Events", span = sm_name_span);
    let completion_origin_type_name =
        format_ident!("{sm_name}CompletionOrigin", span = sm_name_span);
    let error_type_name = format_ident!("{sm_name}Error", span = sm_name_span);
    let state_machine_type_name = format_ident!("{sm_name}StateMachine", span = sm_name_span);
    let state_machine_context_type_name =
        format_ident!("{sm_name}StateMachineContext", span = sm_name_span);
    let event_generics = sm.event_generics_with_lifetimes(&sm.event_data.all_lifetimes);
    let mut reserved_idents = source_idents(sm, &event_generics);
    let context_type_ident = fresh_type_ident(&mut reserved_idents, "__SmlContext");
    let event_input_ident = fresh_type_ident(&mut reserved_idents, "__SmlEventInput");
    let (event_impl_generics, event_type_generics, event_where_clause) =
        event_generics.split_for_impl();

    // Get only the unique states
    let mut state_list: Vec<_> = sm.states.values().collect();
    state_list.sort_by_key(|state| state.to_string());

    let state_list: Vec<_> = state_list
        .iter()
        .map(
            |value| match sm.state_data.data_types.get(&value.to_string()) {
                None => {
                    quote! {
                        #value
                    }
                }
                Some(t) => {
                    quote! {
                        #value(#t)
                    }
                }
            },
        )
        .collect();

    // Extract events
    let mut event_list: Vec<_> = sm.events.values().collect();
    event_list.sort_by_key(|event| event.to_string());

    // Extract events
    let event_list: Vec<_> = event_list
        .iter()
        .map(
            |value| match sm.event_data.data_types.get(&value.to_string()) {
                None => {
                    quote! {
                        #value
                    }
                }
                Some(t) => {
                    quote! {
                        #value(#t)
                    }
                }
            },
        )
        .collect();

    let mut completion_events = Vec::new();
    let mut has_anonymous_completion = false;
    let mut external_events = Vec::new();
    for event_mappings in sm.states_events_mapping.values() {
        for mapping in event_mappings.values() {
            if mapping.event_external
                && !external_events
                    .iter()
                    .any(|event: &Ident| event == &mapping.event)
            {
                external_events.push(mapping.event.clone());
            }
            if mapping.event_kind == EventKind::Completion && mapping.event_wildcard {
                has_anonymous_completion = true;
            } else if mapping.event_kind == EventKind::Completion
                && !completion_events
                    .iter()
                    .any(|event: &Ident| event == &mapping.event)
            {
                completion_events.push(mapping.event.clone());
            }
        }
    }
    completion_events.sort_by_key(|event| event.to_string());
    external_events.sort_by_key(|event| event.to_string());
    let has_completion_events = has_anonymous_completion || !completion_events.is_empty();
    let has_exception_handlers = sm
        .states_events_mapping
        .values()
        .flat_map(|mappings| mappings.values())
        .any(|mapping| mapping.event_kind == EventKind::Exception);
    let has_deferred_events = sm
        .states_events_mapping
        .values()
        .flat_map(|mappings| mappings.values())
        .flat_map(|mapping| mapping.transitions.iter())
        .any(|transition| transition.defer);

    let transitions = &sm.states_events_mapping;

    let in_states: Vec<_> = transitions
        .keys()
        .map(|name| {
            let state_name = sm.states.get(name).unwrap();

            match sm.state_data.data_types.get(name) {
                None => {
                    quote! {
                        #state_name
                    }
                }
                Some(_) => {
                    quote! {
                        #state_name(ref state_data)
                    }
                }
            }
        })
        .collect();

    let events: Vec<Vec<_>> = transitions
        .values()
        .map(|value| {
            value
                .values()
                .map(|mapping| {
                    if mapping.event_wildcard {
                        return quote! { _ };
                    }
                    let value = &mapping.event;

                    match sm.event_data.data_types.get(&mapping.event.to_string()) {
                        None => {
                            quote! {
                                #value
                            }
                        }
                        Some(_) => {
                            quote! {
                                #value(event_data)
                            }
                        }
                    }
                })
                .collect()
        })
        .collect();

    // Map guards, actions and output states into code blocks
    let guards: Vec<Vec<_>> = transitions
        .values()
        .map(|event_mappings| {
            event_mappings
                .values()
                .map(|event_mapping| {
                    event_mapping
                        .transitions
                        .iter()
                        .map(|transition| transition.guard.clone())
                        .collect::<Vec<_>>()
                })
                .collect::<Vec<_>>()
        })
        .collect();

    let actions: Vec<Vec<_>> = transitions
        .values()
        .map(|event_mappings| {
            event_mappings
                .values()
                .map(|event_mapping| {
                    event_mapping
                        .transitions
                        .iter()
                        .map(|transition| {
                            transition
                                .action
                                .iter()
                                .cloned()
                                .chain(transition.additional_actions.iter().cloned())
                                .collect::<Vec<_>>()
                        })
                        .collect::<Vec<_>>()
                })
                .collect::<Vec<_>>()
        })
        .collect();

    let process_events: Vec<Vec<_>> = transitions
        .values()
        .map(|event_mappings| {
            event_mappings
                .values()
                .map(|event_mapping| {
                    event_mapping
                        .transitions
                        .iter()
                        .map(|transition| transition.process_events.clone())
                        .collect::<Vec<_>>()
                })
                .collect::<Vec<_>>()
        })
        .collect();

    let deferred_events: Vec<Vec<_>> = transitions
        .values()
        .map(|event_mappings| {
            event_mappings
                .values()
                .map(|event_mapping| {
                    let event = &event_mapping.event;
                    event_mapping
                        .transitions
                        .iter()
                        .map(|transition| {
                            if !transition.defer {
                                return TokenStream::new();
                            }
                            if sm
                                .event_data
                                .data_types
                                .contains_key(&event_mapping.event.to_string())
                            {
                                quote! { #events_type_name::#event(event_data) }
                            } else {
                                quote! { #events_type_name::#event }
                            }
                        })
                        .collect::<Vec<_>>()
                })
                .collect::<Vec<_>>()
        })
        .collect();

    let eval_actions: Vec<Vec<_>> = transitions
        .values()
        .map(|event_mappings| {
            event_mappings
                .values()
                .map(|event_mapping| {
                    event_mapping
                        .transitions
                        .iter()
                        .map(|transition| transition.eval_actions.clone())
                        .collect::<Vec<_>>()
                })
                .collect::<Vec<_>>()
        })
        .collect();

    let event_kinds: Vec<Vec<_>> = transitions
        .values()
        .map(|event_mappings| {
            event_mappings
                .values()
                .map(|event_mapping| event_mapping.event_kind)
                .collect()
        })
        .collect();

    let action_parameters: Vec<Vec<_>> = transitions
        .iter()
        .map(|(name, value)| {
            let state_name = &sm.states.get(name).unwrap().to_string();

            value
                .values()
                .map(|mapping| {
                    let state_data = match sm.state_data.data_types.get(state_name) {
                        Some(Type::Reference(_)) => quote! { state_data },
                        Some(_) => quote! { &state_data },
                        None => quote! {},
                    };

                    let event_data =
                        if mapping.event_kind == EventKind::Exception && !mapping.event_wildcard {
                            quote! { error_data }
                        } else {
                            match sm.event_data.data_types.get(&mapping.event.to_string()) {
                                Some(Type::Reference(_)) if mapping.event_external => {
                                    quote! { event_data }
                                }
                                Some(_)
                                    if mapping.event_external
                                        && mapping.event_kind == EventKind::Completion =>
                                {
                                    quote! { event_data }
                                }
                                Some(_) if mapping.event_external => quote! { &event_data },
                                Some(_) if mapping.event_kind == EventKind::Completion => {
                                    quote! { core::clone::Clone::clone(event_data) }
                                }
                                Some(_) => quote! { event_data },
                                None => quote! {},
                            }
                        };

                    if state_data.is_empty() || event_data.is_empty() {
                        quote! { #state_data #event_data }
                    } else {
                        quote! { #state_data, #event_data }
                    }
                })
                .collect()
        })
        .collect();

    let guard_parameters: Vec<Vec<_>> = transitions
        .iter()
        .map(|(name, value)| {
            let state_name = &sm.states.get(name).unwrap().to_string();

            value
                .values()
                .map(|mapping| {
                    let state_data = match sm.state_data.data_types.get(state_name) {
                        Some(Type::Reference(_)) => quote! { state_data },
                        Some(_) => quote! { &state_data },
                        None => quote! {},
                    };

                    let event_data = match sm.event_data.data_types.get(&mapping.event.to_string())
                    {
                        Some(Type::Reference(_)) if mapping.event_kind == EventKind::Completion => {
                            quote! { *event_data }
                        }
                        Some(_) if mapping.event_kind == EventKind::Completion => {
                            quote! { event_data }
                        }
                        Some(Type::Reference(_)) => quote! { event_data },
                        Some(_) => quote! { &event_data },
                        None => quote! {},
                    };

                    if state_data.is_empty() || event_data.is_empty() {
                        quote! { #state_data #event_data }
                    } else {
                        quote! { #state_data, #event_data }
                    }
                })
                .collect()
        })
        .collect();

    let custom_error = if let Some(error_type) = &sm.fixed_error_type {
        quote! { #error_type }
    } else if sm.custom_error {
        quote! { Self::Error }
    } else {
        quote! { () }
    };

    let out_states: Vec<Vec<Vec<TokenStream>>> = transitions
        .values()
        .map(|event_mappings| {
            event_mappings
                .values()
                .map(|event_mapping| {
                    event_mapping
                        .transitions
                        .iter()
                        .map(|transition| transition.out_state.clone())
                        .map(|out_state| {
                            match sm.state_data.data_types.get(&out_state.to_string()) {
                                None => {
                                    quote! {
                                        #out_state
                                    }
                                }
                                Some(_) => {
                                    quote! {
                                        #out_state(_data)
                                    }
                                }
                            }
                        })
                        .collect::<Vec<_>>()
                })
                .collect::<Vec<_>>()
        })
        .collect();
    let internal_transitions: Vec<Vec<Vec<bool>>> = transitions
        .values()
        .map(|event_mappings| {
            event_mappings
                .values()
                .map(|event_mapping| {
                    event_mapping
                        .transitions
                        .iter()
                        .map(|transition| transition.internal_transition)
                        .collect()
                })
                .collect()
        })
        .collect();
    let default_outputs: Vec<Vec<Vec<bool>>> = transitions
        .values()
        .map(|event_mappings| {
            event_mappings
                .values()
                .map(|event_mapping| {
                    event_mapping
                        .transitions
                        .iter()
                        .map(|transition| transition.default_output)
                        .collect()
                })
                .collect()
        })
        .collect();

    let temporary_context = match &sm.temporary_context_type {
        Some(tct) => {
            quote! { temporary_context: #tct, }
        }
        None => {
            quote! {}
        }
    };

    // Keep track of already added actions not to duplicate definitions
    let mut action_set: Vec<syn::Ident> = Vec::new();
    let mut guard_set: Vec<syn::Ident> = Vec::new();

    let mut guard_list = proc_macro2::TokenStream::new();
    let mut action_list = proc_macro2::TokenStream::new();

    let mut entries_exits = proc_macro2::TokenStream::new();

    for (state, event_mappings) in transitions.iter() {
        // create the state data token stream
        let state_data = match sm.state_data.data_types.get(state) {
            Some(st @ Type::Reference(_)) => quote! { state_data: #st, },
            Some(st) => quote! { state_data: &#st, },
            None => quote! {},
        };

        let entry_ident = format_ident!("on_entry_{}", string_morph::to_snake_case(state));
        let state_name = format!("[{}::{}]", states_type_name, state);
        let entry_exit_async = if sm.entry_exit_async {
            quote! { async }
        } else {
            quote! {}
        };
        entries_exits.extend(quote! {
            #[doc = concat!("Called on entry to ", #state_name)]
            #[inline(always)]
            #entry_exit_async fn #entry_ident(&mut self) {}
        });
        let exit_ident = format_ident!("on_exit_{}", string_morph::to_snake_case(state));
        entries_exits.extend(quote! {
            #[doc = concat!("Called on exit from ", #state_name)]
            #[inline(always)]
            #entry_exit_async fn #exit_ident(&mut self) {}
        });

        for event_mapping in event_mappings.values() {
            let event = event_mapping.event.to_string();
            for transition in &event_mapping.transitions {
                // get input state lifetimes
                let in_state_lifetimes = sm
                    .state_data
                    .lifetimes
                    .get(&event_mapping.in_state.to_string())
                    .cloned()
                    .unwrap_or_default();

                // get output state lifetimes
                let out_state_lifetimes = sm
                    .state_data
                    .lifetimes
                    .get(&transition.out_state.to_string())
                    .cloned()
                    .unwrap_or_default();

                // get event lifetimes
                let event_lifetimes = sm
                    .event_data
                    .lifetimes
                    .get(&event)
                    .cloned()
                    .unwrap_or_default();

                // combine all lifetimes
                let mut all_lifetimes = Lifetimes::new();
                all_lifetimes.extend(&in_state_lifetimes);
                all_lifetimes.extend(&out_state_lifetimes);
                all_lifetimes.extend(&event_lifetimes);
                let callback_generics = sm.callback_generics_with_lifetimes(
                    &all_lifetimes,
                    sm.event_data.data_types.get(&event),
                );
                let (callback_impl_generics, _, callback_where_clause) =
                    callback_generics.split_for_impl();

                // Create the guard traits for user implementation
                if let Some(guard_expression) = &transition.guard {
                    visit_guards(guard_expression,|guard| {
                        let is_async = guard.is_async;
                        let guard = &guard.ident;
                        let event_data = match sm.event_data.data_types.get(&event) {
                            Some(et @ Type::Reference(_)) => quote! { event_data: #et },
                            Some(et) => quote! { event_data: &#et },
                            None => quote! {},
                        };

                        // Only add the guard if it hasn't been added before
                        if !guard_set.iter().any(|g| g == guard) {
                            guard_set.push(guard.clone());
                            let is_async = if is_async { quote!{ async } } else { quote!{ } };
                            guard_list.extend(quote! {
                            #[allow(missing_docs)]
                            #[allow(clippy::result_unit_err)]
                            #is_async fn #guard #callback_impl_generics (&self, #temporary_context #state_data #event_data) -> Result<bool,#custom_error> #callback_where_clause;
                        });
                        };
                        Ok(())
                    }).unwrap();
                }
                for eval in &transition.eval_actions {
                    visit_guards(&eval.guard, |guard| {
                        let is_async = guard.is_async;
                        let guard = &guard.ident;
                        let event_data = match sm.event_data.data_types.get(&event) {
                            Some(et @ Type::Reference(_)) => quote! { event_data: #et },
                            Some(et) => quote! { event_data: &#et },
                            None => quote! {},
                        };
                        if !guard_set.iter().any(|known| known == guard) {
                            guard_set.push(guard.clone());
                            let is_async = if is_async {
                                quote! { async }
                            } else {
                                quote! {}
                            };
                            guard_list.extend(quote! {
                                #[allow(missing_docs)]
                                #[allow(clippy::result_unit_err)]
                                #is_async fn #guard #callback_impl_generics(
                                    &self,
                                    #temporary_context #state_data #event_data
                                ) -> Result<bool, #custom_error> #callback_where_clause;
                            });
                        }
                        Ok(())
                    })
                    .unwrap();
                }

                // Create the action traits for user implementation
                let transition_actions = transition
                    .action
                    .iter()
                    .chain(transition.additional_actions.iter())
                    .collect::<Vec<_>>();
                let last_action_index = transition_actions.len().saturating_sub(1);
                for (
                    action_index,
                    AsyncIdent {
                        ident: action,
                        is_async,
                    },
                ) in transition_actions.into_iter().enumerate()
                {
                    let is_async = if *is_async {
                        quote! { async }
                    } else {
                        quote! {}
                    };
                    let return_type =
                        if matches!(event_mapping.event_kind, EventKind::Entry | EventKind::Exit) {
                            quote! { Result<(),#custom_error> }
                        } else if action_index == last_action_index {
                            if let Some(output_data) = sm
                                .state_data
                                .data_types
                                .get(&transition.out_state.to_string())
                            {
                                quote! { Result<#output_data,#custom_error> }
                            } else {
                                quote! { Result<(),#custom_error> }
                            }
                        } else {
                            quote! { Result<(),#custom_error> }
                        };

                    let event_data = if event_mapping.event_kind == EventKind::Exception
                        && !event_mapping.event_wildcard
                    {
                        let error_type =
                            sm.fixed_error_type.as_ref().expect("typed exception error");
                        quote! { error_data: &#error_type }
                    } else {
                        match sm.event_data.data_types.get(&event) {
                            Some(et @ Type::Reference(_)) if event_mapping.event_external => {
                                quote! { event_data: #et }
                            }
                            Some(et) if event_mapping.event_external => {
                                quote! { event_data: &#et }
                            }
                            Some(et) => {
                                quote! { event_data: #et }
                            }
                            None => quote! {},
                        }
                    };

                    // Only add the action if it hasn't been added before
                    if !action_set.iter().any(|a| a == action) {
                        action_set.push(action.clone());
                        action_list.extend(quote! {
                            #[allow(missing_docs)]
                            #[allow(clippy::unused_unit)]
                            #is_async fn #action #callback_impl_generics (&mut self, #temporary_context #state_data #event_data) -> #return_type #callback_where_clause;
                        });
                    }
                }
                for eval in &transition.eval_actions {
                    let action = &eval.action.ident;
                    if action_set.iter().any(|known| known == action) {
                        continue;
                    }
                    action_set.push(action.clone());
                    let is_async = if eval.action.is_async {
                        quote! { async }
                    } else {
                        quote! {}
                    };
                    let event_data = match sm.event_data.data_types.get(&event) {
                        Some(et @ Type::Reference(_)) if event_mapping.event_external => {
                            quote! { event_data: #et }
                        }
                        Some(et) if event_mapping.event_external => quote! { event_data: &#et },
                        Some(et) => quote! { event_data: #et },
                        None => quote! {},
                    };
                    action_list.extend(quote! {
                        #[allow(missing_docs)]
                        #is_async fn #action #callback_impl_generics(
                            &mut self,
                            #temporary_context #state_data #event_data
                        ) -> Result<(), #custom_error> #callback_where_clause;
                    });
                }
            }
        }
    }

    let temporary_context_call = match &sm.temporary_context_type {
        Some(_) => {
            quote! { temporary_context, }
        }
        None => {
            quote! {}
        }
    };
    let completion_context_call = match &sm.temporary_context_type {
        Some(Type::Reference(reference)) if reference.mutability.is_some() => {
            quote! { &mut *temporary_context, }
        }
        Some(_) => quote! { temporary_context, },
        None => quote! {},
    };

    let mut entry_actions = std::collections::HashMap::new();
    let mut exit_actions = std::collections::HashMap::new();
    for (state, event_mappings) in transitions {
        for mapping in event_mappings.values() {
            if !matches!(mapping.event_kind, EventKind::Entry | EventKind::Exit) {
                continue;
            }
            if let Some(transition) = mapping.transitions.first() {
                let mut call = TokenStream::new();
                for action in transition
                    .action
                    .iter()
                    .chain(transition.additional_actions.iter())
                {
                    let action_ident = &action.ident;
                    let action_await = if action.is_async {
                        quote! { .await }
                    } else {
                        quote! {}
                    };
                    call.extend(quote! {
                        self.context.#action_ident(#temporary_context_call)
                            #action_await
                            .map_err(#error_type_name::ActionFailed)?;
                        self.context.log_action(stringify!(#action_ident));
                    });
                }
                if mapping.event_kind == EventKind::Entry {
                    entry_actions.insert(state.clone(), call);
                } else {
                    exit_actions.insert(state.clone(), call);
                }
            }
        }
    }

    let mut is_async_state_machine = sm.entry_exit_async
        || actions
            .iter()
            .flatten()
            .flatten()
            .flatten()
            .any(|action| action.is_async);
    for expression in guards.iter().flatten().flatten().flatten() {
        visit_guards(expression, |guard| {
            is_async_state_machine |= guard.is_async;
            Ok(())
        })
        .unwrap();
    }
    for eval in eval_actions.iter().flatten().flatten().flatten() {
        is_async_state_machine |= eval.action.is_async;
        visit_guards(&eval.guard, |guard| {
            is_async_state_machine |= guard.is_async;
            Ok(())
        })
        .unwrap();
    }
    let has_queue_actions = has_deferred_events
        || process_events
            .iter()
            .flatten()
            .flatten()
            .any(|events| !events.is_empty());
    let async_queue = is_async_state_machine && has_queue_actions;

    let entry_exit_await = if sm.entry_exit_async {
        quote! { .await }
    } else {
        quote! {}
    };
    let completion_await = if is_async_state_machine {
        quote! { .await }
    } else {
        quote! {}
    };

    // Create the code blocks inside the switch cases
    let code_blocks: Vec<Vec<_>> = guards
        .iter()
        .zip(event_kinds.iter())
        .zip(internal_transitions.iter())
        .zip(process_events.iter())
        .zip(deferred_events.iter())
        .zip(eval_actions.iter())
        .zip(default_outputs.iter())
        .zip(
            actions
                .iter()
                .zip(in_states.iter().zip(out_states.iter().zip(action_parameters.iter().zip(guard_parameters.iter())))),
        )
        .map(
            |(((((((guards, event_kinds), internal_transitions), process_events), deferred_events), eval_actions), default_outputs), (actions, (in_state, (out_states, (action_parameters, guard_parameters)))))| {
                guards
                    .iter()
                    .zip(event_kinds.iter())
                    .zip(internal_transitions.iter())
                    .zip(process_events.iter())
                    .zip(deferred_events.iter())
                    .zip(eval_actions.iter())
                    .zip(default_outputs.iter())
                    .zip(
                        actions
                            .iter()
                            .zip(out_states.iter().zip(action_parameters.iter().zip(guard_parameters.iter()))),
                    )
                    .map(|(((((((guard, event_kind), internal_transitions), process_events), deferred_events), eval_actions), default_outputs), (action, (out_state, (action_params, guard_params))))| {
                        let streams: Vec<TokenStream> =
                            guard.iter().zip(
                                action.iter().zip(out_state).zip(internal_transitions.iter()).zip(process_events.iter()).zip(deferred_events.iter()).zip(eval_actions.iter()).zip(default_outputs.iter())
                            ).map(|(guard, ((((((action,out_state), internal_transition), process_events), deferred_event), eval_actions), default_output))| {
                                let binding = out_state.to_string();
                                let out_state_string = binding.split('(').next().unwrap().trim();
                                let binding = in_state.to_string();
                                let in_state_string = binding.split('(').next().unwrap().trim();

                                let entry_ident = format_ident!("on_entry_{}", string_morph::to_snake_case(out_state_string));
                                let exit_ident = format_ident!("on_exit_{}", string_morph::to_snake_case(in_state_string));
                                let entry_action = entry_actions
                                    .get(out_state_string)
                                    .cloned()
                                    .unwrap_or_default();
                                let exit_action = exit_actions
                                    .get(in_state_string)
                                    .cloned()
                                    .unwrap_or_default();

                                let produces_state_data = out_state.to_string().contains("(_data)");
                                let (is_async_action, action_code) = generate_actions(
                                    action,
                                    &temporary_context_call,
                                    action_params,
                                    &error_type_name,
                                    produces_state_data,
                                    eval_actions,
                                    guard_params,
                                );
                                is_async_state_machine |= is_async_action;
                                let process_await = if is_async_state_machine { quote! { .await } } else { quote! {} };
                                let process_code = process_events.iter().map(|event| {
                                    if async_queue {
                                        quote! {
                                            self.pending.defer((#event).into())
                                                .map_err(|_| #error_type_name::QueueFull)?;
                                        }
                                    } else {
                                        quote! { let _ = self.process_event(#event)#process_await?; }
                                    }
                                });
                                let defer_code = if deferred_event.is_empty() {
                                    quote! {}
                                } else {
                                    quote! {
                                        self.deferred.defer(#deferred_event)
                                            .map_err(|_| #error_type_name::QueueFull)?;
                                    }
                                };
                                let default_output_code = if *default_output
                                    && action.is_empty()
                                    && out_state.to_string().contains("(_data)")
                                {
                                    quote! { let _data = core::default::Default::default(); }
                                } else {
                                    quote! {}
                                };
                                let drain_deferred = if has_deferred_events {
                                    if async_queue {
                                        quote! {
                                            while let Some(deferred_event) = self.deferred.pop() {
                                                self.pending.defer(deferred_event)
                                                    .map_err(|_| #error_type_name::QueueFull)?;
                                            }
                                        }
                                    } else {
                                        quote! {
                                            while let Some(deferred_event) = self.deferred.pop() {
                                                let _ = self.process_event(deferred_event)#process_await;
                                            }
                                        }
                                    }
                                } else {
                                    quote! {}
                                };

                                let finish_transition = if *event_kind == EventKind::Completion {
                                    quote! { return Ok(true); }
                                } else if *event_kind == EventKind::Exception {
                                    quote! { return Ok(&self.state); }
                                } else if has_completion_events {
                                    quote! {
                                        if let Some(ref completion_origin) = completion_origin {
                                            while self.process_completion(#completion_context_call completion_origin)#completion_await? {}
                                        }
                                        return Ok(&self.state);
                                    }
                                } else {
                                    quote! { return Ok(&self.state); }
                                };

                                let transition = if *internal_transition {
                                    quote!{
                                            #action_code
                                            #default_output_code
                                            #defer_code
                                            #(#process_code)*
                                            #finish_transition
                                        }
                                } else {
                                    quote!{
                                            self.context.#exit_ident()#entry_exit_await;
                                            #exit_action
                                            #action_code
                                            #default_output_code
                                            let out_state = #states_type_name::#out_state;
                                            self.context().transition_callback(&self.state, &out_state);
                                            self.state = out_state;
                                            self.context.#entry_ident()#entry_exit_await;
                                            #entry_action
                                            #defer_code
                                            #(#process_code)*
                                            #drain_deferred
                                            #finish_transition
                                        }
                                };
                                if let Some(expr) = guard { // Guarded transition
                                    let guard_expression= expr.to_token_stream(&mut |async_ident: &AsyncIdent| {
                                        let guard_ident = &async_ident.ident;
                                        let guard_await = if async_ident.is_async {
                                            is_async_state_machine = true;
                                            quote! { .await }
                                        } else {
                                            quote! {}
                                        };
                                        quote! {
                                            {
                                                let guard_result = self.context.#guard_ident(#temporary_context_call #guard_params)
                                                    #guard_await.map_err(#error_type_name::GuardFailed)?;
                                                self.context.log_guard(stringify!(#guard_ident), guard_result);
                                                guard_result
                                            }
                                        }
                                    });
                                    quote! {
                                        // This #guard_expression contains a boolean expression of guard functions
                                        // Each guard function has Result<bool,_> return type.
                                        // For example, [ f && !g ] will expand into
                                        //  self.context.f()? && !self.context.g()?
                                        let guard_passed = #guard_expression;

                                        // If the guard passed, we transition immediately.
                                        // Otherwise, there may be a later transition that passes,
                                        // so we'll defer to that.
                                        if guard_passed {
                                            #transition
                                        }
                                    }
                                } else { // Unguarded transition
                                   quote!{
                                        #transition
                                   }
                                }
                            }
                            ).collect();
                        quote!{
                            #(#streams)*
                        }
                    })
                    .collect()
            },
        )
        .collect();

    let state_dispatch_arms: Vec<TokenStream> = transitions
        .values()
        .zip(in_states.iter().zip(events.iter().zip(code_blocks.iter())))
        .map(|(event_mappings, (in_state, (events, code_blocks)))| {
            let mut normal_arms = Vec::new();
            let mut unexpected_arms = Vec::new();
            let mut unexpected_wildcard = None;

            for ((mapping, event), code_block) in event_mappings
                .values()
                .zip(events.iter())
                .zip(code_blocks.iter())
            {
                let event_pattern = if mapping.event_wildcard {
                    quote! { _ }
                } else {
                    quote! { #events_type_name::#event }
                };
                let arm = quote! {
                    #event_pattern => {
                        #code_block

                        #[allow(unreachable_code)]
                        {
                            Err(#error_type_name::TransitionsFailed)
                        }
                    }
                };

                match mapping.event_kind {
                    EventKind::Normal => normal_arms.push(arm),
                    EventKind::Unexpected if mapping.event_wildcard => {
                        unexpected_wildcard = Some(arm)
                    }
                    EventKind::Unexpected => unexpected_arms.push(arm),
                    EventKind::Completion => {}
                    EventKind::Entry | EventKind::Exit => {}
                    EventKind::Exception => {}
                }
            }

            let fallback = if unexpected_arms.is_empty() && unexpected_wildcard.is_none() {
                quote! {
                    #[allow(unreachable_patterns)]
                    _ => Err(#error_type_name::InvalidEvent)
                }
            } else {
                quote! {
                    #[allow(unreachable_patterns)]
                    unhandled_event => match unhandled_event {
                        #(#unexpected_arms,)*
                        #unexpected_wildcard
                        #[allow(unreachable_patterns)]
                        _ => Err(#error_type_name::InvalidEvent),
                    }
                }
            };

            quote! {
                #[allow(clippy::match_single_binding)]
                #states_type_name::#in_state => match event {
                    #(#normal_arms,)*
                    #fallback
                }
            }
        })
        .collect();

    let exception_state_arms: Vec<TokenStream> = transitions
        .values()
        .zip(in_states.iter().zip(code_blocks.iter()))
        .filter_map(|(event_mappings, (in_state, code_blocks))| {
            let mappings = event_mappings
                .values()
                .zip(code_blocks.iter())
                .collect::<Vec<_>>();
            mappings
                .iter()
                .find(|(mapping, _)| {
                    mapping.event_kind == EventKind::Exception && !mapping.event_wildcard
                })
                .or_else(|| {
                    mappings
                        .iter()
                        .find(|(mapping, _)| mapping.event_kind == EventKind::Exception)
                })
                .map(|(_, code_block)| {
                    quote! {
                        #states_type_name::#in_state => {
                            #code_block
                            #[allow(unreachable_code)]
                            Err(error)
                        }
                    }
                })
        })
        .collect();

    let completion_state_arms: Vec<TokenStream> = transitions
        .values()
        .zip(in_states.iter().zip(events.iter().zip(code_blocks.iter())))
        .map(|(event_mappings, (in_state, (events, code_blocks)))| {
            let mut completion_arms = Vec::new();
            let mut anonymous_completion_arm = None;
            for (mapping, (event, code_block)) in event_mappings
                .values()
                .zip(events.iter().zip(code_blocks.iter()))
                .filter(|(mapping, _)| mapping.event_kind == EventKind::Completion)
            {
                let completion_pattern = if mapping.event_wildcard {
                    quote! { _ }
                } else {
                    quote! { #completion_origin_type_name::#event }
                };
                let arm = quote! {
                    #completion_pattern => {
                        #code_block

                        #[allow(unreachable_code)]
                        {
                            Ok(false)
                        }
                    }
                };
                if mapping.event_wildcard {
                    anonymous_completion_arm = Some(arm);
                } else {
                    completion_arms.push(arm);
                }
            }

            quote! {
                #states_type_name::#in_state => match origin {
                    #(#completion_arms,)*
                    #anonymous_completion_arm
                    #[allow(unreachable_patterns)]
                    _ => Ok(false),
                }
            }
        })
        .collect();

    let starting_state = &sm.starting_state;
    let initial_entry_arms: Vec<_> = sm
        .states
        .iter()
        .map(|(state_name, state)| {
            let entry_ident = format_ident!("on_entry_{}", string_morph::to_snake_case(state_name));
            let pattern = if sm.state_data.data_types.contains_key(state_name) {
                quote! { #states_type_name::#state(..) }
            } else {
                quote! { #states_type_name::#state }
            };
            let entry_action = entry_actions.get(state_name).cloned().unwrap_or_default();
            quote! {
                #pattern => {
                    self.context.#entry_ident()#entry_exit_await;
                    #entry_action
                }
            }
        })
        .collect();
    let is_terminated = match sm.states.get("X") {
        Some(terminal) if sm.state_data.data_types.contains_key("X") => quote! {
            matches!(self.state, #states_type_name::#terminal(..))
        },
        Some(terminal) => quote! {
            matches!(self.state, #states_type_name::#terminal)
        },
        None => quote! { false },
    };

    // create a token stream for creating a new machine.  If the starting state contains data, then
    // add a second argument to pass this initial data
    let starting_state_name = starting_state.to_string();
    let deferred_init = has_deferred_events.then(|| {
        quote! { deferred: ::sml::utility::EventQueue::new(), }
    });
    let pending_init = async_queue.then(|| quote! { pending: ::sml::utility::EventQueue::new(), });
    let new_sm_code = match sm.state_data.data_types.get(&starting_state_name) {
        Some(st) if type_matches_state(st, &starting_state_name) => quote! {
            pub fn new(context: #context_type_ident) -> Self
            where
                #st: core::default::Default,
            {
                #state_machine_type_name {
                    state: #states_type_name::#starting_state(core::default::Default::default()),
                    context,
                    #deferred_init
                    #pending_init
                }
            }

            pub const fn new_with_state_data(context: #context_type_ident, state_data: #st) -> Self {
                #state_machine_type_name {
                    state: #states_type_name::#starting_state(state_data),
                    context,
                    #deferred_init
                    #pending_init
                }
            }
        },
        Some(st) => quote! {
            pub const fn new(context: #context_type_ident, state_data: #st ) -> Self {
                #state_machine_type_name {
                    state: #states_type_name::#starting_state (state_data),
                    context,
                    #deferred_init
                    #pending_init
                }
            }
        },
        None => quote! {
            pub const fn new(context: #context_type_ident ) -> Self {
                #state_machine_type_name {
                    state: #states_type_name::#starting_state,
                    context,
                    #deferred_init
                    #pending_init
                }
            }
        },
    };

    let state_lifetimes = &sm.state_data.all_lifetimes;
    let event_lifetimes = &sm.event_data.all_lifetimes;

    // lifetimes that exists in #events_type_name but not in #states_type_name
    let event_unique_lifetimes = event_lifetimes - state_lifetimes;
    let dispatch_generics = sm.event_generics_with_lifetimes(&event_unique_lifetimes);
    let (dispatch_impl_generics, _, dispatch_where_clause) = dispatch_generics.split_for_impl();
    let mut process_event_generics = dispatch_generics.clone();
    process_event_generics
        .params
        .push(parse_quote!(#event_input_ident));
    process_event_generics
        .make_where_clause()
        .predicates
        .push(parse_quote!(
            #event_input_ident: Into<#events_type_name #event_type_generics>
        ));
    let (process_event_impl_generics, _, process_event_where_clause) =
        process_event_generics.split_for_impl();
    let mut machine_lifetimes = state_lifetimes.clone();
    machine_lifetimes.extend(event_lifetimes);
    let mut machine_impl_generics = sm.event_generics_with_lifetimes(&machine_lifetimes);
    machine_impl_generics.params.push(parse_quote!(
        #context_type_ident: #state_machine_context_type_name
    ));
    let (machine_impl_generics, _, machine_impl_where_clause) =
        machine_impl_generics.split_for_impl();
    let external_event_conversions: Vec<_> = external_events
        .iter()
        .map(|event| {
            let event_type = sm
                .event_data
                .data_types
                .get(&event.to_string())
                .expect("external events carry their concrete type");
            quote! {
                impl #event_impl_generics From<#event_type>
                    for #events_type_name #event_type_generics
                    #event_where_clause
                {
                    #[inline(always)]
                    fn from(event: #event_type) -> Self {
                        Self::#event(event)
                    }
                }
            }
        })
        .collect();

    let mut completion_lifetimes = Lifetimes::new();
    let completion_origin_variants: Vec<_> = completion_events
        .iter()
        .map(|event| {
            let event_name = event.to_string();
            if let Some(lifetimes) = sm.event_data.lifetimes.get(&event_name) {
                completion_lifetimes.extend(lifetimes);
            }
            match sm.event_data.data_types.get(&event_name) {
                Some(data_type) => quote! { #event(#data_type) },
                None => quote! { #event },
            }
        })
        .collect();
    let completion_generic_type = completion_events.iter().find_map(|event| {
        sm.event_data
            .data_types
            .get(&event.to_string())
            .filter(|event_type| sm.type_uses_event_generics(event_type))
    });
    let completion_generics =
        sm.completion_generics_with_lifetimes(&completion_lifetimes, completion_generic_type);
    let (completion_impl_generics, completion_type_generics, completion_where_clause) =
        completion_generics.split_for_impl();
    let completion_origin_clone_arms: Vec<_> = completion_events
        .iter()
        .map(|event| {
            let event_name = event.to_string();
            match sm.event_data.data_types.get(&event_name) {
                Some(_) => quote! {
                    #events_type_name::#event(event_data) => {
                        Some(#completion_origin_type_name::#event(
                            core::clone::Clone::clone(event_data)
                        ))
                    }
                },
                None => quote! {
                    #events_type_name::#event => {
                        Some(#completion_origin_type_name::#event)
                    }
                },
            }
        })
        .collect();
    let anonymous_completion_variant = if has_anonymous_completion {
        quote! { __AnyCompletion, }
    } else {
        quote! {}
    };
    let unmatched_completion_origin = if has_anonymous_completion {
        quote! { Some(#completion_origin_type_name::__AnyCompletion) }
    } else {
        quote! { None }
    };
    let completion_origin_definition = if has_completion_events {
        quote! {
            enum #completion_origin_type_name #completion_impl_generics
                #completion_where_clause
            {
                #anonymous_completion_variant
                #(#completion_origin_variants),*
            }
        }
    } else {
        quote! {}
    };
    let capture_completion_origin = if has_completion_events {
        quote! {
            let completion_origin = match &event {
                #(#completion_origin_clone_arms,)*
                _ => #unmatched_completion_origin,
            };
        }
    } else {
        quote! {}
    };

    let custom_error = if sm.custom_error && sm.fixed_error_type.is_none() {
        quote! {
            /// The error type returned by guard or action functions.
            type Error: core::fmt::Debug;
        }
    } else {
        quote! {}
    };

    let is_async = if is_async_state_machine {
        quote! { async }
    } else {
        quote! {}
    };

    let error_type = if let Some(fixed_error_type) = &sm.fixed_error_type {
        quote! { #error_type_name<#fixed_error_type> }
    } else if sm.custom_error {
        quote! { #error_type_name<<#context_type_ident as #state_machine_context_type_name>::Error> }
    } else {
        quote! {#error_type_name}
    };

    let process_completion = if has_completion_events {
        quote! {
            #[inline]
            #is_async fn process_completion #completion_impl_generics(
                &mut self,
                #temporary_context
                origin: &#completion_origin_type_name #completion_type_generics
            ) -> Result<bool, #error_type>
                #completion_where_clause
            {
                match self.state {
                    #(#completion_state_arms),*
                }
            }
        }
    } else {
        quote! {}
    };
    let exception_error_binding = sm.fixed_error_type.as_ref().map(|_| {
        quote! {
            let error_data = match &error {
                #error_type_name::GuardFailed(error_data)
                | #error_type_name::ActionFailed(error_data) => error_data,
                _ => return Err(error),
            };
        }
    });
    let process_exception = if has_exception_handlers {
        quote! {
            #is_async fn process_exception(
                &mut self,
                #temporary_context
                error: #error_type,
            ) -> Result<&#states_type_name<#state_lifetimes>, #error_type> {
                #exception_error_binding
                match self.state {
                    #(#exception_state_arms,)*
                    _ => Err(error),
                }
            }
        }
    } else {
        quote! {}
    };
    let event_dispatch = if has_exception_handlers {
        if is_async_state_machine {
            quote! {
                let dispatch_result: Result<(), #error_type> =
                    async { match self.state { #(#state_dispatch_arms),* } }
                        .await
                        .map(|_| ());
                match dispatch_result {
                    Err(error @ (#error_type_name::GuardFailed(_) | #error_type_name::ActionFailed(_))) => {
                        self.process_exception(#completion_context_call error).await
                    }
                    Ok(()) => Ok(&self.state),
                    Err(error) => Err(error),
                }
            }
        } else {
            quote! {
                let dispatch_result: Result<(), #error_type> = (|| -> Result<&#states_type_name<#state_lifetimes>, #error_type> {
                    match self.state { #(#state_dispatch_arms),* }
                })().map(|_| ());
                match dispatch_result {
                    Err(error @ (#error_type_name::GuardFailed(_) | #error_type_name::ActionFailed(_))) => {
                        self.process_exception(#completion_context_call error)
                    }
                    Ok(()) => Ok(&self.state),
                    Err(error) => Err(error),
                }
            }
        }
    } else {
        quote! { match self.state { #(#state_dispatch_arms),* } }
    };
    let queued_dispatch = async_queue.then(|| {
        quote! {
            async fn dispatch_queued #dispatch_impl_generics(
                &mut self,
                #temporary_context
                event: #events_type_name #event_type_generics,
            ) -> Result<&#states_type_name<#state_lifetimes>, #error_type>
                #dispatch_where_clause
            {
                #capture_completion_origin
                self.context.log_process_event(self.state(), &event);
                #event_dispatch
            }
        }
    });
    let public_dispatch = if async_queue {
        quote! {
            self.pending.defer(event.into())
                .map_err(|_| #error_type_name::QueueFull)?;
            while let Some(event) = self.pending.pop() {
                let _ = self.dispatch_queued(#completion_context_call event).await?;
            }
            Ok(&self.state)
        }
    } else {
        quote! {
            let event: #events_type_name #event_type_generics = event.into();
            #capture_completion_origin
            self.context.log_process_event(self.state(), &event);
            #event_dispatch
        }
    };
    let initialize_context = if has_anonymous_completion {
        temporary_context.clone()
    } else {
        quote! {}
    };
    let initialize = {
        let stabilize = if has_anonymous_completion {
            quote! {
                let completion_origin =
                    #completion_origin_type_name::__AnyCompletion;
                while self.process_completion(
                    #completion_context_call
                    &completion_origin
                )#completion_await? {}
            }
        } else {
            quote! {}
        };
        quote! {
            /// Enters the initial state and runs anonymous `completion<_>`
            /// transitions until the machine reaches a stable state.
            pub #is_async fn initialize(
                &mut self,
                #initialize_context
            ) -> Result<&#states_type_name<#state_lifetimes>, #error_type> {
                match self.state {
                    #(#initial_entry_arms),*
                }
                #stabilize
                Ok(&self.state)
            }
        }
    };

    let machine_trait_impl = if !is_async_state_machine && sm.temporary_context_type.is_none() {
        quote! {
            impl #machine_impl_generics
                ::sml::Machine<#events_type_name #event_type_generics>
                for #state_machine_type_name<#state_lifetimes #context_type_ident>
                #machine_impl_where_clause
            {
                type State = #states_type_name<#state_lifetimes>;

                #[inline]
                fn process_event(
                    &mut self,
                    event: #events_type_name #event_type_generics
                ) -> bool {
                    self.process_event(event).is_ok()
                }
            }
        }
    } else {
        quote! {}
    };
    let terminated_trait_impl = quote! {
        impl<#state_lifetimes #context_type_ident: #state_machine_context_type_name>
            ::sml::Terminated for #state_machine_type_name<#state_lifetimes #context_type_ident>
        {
            #[inline(always)]
            fn is_terminated(&self) -> bool {
                self.is_terminated()
            }
        }
    };

    let states_attr_list = &sm.states_attr;
    let events_attr_list = &sm.events_attr;
    let deferred_field = has_deferred_events.then(|| {
        quote! { deferred: ::sml::utility::EventQueue<#events_type_name #event_type_generics, 16>, }
    });
    let pending_field = async_queue.then(|| {
        quote! { pending: ::sml::utility::EventQueue<#events_type_name #event_type_generics, 16>, }
    });
    // Build the states and events output
    quote! {
        /// This trait outlines the guards and actions that need to be implemented for the state
        /// machine.
        pub trait #state_machine_context_type_name {
            #custom_error
            #guard_list
            #action_list
            #entries_exits


            /// Called at the beginning of a state machine's `process_event()`. No-op by
            /// default but can be overridden in implementations of a state machine's
            /// `StateMachineContext` trait.
            fn log_process_event #event_impl_generics(
                &self,
                current_state: &#states_type_name,
                event: &#events_type_name #event_type_generics,
            ) #event_where_clause {}

            /// Called after executing a guard during `process_event()`. No-op by
            /// default but can be overridden in implementations of a state machine's
            /// `StateMachineContext` trait.
            fn log_guard(&self, guard: &'static str, result: bool) {}

            /// Called after executing an action during `process_event()`. No-op by
            /// default but can be overridden in implementations of a state machine's
            /// `StateMachineContext` trait.
            fn log_action(&self, action: &'static str) {}

            /// Called when transitioning to a new state as a result of an event passed to
            /// `process_event()`. No-op by default which can be overridden in implementations
            /// of a state machine's `StateMachineContext` trait.
            fn transition_callback(&self, old_state: & #states_type_name, new_state: & #states_type_name) {}
        }

        /// List of auto-generated states.
        #[allow(missing_docs)]
        #(#states_attr_list)*
        pub enum #states_type_name <#state_lifetimes> { #(#state_list),* }

        /// Manually define PartialEq for #states_type_name based on variant only to address issue-#21
        impl<#state_lifetimes> PartialEq for #states_type_name <#state_lifetimes> {
            fn eq(&self, other: &Self) -> bool {
                use core::mem::discriminant;
                discriminant(self) == discriminant(other)
            }
        }

        /// List of auto-generated events.
        #[allow(missing_docs)]
        #(#events_attr_list)*
        pub enum #events_type_name #event_impl_generics #event_where_clause {
            #(#event_list),*
        }

        #(#external_event_conversions)*

        #completion_origin_definition

        /// Manually define PartialEq for #events_type_name based on variant only to address issue-#21
        impl #event_impl_generics PartialEq for #events_type_name #event_type_generics
            #event_where_clause
        {
            fn eq(&self, other: &Self) -> bool {
                use core::mem::discriminant;
                discriminant(self) == discriminant(other)
            }
        }

        /// List of possible errors
        #[derive(Debug,PartialEq)]
        pub enum #error_type_name  <T=()> {
            /// When an event is processed which should not come in the current state.
            InvalidEvent,
            /// When an event is processed and none of the transitions happened.
            TransitionsFailed,
            /// When guard is failed.
            GuardFailed(T),
            /// When action returns Err
            ActionFailed(T),
            /// The generated bounded defer queue is full.
            QueueFull,
        }

        /// State machine structure definition.
        pub struct #state_machine_type_name<#state_lifetimes #context_type_ident: #state_machine_context_type_name> {
            state: #states_type_name <#state_lifetimes>,
            context: #context_type_ident,
            #deferred_field
            #pending_field
        }

        impl<#state_lifetimes #context_type_ident: #state_machine_context_type_name>
            #state_machine_type_name<#state_lifetimes #context_type_ident>
        {
            #process_completion
            #process_exception
            #initialize
            #queued_dispatch

            /// Creates a new state machine with the specified starting state.
            #[inline(always)]
            #new_sm_code

            /// Creates a new state machine with an initial state.
            #[inline(always)]
            pub const fn new_with_state(context: #context_type_ident, initial_state: #states_type_name <#state_lifetimes>) -> Self {
                #state_machine_type_name {
                    state: initial_state,
                    context,
                    #deferred_init
                    #pending_init
                }
            }

            /// Returns the current state.
            #[inline(always)]
            pub fn state(&self) -> &#states_type_name <#state_lifetimes> {
                &self.state
            }

            /// Returns true when the active state has the same variant as
            /// `expected`. State payloads are intentionally ignored, matching
            /// `sml.cpp`'s state-identity query.
            #[inline(always)]
            pub fn is(&self, expected: &#states_type_name <#state_lifetimes>) -> bool {
                self.state == *expected
            }

            /// Returns true when the machine is in its terminal `X` state.
            #[inline(always)]
            pub fn is_terminated(&self) -> bool {
                #is_terminated
            }

            /// Replaces the active state and returns the previous state.
            ///
            /// This is primarily intended for focused transition tests and
            /// state restoration.
            pub fn set_state(
                &mut self,
                state: #states_type_name <#state_lifetimes>
            ) -> #states_type_name <#state_lifetimes> {
                core::mem::replace(&mut self.state, state)
            }

            /// Invokes a visitor for the currently active state.
            #[inline(always)]
            pub fn visit_current_state<R>(
                &self,
                visitor: impl FnOnce(&#states_type_name <#state_lifetimes>) -> R
            ) -> R {
                visitor(&self.state)
            }

            /// Returns the current context.
            #[inline(always)]
            pub fn context(&self) -> &#context_type_ident {
                &self.context
            }

            /// Returns the current context as a mutable reference.
            #[inline(always)]
            pub fn context_mut(&mut self) -> &mut #context_type_ident {
                &mut self.context
            }

            /// Process an event.
            ///
            /// It will return `Ok(&NextState)` if the transition was successful, or `Err(#error_type_name)`
            /// if there was an error in the transition.
            pub #is_async fn process_event #process_event_impl_generics (
                &mut self,
                #temporary_context
                event: #event_input_ident
            ) -> Result<&#states_type_name <#state_lifetimes>, #error_type>
            #process_event_where_clause
            {
                #public_dispatch
            }
        }

        #machine_trait_impl
        #terminated_trait_impl
    }
}

fn type_matches_state(data_type: &Type, state: &str) -> bool {
    matches!(
        data_type,
        Type::Path(path)
            if path.qself.is_none()
                && path.path.segments.last().is_some_and(|segment| segment.ident == state)
    )
}
fn generate_actions(
    actions: &[AsyncIdent],
    temporary_context_call: &TokenStream,
    g_a_param: &TokenStream,
    error_type_name: &Ident,
    produces_state_data: bool,
    eval_actions: &[EvalAction],
    guard_params: &TokenStream,
) -> (bool, TokenStream) {
    let mut is_async = false;
    let mut code = TokenStream::new();
    let mut normal_index = 0;
    let total_steps = actions.len() + eval_actions.len();
    for position in 0..total_steps {
        if let Some(eval) = eval_actions.iter().find(|eval| eval.position == position) {
            let action_ident = &eval.action.ident;
            let action_await = if eval.action.is_async {
                is_async = true;
                quote! { .await }
            } else {
                quote! {}
            };
            let guard_expression = eval.guard.to_token_stream(&mut |guard| {
                let guard_ident = &guard.ident;
                let guard_await = if guard.is_async {
                    is_async = true;
                    quote! { .await }
                } else {
                    quote! {}
                };
                quote! {
                    {
                        let guard_result = self.context.#guard_ident(#temporary_context_call #guard_params)
                            #guard_await.map_err(#error_type_name::GuardFailed)?;
                        self.context.log_guard(stringify!(#guard_ident), guard_result);
                        guard_result
                    }
                }
            });
            code.extend(quote! {
                let eval_guard_passed = #guard_expression;
                if eval_guard_passed {
                    let _ = self.context.#action_ident(#temporary_context_call #g_a_param)
                        #action_await
                        .map_err(#error_type_name::ActionFailed)?;
                    self.context.log_action(stringify!(#action_ident));
                }
            });
            continue;
        }

        let AsyncIdent {
            ident: action_ident,
            is_async: is_a_async,
        } = &actions[normal_index];
        let action_await = if *is_a_async {
            is_async = true;
            quote! { .await }
        } else {
            quote! {}
        };
        let result = if produces_state_data && normal_index + 1 == actions.len() {
            quote! { let _data = }
        } else {
            quote! { let _ = }
        };
        code.extend(quote! {
            // ACTION
            #result self.context.#action_ident(#temporary_context_call #g_a_param) #action_await .map_err(#error_type_name::ActionFailed)?;
            self.context.log_action(stringify!(#action_ident));
        });
        normal_index += 1;
    }
    (is_async, code)
}
