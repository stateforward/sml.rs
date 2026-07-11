//! Code generation for an sml.cpp table containing multiple initial states.
//!
//! Each initial state starts an orthogonal region. Events are borrowed and
//! broadcast to every region, so one event may advance several regions.

use crate::parser::event::EventKind;
use crate::parser::state_machine::StateMachine;
use crate::parser::transition::{visit_guards, EvalAction, GuardExpression, StateTransition};
use crate::parser::AsyncIdent;
use proc_macro2::{Ident, Span, TokenStream};
use quote::{format_ident, quote};
use std::collections::{BTreeMap, HashMap, HashSet, VecDeque};
use syn::{parse, Type};

pub fn generate_code(machine: &StateMachine) -> parse::Result<TokenStream> {
    let name = machine.name.as_ref().ok_or_else(|| {
        parse::Error::new(Span::call_site(), "the `sml!` machine must have a name")
    })?;
    validate_supported(machine)?;

    let regions = discover_regions(machine)?;
    let region_count = regions.len();
    let states_name = format_ident!("{}States", name);
    let events_name = format_ident!("{}Events", name);
    let error_name = format_ident!("{}Error", name);
    let context_name = format_ident!("{}StateMachineContext", name);
    let machine_name = format_ident!("{}StateMachine", name);
    let is_async_machine = machine.entry_exit_async
        || machine.transitions.iter().any(|transition| {
            transition
                .action
                .iter()
                .chain(&transition.additional_actions)
                .any(|action| action.is_async)
                || transition.guard.as_ref().is_some_and(guard_contains_async)
                || transition
                    .eval_actions
                    .iter()
                    .any(|eval| eval.action.is_async || guard_contains_async(&eval.guard))
        });
    let has_deferred_events = machine
        .transitions
        .iter()
        .any(|transition| transition.defer);
    let has_queue_actions = has_deferred_events
        || machine
            .transitions
            .iter()
            .any(|transition| !transition.process_events.is_empty());
    let async_queue = is_async_machine && has_queue_actions;
    let async_keyword = is_async_machine.then(|| quote! { async });
    let await_stabilize = is_async_machine.then(|| quote! { .await });

    let states = collect_states(machine);
    let state_types = collect_state_types(machine)?;
    let state_variants = states.iter().map(|state| {
        state_types
            .get(&state.to_string())
            .map_or_else(|| quote! { #state }, |ty| quote! { #state(#ty) })
    });
    let initial_states = regions.iter().map(|region| {
        let initial = &region.initial;
        state_types.get(&initial.to_string()).map_or_else(
            || quote! { #states_name::#initial },
            |_| quote! { #states_name::#initial(core::default::Default::default()) },
        )
    });
    let new_const = state_types.is_empty().then(|| quote! { const });
    let deferred_field = has_deferred_events.then(|| {
        quote! { deferred: ::sml::utility::EventQueue<#events_name, 16>, }
    });
    let deferred_init =
        has_deferred_events.then(|| quote! { deferred: ::sml::utility::EventQueue::new(), });
    let pending_field = async_queue.then(|| {
        quote! { pending: ::sml::utility::EventQueue<#events_name, 16>, }
    });
    let pending_init = async_queue.then(|| quote! { pending: ::sml::utility::EventQueue::new(), });

    let events = collect_events(machine);
    let event_variants = events.values().map(|event| {
        let ident = &event.ident;
        if let Some(ty) = &event.ty {
            quote! { #ident(#ty) }
        } else {
            quote! { #ident }
        }
    });
    let conversions = events.values().filter_map(|event| {
        let ident = &event.ident;
        event.external.then(|| {
            let ty = event.ty.as_ref().expect("external events have a type");
            quote! {
                impl From<#ty> for #events_name {
                    #[inline(always)]
                    fn from(event: #ty) -> Self { Self::#ident(event) }
                }
            }
        })
    });

    let callback_error = if let Some(error) = &machine.fixed_error_type {
        quote! { #error }
    } else if machine.custom_error {
        quote! { Self::Error }
    } else {
        quote! { () }
    };
    let context_error = (machine.custom_error && machine.fixed_error_type.is_none())
        .then(|| quote! { type Error; });
    let generated_error = if let Some(error) = &machine.fixed_error_type {
        quote! { #error_name<#error> }
    } else if machine.custom_error {
        quote! { #error_name<T::Error> }
    } else {
        quote! { #error_name }
    };
    let temporary_context_parameter = machine
        .temporary_context_type
        .as_ref()
        .map(|ty| quote! { temporary_context: #ty, });
    let temporary_context_argument = match &machine.temporary_context_type {
        Some(Type::Reference(reference)) if reference.mutability.is_some() => {
            Some(quote! { &mut *temporary_context })
        }
        Some(_) => Some(quote! { temporary_context }),
        None => None,
    };
    let temporary_context_call = temporary_context_argument
        .as_ref()
        .map(|argument| quote! { #argument, });
    let (guard_methods, action_methods) = generate_context_methods(
        machine,
        &callback_error,
        machine.temporary_context_type.as_ref(),
    )?;
    let lifecycle = collect_lifecycle(machine);
    let region_dispatch = regions
        .iter()
        .enumerate()
        .map(|(index, region)| {
            generate_region_dispatch(
                index,
                region,
                machine,
                &states_name,
                &events_name,
                &error_name,
                &lifecycle,
                &temporary_context_argument,
                has_deferred_events,
                async_queue,
                &quote! { self.states },
                &quote! {
                    self.context.transition_callback(#index, &old_state, new_state);
                },
                &TokenStream::new(),
                &TokenStream::new(),
            )
        })
        .collect::<parse::Result<Vec<_>>>()?;
    let completion_dispatch = regions
        .iter()
        .enumerate()
        .map(|(index, region)| {
            generate_completion_dispatch(
                index,
                region,
                machine,
                &states_name,
                &events_name,
                &error_name,
                &lifecycle,
                &temporary_context_argument,
                has_deferred_events,
                async_queue,
                &quote! { self.states },
                &quote! {
                    self.context.transition_callback(#index, &old_state, new_state);
                },
                &TokenStream::new(),
                &TokenStream::new(),
                &quote! { true },
            )
        })
        .collect::<parse::Result<Vec<_>>>()?;
    let exception_dispatch = generate_exception_dispatch(
        &regions,
        machine,
        &states_name,
        &error_name,
        &lifecycle,
        &temporary_context_argument,
        has_deferred_events,
        async_queue,
        &quote! { self.states },
        &quote! {
            self.context.transition_callback(region_index, &old_state, new_state);
        },
        &TokenStream::new(),
        &TokenStream::new(),
    )?;
    let has_exception_handlers = machine
        .transitions
        .iter()
        .any(|transition| transition.event.kind == EventKind::Exception);

    let initial_entries = regions.iter().enumerate().filter_map(|(index, region)| {
        lifecycle
            .get(&region.initial.to_string())
            .and_then(|hooks| hooks.entry.as_ref())
            .map(|actions| {
                if state_types.contains_key(&region.initial.to_string()) {
                    let initial = &region.initial;
                    let args = callback_arguments(
                        &temporary_context_argument,
                        &[Some(quote! { state_data })],
                    );
                    let calls = action_calls(actions, args, &error_name);
                    quote! {
                        if let #states_name::#initial(state_data) = &self.states[#index] { #calls }
                    }
                } else {
                    let args = callback_arguments(&temporary_context_argument, &[]);
                    action_calls(actions, args, &error_name)
                }
            })
    });

    let dispatch_attempt = if has_exception_handlers {
        let attempt = if is_async_machine {
            quote! {
                async {
                    let mut handled = false;
                    #(#region_dispatch)*
                    Ok::<bool, #generated_error>(handled)
                }.await
            }
        } else {
            quote! {
                (|| -> Result<bool, #generated_error> {
                    let mut handled = false;
                    #(#region_dispatch)*
                    Ok(handled)
                })()
            }
        };
        quote! {
            let dispatch_result = #attempt;
            let (handled, exception_recovered) = match dispatch_result {
                Ok(handled) => (handled, false),
                Err(#error_name::GuardFailed(error)) => {
                    let error_data = &error;
                    let mut exception_handled = false;
                    #exception_dispatch
                    if exception_handled {
                        (true, true)
                    } else {
                        return Err(#error_name::GuardFailed(error));
                    }
                }
                Err(#error_name::ActionFailed(error)) => {
                    let error_data = &error;
                    let mut exception_handled = false;
                    #exception_dispatch
                    if exception_handled {
                        (true, true)
                    } else {
                        return Err(#error_name::ActionFailed(error));
                    }
                }
                Err(error) => return Err(error),
            };
        }
    } else {
        quote! {
            let mut handled = false;
            #(#region_dispatch)*
            let exception_recovered = false;
        }
    };

    let process_event_body = if async_queue {
        quote! {
            self.pending.defer(event.into())
                .map_err(|_| #error_name::QueueFull)?;
            while let Some(event) = self.pending.pop() {
                self.context.log_process_event(&self.states, &event);
                #dispatch_attempt
                if handled {
                    if exception_recovered {
                        self.stabilize(#temporary_context_call None).await?;
                    } else {
                        self.stabilize(#temporary_context_call Some(&event)).await?;
                    }
                } else {
                    return Err(#error_name::InvalidEvent);
                }
            }
            Ok(&self.states)
        }
    } else {
        quote! {
            let event = event.into();
            self.context.log_process_event(&self.states, &event);
            #dispatch_attempt
            if handled {
                if exception_recovered {
                    self.stabilize(#temporary_context_call None)#await_stabilize?;
                } else {
                    self.stabilize(#temporary_context_call Some(&event))#await_stabilize?;
                }
                Ok(&self.states)
            } else {
                Err(#error_name::InvalidEvent)
            }
        }
    };

    let states_attr = &machine.states_attr;
    let events_attr = &machine.events_attr;
    Ok(quote! {
        /// Guards, actions, lifecycle hooks, and logging for this orthogonal machine.
        pub trait #context_name {
            #context_error
            #guard_methods
            #action_methods

            fn log_process_event(&self, states: &[#states_name; #region_count], event: &#events_name) {}
            fn log_guard(&self, guard: &'static str, result: bool) {}
            fn log_action(&self, action: &'static str) {}
            fn transition_callback(
                &self,
                region: usize,
                old_state: &#states_name,
                new_state: &#states_name,
            ) {}
        }

        #[allow(missing_docs)]
        #(#states_attr)*
        pub enum #states_name { #(#state_variants),* }

        impl PartialEq for #states_name {
            fn eq(&self, other: &Self) -> bool {
                core::mem::discriminant(self) == core::mem::discriminant(other)
            }
        }

        #[allow(missing_docs)]
        #(#events_attr)*
        pub enum #events_name { #(#event_variants),* }

        #(#conversions)*

        #[derive(Debug, PartialEq)]
        pub enum #error_name<E = ()> {
            InvalidEvent,
            TransitionsFailed,
            GuardFailed(E),
            ActionFailed(E),
            QueueFull,
        }

        pub struct #machine_name<T: #context_name> {
            states: [#states_name; #region_count],
            context: T,
            #deferred_field
            #pending_field
        }

        impl<T: #context_name> #machine_name<T> {
            #[inline(always)]
            pub #new_const fn new(context: T) -> Self {
                Self {
                    states: [#(#initial_states),*],
                    context,
                    #deferred_init
                    #pending_init
                }
            }

            pub #async_keyword fn initialize(&mut self, #temporary_context_parameter) -> Result<&[#states_name; #region_count], #generated_error> {
                #(#initial_entries)*
                self.stabilize(#temporary_context_call None)#await_stabilize?;
                Ok(&self.states)
            }

            #async_keyword fn stabilize(&mut self, #temporary_context_parameter origin: Option<&#events_name>) -> Result<(), #generated_error> {
                loop {
                    let mut progressed = false;
                    #(#completion_dispatch)*
                    if !progressed { return Ok(()); }
                }
            }

            #[inline(always)]
            pub fn states(&self) -> &[#states_name; #region_count] { &self.states }

            #[inline(always)]
            pub fn state(&self, region: usize) -> Option<&#states_name> {
                self.states.get(region)
            }

            #[inline(always)]
            pub fn is(&self, expected: &[#states_name; #region_count]) -> bool {
                self.states == *expected
            }

            #[inline(always)]
            pub fn is_region(&self, region: usize, expected: &#states_name) -> bool {
                self.states.get(region).map_or(false, |state| state == expected)
            }

            #[inline(always)]
            pub fn is_terminated(&self) -> bool {
                self.states.iter().all(|state| matches!(state, #states_name::X))
            }

            #[inline(always)]
            pub fn context(&self) -> &T { &self.context }

            #[inline(always)]
            pub fn context_mut(&mut self) -> &mut T { &mut self.context }

            pub #async_keyword fn process_event<EventInput>(
                &mut self,
                #temporary_context_parameter
                event: EventInput,
            ) -> Result<&[#states_name; #region_count], #generated_error>
            where
                EventInput: Into<#events_name>,
            {
                #process_event_body
            }
        }

        impl<T: #context_name> ::sml::Terminated for #machine_name<T> {
            #[inline(always)]
            fn is_terminated(&self) -> bool { self.is_terminated() }
        }
    })
}

pub(crate) struct EmbeddedOrthogonal {
    pub state_variants: Vec<TokenStream>,
    pub initial_values: Vec<TokenStream>,
    pub region_count: usize,
    pub dispatch_regions: Vec<TokenStream>,
    pub completion: TokenStream,
    pub exception: TokenStream,
    pub enter_current: TokenStream,
    pub exit_current: TokenStream,
    pub terminal: TokenStream,
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn generate_embedded(
    machine: &StateMachine,
    states_name: &Ident,
    events_name: &Ident,
    error_name: &Ident,
    states_place: TokenStream,
    callback: TokenStream,
    structural_children: &[Ident],
    composite_exit: TokenStream,
    composite_entry: TokenStream,
    composite_terminal: TokenStream,
    temporary_context: &Option<TokenStream>,
    has_deferred_events: bool,
    async_queue: bool,
) -> parse::Result<EmbeddedOrthogonal> {
    let mut normalized_machine = machine.clone();
    for transition in &mut normalized_machine.transitions {
        if transition
            .in_state
            .composite
            .as_ref()
            .is_some_and(|child| structural_children.contains(child))
        {
            transition.in_state.data_type = None;
        } else {
            transition.in_state.composite = None;
        }
        if transition
            .out_state
            .composite
            .as_ref()
            .is_some_and(|child| structural_children.contains(child))
        {
            transition.out_state.data_type = None;
        } else {
            transition.out_state.composite = None;
        }
    }
    let machine = &normalized_machine;
    validate_supported(machine)?;
    let regions = discover_regions(machine)?;
    let region_count = regions.len();
    let states = collect_states(machine);
    let state_types = collect_state_types_excluding(machine, structural_children)?;
    let state_variants = states
        .iter()
        .map(|state| {
            state_types
                .get(&state.to_string())
                .map_or_else(|| quote! { #state }, |ty| quote! { #state(#ty) })
        })
        .collect::<Vec<_>>();
    let initial_values = regions
        .iter()
        .map(|region| {
            let initial = &region.initial;
            state_types.get(&initial.to_string()).map_or_else(
                || quote! { #states_name::#initial },
                |_| quote! { #states_name::#initial(core::default::Default::default()) },
            )
        })
        .collect::<Vec<_>>();
    let lifecycle = collect_lifecycle(machine);
    let dispatch_regions = regions
        .iter()
        .enumerate()
        .map(|(index, region)| {
            generate_region_dispatch(
                index,
                region,
                machine,
                states_name,
                events_name,
                error_name,
                &lifecycle,
                temporary_context,
                has_deferred_events,
                async_queue,
                &states_place,
                &callback,
                &composite_exit,
                &composite_entry,
            )
        })
        .collect::<parse::Result<Vec<_>>>()?;
    let completion_regions = regions
        .iter()
        .enumerate()
        .map(|(index, region)| {
            generate_completion_dispatch(
                index,
                region,
                machine,
                states_name,
                events_name,
                error_name,
                &lifecycle,
                temporary_context,
                has_deferred_events,
                async_queue,
                &states_place,
                &callback,
                &composite_exit,
                &composite_entry,
                &composite_terminal,
            )
        })
        .collect::<parse::Result<TokenStream>>()?;
    let completion = quote! {
        {
            let mut progressed = false;
            #completion_regions
            handled |= progressed;
        }
    };
    let exception = generate_exception_dispatch(
        &regions,
        machine,
        states_name,
        error_name,
        &lifecycle,
        temporary_context,
        has_deferred_events,
        async_queue,
        &states_place,
        &callback,
        &composite_exit,
        &composite_entry,
    )?;
    let lifecycle_for = |entry: bool| {
        regions
            .iter()
            .enumerate()
            .map(|(index, region)| {
                let arms = region.states.iter().filter_map(|state| {
                    let actions = lifecycle.get(state).and_then(|hooks| {
                        if entry {
                            hooks.entry.as_ref()
                        } else {
                            hooks.exit.as_ref()
                        }
                    })?;
                    let state_ident = format_ident!("{}", state);
                    if state_types.contains_key(state) {
                        let calls = action_calls(
                            actions,
                            callback_arguments(temporary_context, &[Some(quote! { state_data })]),
                            error_name,
                        );
                        Some(quote! { #states_name::#state_ident(state_data) => { #calls } })
                    } else {
                        let calls = action_calls(
                            actions,
                            callback_arguments(temporary_context, &[]),
                            error_name,
                        );
                        Some(quote! { #states_name::#state_ident => { #calls } })
                    }
                });
                quote! {
                    match &#states_place[#index] { #(#arms,)* _ => {} }
                }
            })
            .collect::<TokenStream>()
    };
    let enter_current = lifecycle_for(true);
    let exit_current = lifecycle_for(false);
    let terminal = if states.iter().any(|state| state == "X") {
        quote! {
            #states_place.iter().all(|state| matches!(state, #states_name::X))
        }
    } else {
        quote! { false }
    };
    Ok(EmbeddedOrthogonal {
        state_variants,
        initial_values,
        region_count,
        dispatch_regions,
        completion,
        exception,
        enter_current,
        exit_current,
        terminal,
    })
}

fn validate_supported(machine: &StateMachine) -> parse::Result<()> {
    for transition in &machine.transitions {
        if transition.event.kind == EventKind::Completion && transition.internal_transition {
            return Err(parse::Error::new(
                transition.in_state.ident.span(),
                "an anonymous orthogonal completion must leave its source state",
            ));
        }
    }
    Ok(())
}

pub(crate) struct Region {
    pub(crate) initial: Ident,
    pub(crate) states: HashSet<String>,
}

pub(crate) fn discover_regions(machine: &StateMachine) -> parse::Result<Vec<Region>> {
    let initials = machine
        .transitions
        .iter()
        .filter(|transition| transition.in_state.start)
        .map(|transition| transition.in_state.ident.clone())
        .collect::<Vec<_>>();
    let mut adjacency: HashMap<String, Vec<String>> = HashMap::new();
    for transition in &machine.transitions {
        if transition.in_state.wildcard {
            continue;
        }
        let from = transition.in_state.ident.to_string();
        adjacency.entry(from.clone()).or_default();
        if !transition.internal_transition && transition.out_state.ident != "X" {
            let to = transition.out_state.ident.to_string();
            adjacency.entry(from.clone()).or_default().push(to.clone());
            adjacency.entry(to).or_default().push(from);
        }
    }
    let mut claimed = HashMap::<String, usize>::new();
    let mut regions = Vec::new();
    for (index, initial) in initials.into_iter().enumerate() {
        let mut states = HashSet::new();
        let mut queue = VecDeque::from([initial.to_string()]);
        while let Some(state) = queue.pop_front() {
            if let Some(owner) = claimed.get(&state) {
                if *owner != index {
                    return Err(parse::Error::new(
                        initial.span(),
                        format!("orthogonal regions overlap at state `{state}`"),
                    ));
                }
                continue;
            }
            claimed.insert(state.clone(), index);
            states.insert(state.clone());
            queue.extend(adjacency.get(&state).into_iter().flatten().cloned());
        }
        regions.push(Region { initial, states });
    }
    for state in adjacency.keys() {
        if !claimed.contains_key(state) {
            return Err(parse::Error::new(
                Span::call_site(),
                format!("state `{state}` is not connected to an orthogonal initial state"),
            ));
        }
    }
    Ok(regions)
}

fn collect_states(machine: &StateMachine) -> Vec<Ident> {
    let mut states = BTreeMap::<String, Ident>::new();
    for transition in &machine.transitions {
        if !transition.in_state.wildcard {
            states.insert(
                transition.in_state.ident.to_string(),
                transition.in_state.ident.clone(),
            );
        }
        if !transition.internal_transition {
            states.insert(
                transition.out_state.ident.to_string(),
                transition.out_state.ident.clone(),
            );
        }
    }
    states.into_values().collect()
}

fn collect_state_types(machine: &StateMachine) -> parse::Result<BTreeMap<String, Type>> {
    collect_state_types_excluding(machine, &[])
}

fn collect_state_types_excluding(
    machine: &StateMachine,
    structural_children: &[Ident],
) -> parse::Result<BTreeMap<String, Type>> {
    let mut types = BTreeMap::<String, Type>::new();
    for transition in &machine.transitions {
        for (ident, ty, composite) in [
            (
                &transition.in_state.ident,
                transition.in_state.data_type.as_ref(),
                transition.in_state.composite.as_ref(),
            ),
            (
                &transition.out_state.ident,
                transition.out_state.data_type.as_ref(),
                transition.out_state.composite.as_ref(),
            ),
        ] {
            if composite.is_some_and(|child| structural_children.contains(child)) {
                continue;
            }
            let Some(ty) = ty else { continue };
            let key = ident.to_string();
            if let Some(existing) = types.get(&key) {
                if existing != ty {
                    return Err(parse::Error::new(
                        ident.span(),
                        format!("state `{ident}` has incompatible payload types"),
                    ));
                }
            } else {
                types.insert(key, ty.clone());
            }
        }
    }
    Ok(types)
}

struct EventInfo {
    ident: Ident,
    ty: Option<Type>,
    external: bool,
}

fn collect_events(machine: &StateMachine) -> BTreeMap<String, EventInfo> {
    let mut events = BTreeMap::new();
    for transition in &machine.transitions {
        if matches!(
            transition.event.kind,
            EventKind::Normal | EventKind::Unexpected
        ) && !transition.event.wildcard
        {
            events
                .entry(transition.event.ident.to_string())
                .or_insert_with(|| EventInfo {
                    ident: transition.event.ident.clone(),
                    ty: transition.event.data_type.clone(),
                    external: transition.event.external,
                });
        }
    }
    events
}

#[derive(Default)]
struct Lifecycle {
    entry: Option<Vec<AsyncIdent>>,
    exit: Option<Vec<AsyncIdent>>,
}

fn collect_lifecycle(machine: &StateMachine) -> HashMap<String, Lifecycle> {
    let mut lifecycle = HashMap::<String, Lifecycle>::new();
    for transition in &machine.transitions {
        let slot = match transition.event.kind {
            EventKind::Entry => {
                &mut lifecycle
                    .entry(transition.in_state.ident.to_string())
                    .or_default()
                    .entry
            }
            EventKind::Exit => {
                &mut lifecycle
                    .entry(transition.in_state.ident.to_string())
                    .or_default()
                    .exit
            }
            _ => continue,
        };
        *slot = Some(
            transition
                .action
                .iter()
                .chain(&transition.additional_actions)
                .cloned()
                .collect(),
        );
    }
    lifecycle
}

fn generate_context_methods(
    machine: &StateMachine,
    error_type: &TokenStream,
    temporary_context_type: Option<&Type>,
) -> parse::Result<(TokenStream, TokenStream)> {
    let mut guards = BTreeMap::<String, TokenStream>::new();
    let mut actions = BTreeMap::<String, TokenStream>::new();
    for transition in &machine.transitions {
        let event_ty = transition_event_type(machine, transition);
        let state_ty = transition.in_state.data_type.as_ref();
        let callback_parameters = |receiver: TokenStream| {
            let temporary = temporary_context_type.map(|ty| quote! { temporary_context: #ty, });
            let state = state_ty.map(|ty| quote! { state_data: &#ty, });
            let event = event_ty.map(|ty| quote! { event: &#ty, });
            quote! { #receiver, #temporary #state #event }
        };
        if let Some(expression) = &transition.guard {
            visit_guards(expression, |guard| {
                let ident = &guard.ident;
                let async_keyword = guard.is_async.then(|| quote! { async });
                let parameters = callback_parameters(quote! { &self });
                let signature = quote! {
                    #async_keyword fn #ident(#parameters) -> Result<bool, #error_type>;
                };
                insert_unique(&mut guards, ident, signature)?;
                Ok(())
            })?;
        }
        let transition_actions = transition
            .action
            .iter()
            .chain(&transition.additional_actions)
            .collect::<Vec<_>>();
        for (index, action) in transition_actions.iter().enumerate() {
            let ident = &action.ident;
            let async_keyword = action.is_async.then(|| quote! { async });
            let parameters = callback_parameters(quote! { &mut self });
            let produces_state = !transition.internal_transition
                && index + 1 == transition_actions.len()
                && transition.out_state.data_type.is_some();
            let output = if produces_state {
                let ty = transition.out_state.data_type.as_ref().unwrap();
                quote! { #ty }
            } else {
                quote! { () }
            };
            let signature = quote! {
                #async_keyword fn #ident(#parameters) -> Result<#output, #error_type>;
            };
            insert_unique(&mut actions, ident, signature)?;
        }
        for eval in &transition.eval_actions {
            visit_guards(&eval.guard, |guard| {
                let ident = &guard.ident;
                let async_keyword = guard.is_async.then(|| quote! { async });
                let parameters = callback_parameters(quote! { &self });
                let signature = quote! {
                    #async_keyword fn #ident(#parameters) -> Result<bool, #error_type>;
                };
                insert_unique(&mut guards, ident, signature)
            })?;
            let ident = &eval.action.ident;
            let async_keyword = eval.action.is_async.then(|| quote! { async });
            let parameters = callback_parameters(quote! { &mut self });
            let signature = quote! {
                #async_keyword fn #ident(#parameters) -> Result<(), #error_type>;
            };
            insert_unique(&mut actions, ident, signature)?;
        }
    }
    Ok((
        guards.into_values().collect(),
        actions.into_values().collect(),
    ))
}

fn transition_event_type<'a>(
    machine: &'a StateMachine,
    transition: &'a StateTransition,
) -> Option<&'a Type> {
    transition.event.data_type.as_ref().or_else(|| {
        (transition.event.kind == EventKind::Completion && !transition.event.wildcard)
            .then(|| {
                machine
                    .transitions
                    .iter()
                    .find(|candidate| {
                        candidate.event.kind == EventKind::Normal
                            && candidate.event.ident == transition.event.ident
                    })
                    .and_then(|candidate| candidate.event.data_type.as_ref())
            })
            .flatten()
    })
}

fn insert_unique(
    map: &mut BTreeMap<String, TokenStream>,
    ident: &Ident,
    signature: TokenStream,
) -> parse::Result<()> {
    let key = ident.to_string();
    if let Some(existing) = map.get(&key) {
        if existing.to_string() != signature.to_string() {
            return Err(parse::Error::new(
                ident.span(),
                format!("callback `{ident}` is used with incompatible event types"),
            ));
        }
    } else {
        map.insert(key, signature);
    }
    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn generate_region_dispatch(
    index: usize,
    region: &Region,
    machine: &StateMachine,
    states_name: &Ident,
    events_name: &Ident,
    error_name: &Ident,
    lifecycle: &HashMap<String, Lifecycle>,
    temporary_context: &Option<TokenStream>,
    has_deferred_events: bool,
    async_queue: bool,
    states_place: &TokenStream,
    callback: &TokenStream,
    composite_exit: &TokenStream,
    composite_entry: &TokenStream,
) -> parse::Result<TokenStream> {
    let mut arms = Vec::new();
    for state in &region.states {
        let transitions = machine
            .transitions
            .iter()
            .filter(|transition| {
                transition.in_state.ident == state.as_str()
                    && transition.event.kind == EventKind::Normal
            })
            .collect::<Vec<_>>();
        let mut by_event = BTreeMap::<String, Vec<&StateTransition>>::new();
        for transition in transitions {
            by_event
                .entry(transition.event.ident.to_string())
                .or_default()
                .push(transition);
        }
        for event_transitions in by_event.into_values() {
            let first = event_transitions[0];
            let state_ident = &first.in_state.ident;
            let event_ident = &first.event.ident;
            let event_pattern = if first.event.data_type.is_some() {
                quote! { #events_name::#event_ident(event_data) }
            } else {
                quote! { #events_name::#event_ident }
            };
            let branches = event_transitions
                .iter()
                .map(|transition| {
                    generate_transition_branch(
                        index,
                        transition,
                        states_name,
                        error_name,
                        lifecycle,
                        transition.event.data_type.is_some(),
                        temporary_context,
                        has_deferred_events,
                        async_queue,
                        states_place,
                        callback,
                        composite_exit,
                        composite_entry,
                    )
                })
                .collect::<parse::Result<Vec<_>>>()?;
            let state_pattern = if first.in_state.data_type.is_some() {
                quote! { #states_name::#state_ident(state_data) }
            } else {
                quote! { #states_name::#state_ident }
            };
            arms.push(quote! {
                (#state_pattern, #event_pattern) => { #(#branches)* }
            });
        }

        let specific_unexpected = machine.transitions.iter().filter(|transition| {
            transition.in_state.ident == state.as_str()
                && transition.event.kind == EventKind::Unexpected
                && !transition.event.wildcard
        });
        for transition in specific_unexpected {
            let state_ident = &transition.in_state.ident;
            let event_ident = &transition.event.ident;
            let event_pattern = if transition.event.data_type.is_some() {
                quote! { #events_name::#event_ident(event_data) }
            } else {
                quote! { #events_name::#event_ident }
            };
            let branch = generate_transition_branch(
                index,
                transition,
                states_name,
                error_name,
                lifecycle,
                transition.event.data_type.is_some(),
                temporary_context,
                has_deferred_events,
                async_queue,
                states_place,
                callback,
                composite_exit,
                composite_entry,
            )?;
            let state_pattern = if transition.in_state.data_type.is_some() {
                quote! { #states_name::#state_ident(state_data) }
            } else {
                quote! { #states_name::#state_ident }
            };
            arms.push(quote! {
                (#state_pattern, #event_pattern) => { #branch }
            });
        }

        if let Some(transition) = machine.transitions.iter().find(|transition| {
            transition.in_state.ident == state.as_str()
                && transition.event.kind == EventKind::Unexpected
                && transition.event.wildcard
        }) {
            let state_ident = &transition.in_state.ident;
            let branch = generate_transition_branch(
                index,
                transition,
                states_name,
                error_name,
                lifecycle,
                false,
                temporary_context,
                has_deferred_events,
                async_queue,
                states_place,
                callback,
                composite_exit,
                composite_entry,
            )?;
            let state_pattern = if transition.in_state.data_type.is_some() {
                quote! { #states_name::#state_ident(state_data) }
            } else {
                quote! { #states_name::#state_ident }
            };
            arms.push(quote! {
                (#state_pattern, _) => { #branch }
            });
        }
    }
    Ok(quote! {
        {
            let region_index = #index;
            let mut region_handled = false;
            match (&mut #states_place[#index], &event) {
                #(#arms,)*
                _ => {}
            }
            handled |= region_handled;
        }
    })
}

#[allow(clippy::too_many_arguments)]
fn generate_transition_branch(
    index: usize,
    transition: &StateTransition,
    states_name: &Ident,
    error_name: &Ident,
    lifecycle: &HashMap<String, Lifecycle>,
    has_event_data: bool,
    temporary_context: &Option<TokenStream>,
    has_deferred_events: bool,
    async_queue: bool,
    states_place: &TokenStream,
    callback: &TokenStream,
    composite_exit: &TokenStream,
    composite_entry: &TokenStream,
) -> parse::Result<TokenStream> {
    let state_arg = transition
        .in_state
        .data_type
        .is_some()
        .then(|| quote! { state_data });
    let state_callback_arg = state_arg.clone().unwrap_or_default();
    let event_arg = has_event_data.then(|| quote! { event_data });
    let callback_args = callback_arguments(temporary_context, &[state_arg, event_arg]);
    let guard = transition
        .guard
        .as_ref()
        .map(|guard| guard_tokens(guard, &callback_args, error_name))
        .transpose()?;
    let actions = transition
        .action
        .iter()
        .chain(&transition.additional_actions)
        .cloned()
        .collect::<Vec<_>>();
    let produces_state = !transition.internal_transition
        && transition.out_state.data_type.is_some()
        && !actions.is_empty();
    let action_code = action_sequence(
        &actions,
        &transition.eval_actions,
        &callback_args,
        error_name,
        produces_state,
    )?;
    let output_data = if transition.out_state.data_type.is_some()
        && !transition.internal_transition
        && !produces_state
    {
        quote! { let output_data = core::default::Default::default(); }
    } else {
        quote! {}
    };
    let temporary_context_call = temporary_context
        .as_ref()
        .map(|argument| quote! { #argument, });
    let process_code = transition
        .process_events
        .iter()
        .map(|event| {
            if async_queue {
                quote! {
                    self.pending.defer((#event).into())
                        .map_err(|_| #error_name::QueueFull)?;
                }
            } else {
                quote! { let _ = self.process_event(#temporary_context_call #event)?; }
            }
        })
        .collect::<Vec<_>>();
    let defer_code = if transition.defer {
        if transition.event.wildcard {
            return Err(parse::Error::new(
                transition.event.ident.span(),
                "a wildcard event cannot be deferred because its owned type is unknown",
            ));
        }
        let event = &transition.event.ident;
        let events_name = format_ident!(
            "{}Events",
            states_name.to_string().trim_end_matches("States")
        );
        let event_value = if transition.event.data_type.is_some() {
            // The generated queue owns deferred data; cloning is required only
            // on rows that explicitly request `/ defer`.
            quote! { #events_name::#event((*event_data).clone()) }
        } else {
            quote! { #events_name::#event }
        };
        quote! {
            self.deferred.defer(#event_value)
                .map_err(|_| #error_name::QueueFull)?;
        }
    } else {
        quote! {}
    };
    let drain_deferred = if has_deferred_events && !transition.internal_transition {
        if async_queue {
            quote! {
                while let Some(deferred_event) = self.deferred.pop() {
                    self.pending.defer(deferred_event)
                        .map_err(|_| #error_name::QueueFull)?;
                }
            }
        } else {
            quote! {
                while let Some(deferred_event) = self.deferred.pop() {
                    let _ = self.process_event(#temporary_context_call deferred_event);
                }
            }
        }
    } else {
        quote! {}
    };
    let target = &transition.out_state.ident;
    let exit_composite =
        if transition.in_state.composite.is_some() && transition.out_state.composite.is_none() {
            composite_exit.clone()
        } else {
            TokenStream::new()
        };
    let enter_composite =
        if transition.out_state.composite.is_some() && transition.in_state.composite.is_none() {
            composite_entry.clone()
        } else {
            TokenStream::new()
        };
    let body = if transition.internal_transition {
        quote! {
            #action_code
            #defer_code
            #(#process_code)*
            region_handled = true;
        }
    } else {
        let exit = lifecycle
            .get(&transition.in_state.ident.to_string())
            .and_then(|hooks| hooks.exit.as_ref())
            .map(|a| {
                let state = transition
                    .in_state
                    .data_type
                    .is_some()
                    .then(|| state_callback_arg.clone());
                action_calls(
                    a,
                    callback_arguments(temporary_context, &[state]),
                    error_name,
                )
            })
            .unwrap_or_default();
        let entry_actions = lifecycle
            .get(&target.to_string())
            .and_then(|hooks| hooks.entry.as_ref())
            .cloned()
            .unwrap_or_default();
        let target_expression = if transition.out_state.data_type.is_some() {
            quote! { #states_name::#target(output_data) }
        } else {
            quote! { #states_name::#target }
        };
        let entry = if transition.out_state.data_type.is_some() {
            let calls = action_calls(
                &entry_actions,
                callback_arguments(temporary_context, &[Some(quote! { new_state_data })]),
                error_name,
            );
            quote! {
                if let #states_name::#target(new_state_data) = &#states_place[#index] { #calls }
            }
        } else {
            action_calls(
                &entry_actions,
                callback_arguments(temporary_context, &[]),
                error_name,
            )
        };
        quote! {
            #exit_composite
            #exit
            #action_code
            #output_data
            let new_state = #target_expression;
            let old_state = core::mem::replace(&mut #states_place[#index], new_state);
            let new_state = &#states_place[#index];
            #callback
            #entry
            #enter_composite
            #defer_code
            #(#process_code)*
            #drain_deferred
            region_handled = true;
        }
    };
    Ok(if let Some(guard) = guard {
        quote! { if !region_handled && #guard { #body } }
    } else {
        quote! { if !region_handled { #body } }
    })
}

#[allow(clippy::too_many_arguments)]
fn generate_exception_dispatch(
    regions: &[Region],
    machine: &StateMachine,
    states_name: &Ident,
    error_name: &Ident,
    lifecycle: &HashMap<String, Lifecycle>,
    temporary_context: &Option<TokenStream>,
    has_deferred_events: bool,
    async_queue: bool,
    states_place: &TokenStream,
    callback: &TokenStream,
    composite_exit: &TokenStream,
    composite_entry: &TokenStream,
) -> parse::Result<TokenStream> {
    let mut region_dispatches = Vec::new();
    for (index, region) in regions.iter().enumerate() {
        let mut arms = Vec::new();
        for state in &region.states {
            let mut transitions = machine
                .transitions
                .iter()
                .filter(|transition| {
                    transition.in_state.ident == state.as_str()
                        && transition.event.kind == EventKind::Exception
                })
                .collect::<Vec<_>>();
            transitions.sort_by_key(|transition| transition.event.wildcard);
            if transitions.is_empty() {
                continue;
            }
            for transition in &transitions {
                if transition.defer || !transition.process_events.is_empty() {
                    return Err(parse::Error::new(
                        transition.event.ident.span(),
                        "exception handlers cannot defer or process events",
                    ));
                }
            }
            let state_ident = &transitions[0].in_state.ident;
            let state_pattern = if transitions[0].in_state.data_type.is_some() {
                quote! { #states_name::#state_ident(state_data) }
            } else {
                quote! { #states_name::#state_ident }
            };
            let branches = transitions
                .iter()
                .map(|transition| {
                    let typed = !transition.event.wildcard;
                    let branch = generate_transition_branch(
                        index,
                        transition,
                        states_name,
                        error_name,
                        lifecycle,
                        typed,
                        temporary_context,
                        has_deferred_events,
                        async_queue,
                        states_place,
                        callback,
                        composite_exit,
                        composite_entry,
                    )?;
                    Ok(if typed {
                        quote! { let event_data = error_data; #branch }
                    } else {
                        branch
                    })
                })
                .collect::<parse::Result<Vec<_>>>()?;
            arms.push(quote! { #state_pattern => { #(#branches)* } });
        }
        region_dispatches.push(quote! {
            {
                let region_index = #index;
                let mut region_handled = false;
                match &mut #states_place[#index] {
                    #(#arms,)*
                    _ => {}
                }
                exception_handled |= region_handled;
            }
        });
    }
    Ok(quote! { #(#region_dispatches)* })
}

#[allow(clippy::too_many_arguments)]
fn generate_completion_dispatch(
    index: usize,
    region: &Region,
    machine: &StateMachine,
    states_name: &Ident,
    events_name: &Ident,
    error_name: &Ident,
    lifecycle: &HashMap<String, Lifecycle>,
    temporary_context: &Option<TokenStream>,
    has_deferred_events: bool,
    async_queue: bool,
    states_place: &TokenStream,
    callback: &TokenStream,
    composite_exit: &TokenStream,
    composite_entry: &TokenStream,
    composite_terminal: &TokenStream,
) -> parse::Result<TokenStream> {
    let mut anonymous_arms = Vec::new();
    let mut origin_arms = Vec::new();
    for state in &region.states {
        let anonymous = machine
            .transitions
            .iter()
            .filter(|transition| {
                transition.in_state.ident == state.as_str()
                    && transition.event.kind == EventKind::Completion
                    && transition.event.wildcard
            })
            .collect::<Vec<_>>();
        if !anonymous.is_empty() {
            let state_ident = &anonymous[0].in_state.ident;
            let branches = anonymous
                .iter()
                .map(|transition| {
                    let branch = generate_transition_branch(
                        index,
                        transition,
                        states_name,
                        error_name,
                        lifecycle,
                        false,
                        temporary_context,
                        has_deferred_events,
                        async_queue,
                        states_place,
                        callback,
                        composite_exit,
                        composite_entry,
                    )?;
                    Ok(if transition.in_state.composite.is_some() {
                        quote! { if #composite_terminal { #branch } }
                    } else {
                        branch
                    })
                })
                .collect::<parse::Result<Vec<_>>>()?;
            let state_pattern = if anonymous[0].in_state.data_type.is_some() {
                quote! { #states_name::#state_ident(state_data) }
            } else {
                quote! { #states_name::#state_ident }
            };
            anonymous_arms.push(quote! {
                #state_pattern => { #(#branches)* }
            });
        }

        for transition in machine.transitions.iter().filter(|transition| {
            transition.in_state.ident == state.as_str()
                && transition.event.kind == EventKind::Completion
                && !transition.event.wildcard
        }) {
            let state_ident = &transition.in_state.ident;
            let event_ident = &transition.event.ident;
            let has_data = transition_event_type(machine, transition).is_some();
            let event_pattern = if has_data {
                quote! { #events_name::#event_ident(event_data) }
            } else {
                quote! { #events_name::#event_ident }
            };
            let branch = generate_transition_branch(
                index,
                transition,
                states_name,
                error_name,
                lifecycle,
                has_data,
                temporary_context,
                has_deferred_events,
                async_queue,
                states_place,
                callback,
                composite_exit,
                composite_entry,
            )?;
            let branch = if transition.in_state.composite.is_some() {
                quote! { if #composite_terminal { #branch } }
            } else {
                branch
            };
            let state_pattern = if transition.in_state.data_type.is_some() {
                quote! { #states_name::#state_ident(state_data) }
            } else {
                quote! { #states_name::#state_ident }
            };
            origin_arms.push(quote! {
                (#state_pattern, #event_pattern) => { #branch }
            });
        }
    }
    Ok(quote! {
        {
            let region_index = #index;
            let mut region_handled = false;
            if let Some(origin) = origin {
                match (&mut #states_place[#index], origin) {
                    #(#origin_arms,)*
                    _ => {}
                }
            }
            if !region_handled {
                match &mut #states_place[#index] {
                    #(#anonymous_arms,)*
                    _ => {}
                }
            }
            progressed |= region_handled;
        }
    })
}

fn callback_arguments(
    temporary_context: &Option<TokenStream>,
    arguments: &[Option<TokenStream>],
) -> TokenStream {
    let arguments = temporary_context
        .iter()
        .cloned()
        .chain(arguments.iter().flatten().cloned())
        .collect::<Vec<_>>();
    quote! { #(#arguments),* }
}

fn action_sequence(
    actions: &[AsyncIdent],
    eval_actions: &[EvalAction],
    callback_args: &TokenStream,
    error_name: &Ident,
    produces_state: bool,
) -> parse::Result<TokenStream> {
    let mut output = TokenStream::new();
    let mut action_index = 0;
    let total = actions.len() + eval_actions.len();
    for position in 0..total {
        if let Some(eval) = eval_actions.iter().find(|eval| eval.position == position) {
            let guard = guard_tokens(&eval.guard, callback_args, error_name)?;
            let action = &eval.action.ident;
            let action_await = eval.action.is_async.then(|| quote! { .await });
            output.extend(quote! {
                let eval_guard_passed = #guard;
                if eval_guard_passed {
                    self.context.#action(#callback_args)#action_await
                        .map_err(#error_name::ActionFailed)?;
                    self.context.log_action(stringify!(#action));
                }
            });
        } else {
            let action = &actions[action_index];
            let ident = &action.ident;
            let action_await = action.is_async.then(|| quote! { .await });
            let binding = if produces_state && action_index + 1 == actions.len() {
                quote! { let output_data = }
            } else {
                quote! { let _ = }
            };
            output.extend(quote! {
                #binding self.context.#ident(#callback_args)#action_await
                    .map_err(#error_name::ActionFailed)?;
                self.context.log_action(stringify!(#ident));
            });
            action_index += 1;
        }
    }
    Ok(output)
}

fn action_calls(actions: &[AsyncIdent], event_arg: TokenStream, error_name: &Ident) -> TokenStream {
    actions
        .iter()
        .map(|action| {
            let ident = &action.ident;
            let action_await = action.is_async.then(|| quote! { .await });
            quote! {
                self.context.#ident(#event_arg)#action_await.map_err(#error_name::ActionFailed)?;
                self.context.log_action(stringify!(#ident));
            }
        })
        .collect::<TokenStream>()
}

fn guard_tokens(
    guard: &GuardExpression,
    event_arg: &TokenStream,
    error_name: &Ident,
) -> parse::Result<TokenStream> {
    Ok(guard.to_token_stream(&mut |guard| {
        let ident = &guard.ident;
        let guard_await = guard.is_async.then(|| quote! { .await });
        quote! {
            {
                let guard_result = self.context.#ident(#event_arg)#guard_await
                    .map_err(#error_name::GuardFailed)?;
                self.context.log_guard(stringify!(#ident), guard_result);
                guard_result
            }
        }
    }))
}

fn guard_contains_async(guard: &GuardExpression) -> bool {
    let mut contains_async = false;
    let _ = visit_guards(guard, |guard| {
        contains_async |= guard.is_async;
        Ok(())
    });
    contains_async
}
