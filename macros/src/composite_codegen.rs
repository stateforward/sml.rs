//! Native child-first composite state-machine generation.

use crate::parser::event::EventKind;
use crate::parser::state_machine::StateMachine;
use crate::parser::transition::{visit_guards, EvalAction, GuardExpression, StateTransition};
use crate::parser::AsyncIdent;
use proc_macro2::{Ident, Span, TokenStream};
use quote::{format_ident, quote};
use std::collections::{BTreeMap, HashMap};
use syn::{parse, spanned::Spanned, Type};

pub fn generate_code(machines: &[StateMachine]) -> parse::Result<TokenStream> {
    let machine_names = machines
        .iter()
        .filter_map(|machine| machine.name.as_ref())
        .collect::<Vec<_>>();
    let referenced = machines
        .iter()
        .flat_map(|machine| &machine.transitions)
        .flat_map(|transition| {
            transition
                .in_state
                .composite
                .iter()
                .chain(transition.out_state.composite.iter())
        })
        .collect::<Vec<_>>();
    let parent = machines
        .iter()
        .find(|machine| {
            has_composite(machine, &machine_names)
                && machine
                    .name
                    .as_ref()
                    .is_some_and(|name| !referenced.contains(&name))
        })
        .or_else(|| {
            machines
                .iter()
                .find(|machine| has_composite(machine, &machine_names))
        })
        .ok_or_else(|| parse::Error::new(Span::call_site(), "no composite parent table found"))?;
    let child_references = parent
        .transitions
        .iter()
        .flat_map(|transition| {
            transition
                .in_state
                .composite
                .iter()
                .chain(transition.out_state.composite.iter())
        })
        .filter(|reference| machine_names.contains(reference))
        .fold(Vec::<&Ident>::new(), |mut references, reference| {
            if !references.contains(&reference) {
                references.push(reference);
            }
            references
        });
    let has_grandchildren = child_references.iter().any(|reference| {
        machines
            .iter()
            .find(|machine| machine.name.as_ref() == Some(*reference))
            .is_some_and(|child| has_composite(child, &machine_names))
    });
    let has_orthogonal_child = child_references.iter().any(|reference| {
        machines
            .iter()
            .find(|machine| machine.name.as_ref() == Some(*reference))
            .is_some_and(|child| {
                child
                    .transitions
                    .iter()
                    .filter(|transition| transition.in_state.start)
                    .count()
                    > 1
            })
    });
    let orthogonal_root = parent
        .transitions
        .iter()
        .filter(|transition| transition.in_state.start)
        .count()
        > 1;
    if child_references.len() > 1 || has_grandchildren || has_orthogonal_child || orthogonal_root {
        return generate_multi_code(machines, parent, &child_references);
    }
    let child_reference = parent
        .transitions
        .iter()
        .find_map(|transition| {
            transition
                .in_state
                .composite
                .as_ref()
                .or(transition.out_state.composite.as_ref())
                .filter(|reference| machine_names.contains(reference))
        })
        .ok_or_else(|| parse::Error::new(Span::call_site(), "missing `state<Sub>` reference"))?;
    let child = machines
        .iter()
        .find(|machine| machine.name.as_ref() == Some(child_reference))
        .ok_or_else(|| {
            parse::Error::new(
                child_reference.span(),
                format!("no `{child_reference} {{ ... }}` child table was provided"),
            )
        })?;
    validate(parent, child, child_reference)?;

    // Within a composite expansion, only references to the supplied child
    // table are structural. Other `state<T>` spellings are ordinary typed
    // states whose inferred `T` is stored as payload data.
    let mut normalized_parent = parent.clone();
    for transition in &mut normalized_parent.transitions {
        if transition.in_state.composite.as_ref() != Some(child_reference) {
            transition.in_state.composite = None;
        }
        if transition.out_state.composite.as_ref() != Some(child_reference) {
            transition.out_state.composite = None;
        }
    }
    let mut normalized_child = child.clone();
    for transition in &mut normalized_child.transitions {
        transition.in_state.composite = None;
        transition.out_state.composite = None;
    }
    let parent = &normalized_parent;
    let child = &normalized_child;

    let parent_name = parent.name.as_ref().expect("named sml definition");
    let child_state_variant =
        crate::parser::state_ident(&child_reference.to_string(), child_reference.span());
    let parent_states_name = format_ident!("{}States", parent_name);
    let child_states_name = format_ident!("{}{}States", parent_name, child_state_variant);
    let events_name = format_ident!("{}Events", parent_name);
    let context_name = format_ident!("{}StateMachineContext", parent_name);
    let machine_name = format_ident!("{}StateMachine", parent_name);
    let error_name = format_ident!("{}Error", parent_name);
    let is_async_machine = [parent, child].iter().any(|machine| {
        machine.entry_exit_async
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
            })
    });
    let has_deferred_events = [parent, child]
        .iter()
        .flat_map(|machine| &machine.transitions)
        .any(|transition| transition.defer);
    let has_queue_actions = has_deferred_events
        || [parent, child]
            .iter()
            .flat_map(|machine| &machine.transitions)
            .any(|transition| !transition.process_events.is_empty());
    let async_queue = is_async_machine && has_queue_actions;
    let async_keyword = is_async_machine.then(|| quote! { async });
    let await_stabilize = is_async_machine.then(|| quote! { .await });

    let parent_states = collect_states(parent);
    let child_states = collect_states(child);
    let parent_state_types = collect_state_types(parent, Some(child_reference))?;
    let child_state_types = collect_state_types(child, None)?;
    let parent_state_variants = parent_states.values().map(|state| {
        parent_state_types
            .get(&state.to_string())
            .map_or_else(|| quote! { #state }, |ty| quote! { #state(#ty) })
    });
    let child_state_variants = child_states.values().map(|state| {
        child_state_types
            .get(&state.to_string())
            .map_or_else(|| quote! { #state }, |ty| quote! { #state(#ty) })
    });
    let parent_terminated = if parent_states.contains_key("X") {
        quote! { matches!(self.state, #parent_states_name::X) }
    } else {
        quote! { false }
    };
    let parent_initial = initial_state(parent)?;
    let child_initial = initial_state(child)?;
    let parent_initial_value = parent_state_types
        .contains_key(&parent_initial.to_string())
        .then(|| quote! { (core::default::Default::default()) });
    let child_initial_value = child_state_types
        .contains_key(&child_initial.to_string())
        .then(|| quote! { (core::default::Default::default()) });
    let new_const =
        (parent_state_types.is_empty() && child_state_types.is_empty()).then(|| quote! { const });
    let deferred_field = has_deferred_events.then(|| {
        quote! { deferred: ::sml::utility::EventQueue<#events_name, 16>, }
    });
    let deferred_init =
        has_deferred_events.then(|| quote! { deferred: ::sml::utility::EventQueue::new(), });
    let pending_field = async_queue.then(|| {
        quote! { pending: ::sml::utility::EventQueue<#events_name, 16>, }
    });
    let pending_init = async_queue.then(|| quote! { pending: ::sml::utility::EventQueue::new(), });
    let events = collect_events(parent, child)?;
    let event_variants = events.values().map(|event| {
        let ident = &event.ident;
        if let Some(ty) = &event.ty {
            quote! { #ident(#ty) }
        } else {
            quote! { #ident }
        }
    });
    let conversions = events.values().filter(|event| event.external).map(|event| {
        let ident = &event.ident;
        let ty = event.ty.as_ref().expect("external event type");
        quote! {
            impl From<#ty> for #events_name {
                #[inline(always)]
                fn from(event: #ty) -> Self { Self::#ident(event) }
            }
        }
    });
    let fixed_error_type = parent
        .fixed_error_type
        .as_ref()
        .or(child.fixed_error_type.as_ref());
    if let (Some(parent_error), Some(child_error)) = (
        parent.fixed_error_type.as_ref(),
        child.fixed_error_type.as_ref(),
    ) {
        if parent_error != child_error {
            return Err(parse::Error::new(
                child_error.span(),
                "parent and child exception error types must match",
            ));
        }
    }
    let custom_error = parent.custom_error || child.custom_error;
    let callback_error = if let Some(error) = fixed_error_type {
        quote! { #error }
    } else if custom_error {
        quote! { Self::Error }
    } else {
        quote! { () }
    };
    let context_error =
        (custom_error && fixed_error_type.is_none()).then(|| quote! { type Error; });
    let generated_error = if let Some(error) = fixed_error_type {
        quote! { #error_name<#error> }
    } else if custom_error {
        quote! { #error_name<T::Error> }
    } else {
        quote! { #error_name }
    };
    let temporary_context_type = match (
        parent.temporary_context_type.as_ref(),
        child.temporary_context_type.as_ref(),
    ) {
        (Some(parent_type), Some(child_type)) if parent_type != child_type => {
            return Err(parse::Error::new(
                child_type.span(),
                "parent and child temporary context types must match",
            ));
        }
        (Some(ty), _) | (_, Some(ty)) => Some(ty),
        (None, None) => None,
    };
    let temporary_context_parameter =
        temporary_context_type.map(|ty| quote! { temporary_context: #ty, });
    let temporary_context_argument = match temporary_context_type {
        Some(Type::Reference(reference)) if reference.mutability.is_some() => {
            Some(quote! { &mut *temporary_context })
        }
        Some(_) => Some(quote! { temporary_context }),
        None => None,
    };
    let temporary_context_call = temporary_context_argument
        .as_ref()
        .map(|argument| quote! { #argument, });
    let (guards, actions) = context_methods(
        parent,
        child,
        &events,
        &callback_error,
        temporary_context_type,
    )?;
    let parent_lifecycle = collect_lifecycle(parent);
    let child_lifecycle = collect_lifecycle(child);
    let child_entry_hook = current_lifecycle_code(
        &child_lifecycle,
        true,
        &child_states_name,
        quote! { self.child_state },
        &child_state_types,
        &error_name,
        &temporary_context_argument,
    );
    let child_has_history = child
        .transitions
        .iter()
        .any(|transition| transition.in_state.history);
    let enter_child_current = if child_has_history {
        child_entry_hook
    } else {
        quote! {
            self.child_state = #child_states_name::#child_initial #child_initial_value;
            #child_entry_hook
        }
    };
    let exit_child_current = current_lifecycle_code(
        &child_lifecycle,
        false,
        &child_states_name,
        quote! { self.child_state },
        &child_state_types,
        &error_name,
        &temporary_context_argument,
    );
    let child_dispatch = dispatch_code(
        child,
        quote! { self.child_state },
        &child_states_name,
        &events_name,
        &error_name,
        &child_lifecycle,
        TokenStream::new(),
        TokenStream::new(),
        quote! { self.context.child_transition_callback(&old_state, new_state); },
        &temporary_context_argument,
        has_deferred_events,
        async_queue,
    )?;
    let parent_dispatch = dispatch_code(
        parent,
        quote! { self.state },
        &parent_states_name,
        &events_name,
        &error_name,
        &parent_lifecycle,
        exit_child_current.clone(),
        enter_child_current.clone(),
        quote! { self.context.transition_callback(&old_state, new_state); },
        &temporary_context_argument,
        has_deferred_events,
        async_queue,
    )?;
    let child_completion = completion_code(
        child,
        quote! { self.child_state },
        &child_states_name,
        &events_name,
        &events,
        &error_name,
        &child_lifecycle,
        quote! { true },
        TokenStream::new(),
        TokenStream::new(),
        quote! { self.context.child_transition_callback(&old_state, new_state); },
        &temporary_context_argument,
        has_deferred_events,
        async_queue,
    )?;
    let child_terminal = if child_states.contains_key("X") {
        quote! { matches!(self.child_state, #child_states_name::X) }
    } else {
        quote! { false }
    };
    let parent_completion = completion_code(
        parent,
        quote! { self.state },
        &parent_states_name,
        &events_name,
        &events,
        &error_name,
        &parent_lifecycle,
        child_terminal.clone(),
        exit_child_current.clone(),
        enter_child_current.clone(),
        quote! { self.context.transition_callback(&old_state, new_state); },
        &temporary_context_argument,
        has_deferred_events,
        async_queue,
    )?;
    let child_exception = exception_code(
        child,
        quote! { self.child_state },
        &child_states_name,
        &events_name,
        &error_name,
        &child_lifecycle,
        TokenStream::new(),
        TokenStream::new(),
        quote! { self.context.child_transition_callback(&old_state, new_state); },
        &temporary_context_argument,
        has_deferred_events,
        async_queue,
    )?;
    let parent_exception = exception_code(
        parent,
        quote! { self.state },
        &parent_states_name,
        &events_name,
        &error_name,
        &parent_lifecycle,
        exit_child_current.clone(),
        enter_child_current.clone(),
        quote! { self.context.transition_callback(&old_state, new_state); },
        &temporary_context_argument,
        has_deferred_events,
        async_queue,
    )?;
    let has_exception_handlers = [parent, child]
        .iter()
        .flat_map(|machine| &machine.transitions)
        .any(|transition| transition.event.kind == EventKind::Exception);
    let parent_initial_entry = parent_lifecycle
        .get(&parent_initial.to_string())
        .and_then(|hooks| hooks.entry.as_ref())
        .map(|actions| {
            if parent_state_types.contains_key(&parent_initial.to_string()) {
                let calls = action_calls(
                    actions,
                    callback_arguments(&temporary_context_argument, &[Some(quote! { state_data })]),
                    &error_name,
                );
                quote! {
                    if let #parent_states_name::#parent_initial(state_data) = &self.state { #calls }
                }
            } else {
                action_calls(
                    actions,
                    callback_arguments(&temporary_context_argument, &[]),
                    &error_name,
                )
            }
        })
        .unwrap_or_default();

    let dispatch_attempt = if has_exception_handlers {
        let attempt = if is_async_machine {
            quote! {
                async {
                    let mut handled = false;
                    if self.child_is_active() { #child_dispatch }
                    if !handled { #parent_dispatch }
                    Ok::<bool, #generated_error>(handled)
                }.await
            }
        } else {
            quote! {
                (|| -> Result<bool, #generated_error> {
                    let mut handled = false;
                    if self.child_is_active() { #child_dispatch }
                    if !handled { #parent_dispatch }
                    Ok(handled)
                })()
            }
        };
        let recover_guard = quote! {
            let error_data = &error;
            let mut exception_handled = false;
            if self.child_is_active() { #child_exception }
            #parent_exception
            if exception_handled {
                (true, true)
            } else {
                return Err(#error_name::GuardFailed(error));
            }
        };
        let recover_action = quote! {
            let error_data = &error;
            let mut exception_handled = false;
            if self.child_is_active() { #child_exception }
            #parent_exception
            if exception_handled {
                (true, true)
            } else {
                return Err(#error_name::ActionFailed(error));
            }
        };
        quote! {
            let dispatch_result = #attempt;
            let (handled, exception_recovered) = match dispatch_result {
                Ok(handled) => (handled, false),
                Err(#error_name::GuardFailed(error)) => { #recover_guard }
                Err(#error_name::ActionFailed(error)) => { #recover_action }
                Err(error) => return Err(error),
            };
        }
    } else {
        quote! {
            let mut handled = false;
            if self.child_is_active() { #child_dispatch }
            if !handled { #parent_dispatch }
            let exception_recovered = false;
        }
    };

    let process_event_body = if async_queue {
        quote! {
            self.pending.defer(event.into())
                .map_err(|_| #error_name::QueueFull)?;
            while let Some(event) = self.pending.pop() {
                self.context.log_process_event(&self.state, &event);
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
            Ok(&self.state)
        }
    } else {
        quote! {
            let event = event.into();
            self.context.log_process_event(&self.state, &event);
            #dispatch_attempt
            if handled {
                if exception_recovered {
                    self.stabilize(#temporary_context_call None)#await_stabilize?;
                } else {
                    self.stabilize(#temporary_context_call Some(&event))#await_stabilize?;
                }
                Ok(&self.state)
            } else {
                Err(#error_name::InvalidEvent)
            }
        }
    };

    Ok(quote! {
        /// Unified callbacks for the parent and its composite child.
        pub trait #context_name {
            #context_error
            #guards
            #actions
            fn log_process_event(&self, state: &#parent_states_name, event: &#events_name) {}
            fn log_guard(&self, guard: &'static str, result: bool) {}
            fn log_action(&self, action: &'static str) {}
            fn transition_callback(&self, old_state: &#parent_states_name, new_state: &#parent_states_name) {}
            fn child_transition_callback(&self, old_state: &#child_states_name, new_state: &#child_states_name) {}
        }

        #[allow(missing_docs)]
        pub enum #parent_states_name { #(#parent_state_variants),* }
        impl PartialEq for #parent_states_name {
            fn eq(&self, other: &Self) -> bool {
                core::mem::discriminant(self) == core::mem::discriminant(other)
            }
        }

        #[allow(missing_docs)]
        pub enum #child_states_name { #(#child_state_variants),* }
        impl PartialEq for #child_states_name {
            fn eq(&self, other: &Self) -> bool {
                core::mem::discriminant(self) == core::mem::discriminant(other)
            }
        }

        #[allow(missing_docs)]
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
            state: #parent_states_name,
            child_state: #child_states_name,
            context: T,
            #deferred_field
            #pending_field
        }

        impl<T: #context_name> #machine_name<T> {
            pub #new_const fn new(context: T) -> Self {
                Self {
                    state: #parent_states_name::#parent_initial #parent_initial_value,
                    child_state: #child_states_name::#child_initial #child_initial_value,
                    context,
                    #deferred_init
                    #pending_init
                }
            }

            pub #async_keyword fn initialize(&mut self, #temporary_context_parameter) -> Result<&#parent_states_name, #generated_error> {
                #parent_initial_entry
                self.stabilize(#temporary_context_call None)#await_stabilize?;
                Ok(&self.state)
            }

            #async_keyword fn stabilize(&mut self, #temporary_context_parameter origin: Option<&#events_name>) -> Result<(), #generated_error> {
                loop {
                    let mut handled = false;
                    if self.child_is_active() {
                        #child_completion
                    }
                    if !handled {
                        #parent_completion
                    }
                    if !handled { return Ok(()); }
                }
            }

            #[inline(always)]
            pub fn state(&self) -> &#parent_states_name { &self.state }

            #[inline(always)]
            pub fn child_state(&self) -> &#child_states_name { &self.child_state }

            pub fn set_state(&mut self, state: #parent_states_name) -> #parent_states_name {
                core::mem::replace(&mut self.state, state)
            }

            pub fn set_child_state(&mut self, state: #child_states_name) -> #child_states_name {
                core::mem::replace(&mut self.child_state, state)
            }

            #[inline(always)]
            pub fn visit_current_state<R>(
                &self,
                visitor: impl FnOnce(&#parent_states_name) -> R,
            ) -> R {
                visitor(&self.state)
            }

            #[inline(always)]
            pub fn visit_child_state<R>(
                &self,
                visitor: impl FnOnce(&#child_states_name) -> R,
            ) -> R {
                visitor(&self.child_state)
            }

            #[inline(always)]
            pub fn is(&self, expected: &#parent_states_name) -> bool { self.state == *expected }

            #[inline(always)]
            pub fn is_child(&self, expected: &#child_states_name) -> bool {
                self.child_state == *expected
            }

            #[inline(always)]
            pub fn child_is_active(&self) -> bool {
                matches!(self.state, #parent_states_name::#child_state_variant)
            }

            #[inline(always)]
            pub fn is_terminated(&self) -> bool {
                #parent_terminated
            }

            #[inline(always)]
            pub fn context(&self) -> &T { &self.context }

            #[inline(always)]
            pub fn context_mut(&mut self) -> &mut T { &mut self.context }

            pub #async_keyword fn process_event<EventInput>(
                &mut self,
                #temporary_context_parameter
                event: EventInput,
            ) -> Result<&#parent_states_name, #generated_error>
            where
                EventInput: Into<#events_name>,
            {
                #process_event_body
            }
        }

        impl<T: #context_name> ::sml::Terminated for #machine_name<T> {
            fn is_terminated(&self) -> bool { self.is_terminated() }
        }
    })
}

fn generate_multi_code(
    machines: &[StateMachine],
    parent: &StateMachine,
    child_references: &[&Ident],
) -> parse::Result<TokenStream> {
    let mut normalized_parent = parent.clone();
    for transition in &mut normalized_parent.transitions {
        if !transition
            .in_state
            .composite
            .as_ref()
            .is_some_and(|reference| child_references.contains(&reference))
        {
            transition.in_state.composite = None;
        }
        if !transition
            .out_state
            .composite
            .as_ref()
            .is_some_and(|reference| child_references.contains(&reference))
        {
            transition.out_state.composite = None;
        }
    }
    let known_names = machines
        .iter()
        .filter_map(|machine| machine.name.as_ref())
        .collect::<Vec<_>>();
    let parent_name_original = parent.name.as_ref().expect("named parent").clone();
    let mut queue = child_references
        .iter()
        .map(|reference| ((*reference).clone(), parent_name_original.clone(), 1usize))
        .collect::<std::collections::VecDeque<_>>();
    let mut normalized_children = Vec::new();
    let mut all_child_references = Vec::<Ident>::new();
    let mut node_parents = HashMap::<String, Ident>::new();
    let mut node_depths = HashMap::<String, usize>::new();
    while let Some((reference, node_parent, depth)) = queue.pop_front() {
        if all_child_references.contains(&reference) {
            return Err(parse::Error::new(
                reference.span(),
                "a composite table cannot be owned by multiple parents or form a cycle",
            ));
        }
        let original = machines
            .iter()
            .find(|machine| machine.name.as_ref() == Some(&reference))
            .ok_or_else(|| {
                parse::Error::new(
                    reference.span(),
                    format!("missing child table `{reference}`"),
                )
            })?;
        validate(&normalized_parent, original, &reference)?;
        let direct_children = original
            .transitions
            .iter()
            .flat_map(|transition| {
                transition
                    .in_state
                    .composite
                    .iter()
                    .chain(transition.out_state.composite.iter())
            })
            .filter(|candidate| known_names.contains(candidate))
            .fold(Vec::<Ident>::new(), |mut children, child| {
                if !children.contains(child) {
                    children.push(child.clone());
                }
                children
            });
        let mut child = original.clone();
        for transition in &mut child.transitions {
            if !transition
                .in_state
                .composite
                .as_ref()
                .is_some_and(|candidate| direct_children.contains(candidate))
            {
                transition.in_state.composite = None;
            }
            if !transition
                .out_state
                .composite
                .as_ref()
                .is_some_and(|candidate| direct_children.contains(candidate))
            {
                transition.out_state.composite = None;
            }
        }
        for direct_child in direct_children {
            queue.push_back((direct_child, reference.clone(), depth + 1));
        }
        node_parents.insert(reference.to_string(), node_parent);
        node_depths.insert(reference.to_string(), depth);
        all_child_references.push(reference);
        normalized_children.push(child);
    }
    let parent = &normalized_parent;
    let child_machines = normalized_children.iter().collect::<Vec<_>>();
    let mut all_machines = vec![parent];
    all_machines.extend(child_machines.iter().copied());

    let parent_name = parent.name.as_ref().expect("named parent");
    let root_orthogonal = parent
        .transitions
        .iter()
        .filter(|transition| transition.in_state.start)
        .count()
        > 1;
    let root_regions = root_orthogonal
        .then(|| crate::orthogonal_codegen::discover_regions(parent))
        .transpose()?;
    let parent_states_name = format_ident!("{}States", parent_name);
    let events_name = format_ident!("{}Events", parent_name);
    let context_name = format_ident!("{}StateMachineContext", parent_name);
    let machine_name = format_ident!("{}StateMachine", parent_name);
    let error_name = format_ident!("{}Error", parent_name);

    let is_async_machine = all_machines.iter().any(|machine| {
        machine.entry_exit_async
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
            })
    });
    let async_keyword = is_async_machine.then(|| quote! { async });
    let await_stabilize = is_async_machine.then(|| quote! { .await });
    let has_deferred_events = all_machines
        .iter()
        .flat_map(|machine| &machine.transitions)
        .any(|transition| transition.defer);
    let has_queue_actions = has_deferred_events
        || all_machines
            .iter()
            .flat_map(|machine| &machine.transitions)
            .any(|transition| !transition.process_events.is_empty());
    let async_queue = is_async_machine && has_queue_actions;

    let fixed_error_types = all_machines
        .iter()
        .filter_map(|machine| machine.fixed_error_type.as_ref())
        .collect::<Vec<_>>();
    if fixed_error_types
        .windows(2)
        .any(|errors| errors[0] != errors[1])
    {
        return Err(parse::Error::new(
            fixed_error_types[1].span(),
            "all tables in a composite tree must use the same exception error type",
        ));
    }
    let fixed_error_type = fixed_error_types.first().copied();
    let custom_error = all_machines.iter().any(|machine| machine.custom_error);
    let callback_error = if let Some(error) = fixed_error_type {
        quote! { #error }
    } else if custom_error {
        quote! { Self::Error }
    } else {
        quote! { () }
    };
    let context_error =
        (custom_error && fixed_error_type.is_none()).then(|| quote! { type Error; });
    let generated_error = if let Some(error) = fixed_error_type {
        quote! { #error_name<#error> }
    } else if custom_error {
        quote! { #error_name<T::Error> }
    } else {
        quote! { #error_name }
    };

    let temporary_types = all_machines
        .iter()
        .filter_map(|machine| machine.temporary_context_type.as_ref())
        .collect::<Vec<_>>();
    if temporary_types.windows(2).any(|types| types[0] != types[1]) {
        return Err(parse::Error::new(
            temporary_types[1].span(),
            "all tables in a composite tree must use the same temporary context type",
        ));
    }
    let temporary_context_type = temporary_types.first().copied();
    let temporary_context_parameter =
        temporary_context_type.map(|ty| quote! { temporary_context: #ty, });
    let temporary_context_argument = match temporary_context_type {
        Some(Type::Reference(reference)) if reference.mutability.is_some() => {
            Some(quote! { &mut *temporary_context })
        }
        Some(_) => Some(quote! { temporary_context }),
        None => None,
    };
    let temporary_context_call = temporary_context_argument
        .as_ref()
        .map(|argument| quote! { #argument, });

    let events = collect_events_many(&all_machines)?;
    let event_variants = events.values().map(|event| {
        let ident = &event.ident;
        event
            .ty
            .as_ref()
            .map_or_else(|| quote! { #ident }, |ty| quote! { #ident(#ty) })
    });
    let conversions = events.values().filter(|event| event.external).map(|event| {
        let ident = &event.ident;
        let ty = event.ty.as_ref().expect("external event type");
        quote! {
            impl From<#ty> for #events_name {
                fn from(event: #ty) -> Self { Self::#ident(event) }
            }
        }
    });
    let (guards, actions) = context_methods_many(
        &all_machines,
        &events,
        &callback_error,
        temporary_context_type,
    )?;

    let structural_children = child_references
        .iter()
        .map(|reference| (*reference).clone())
        .collect::<Vec<_>>();
    let parent_states = collect_states(parent);
    let parent_state_types = collect_state_types_multi(parent, &structural_children)?;
    let parent_state_variants = parent_states.values().map(|state| {
        parent_state_types
            .get(&state.to_string())
            .map_or_else(|| quote! { #state }, |ty| quote! { #state(#ty) })
    });
    let parent_initial = initial_state(parent)?.clone();
    let parent_initial_value = parent_state_types
        .contains_key(&parent_initial.to_string())
        .then(|| quote! { (core::default::Default::default()) });
    let parent_lifecycle = collect_lifecycle(parent);

    let mut child_enum_definitions = Vec::new();
    let mut child_fields = Vec::new();
    let mut child_initializers = Vec::new();
    let mut child_callback_methods = Vec::new();
    let mut child_api = Vec::new();
    let mut child_dispatches = Vec::<(usize, Option<usize>, TokenStream)>::new();
    let mut child_completions = Vec::<(usize, TokenStream)>::new();
    let mut child_exceptions = Vec::<(usize, TokenStream)>::new();
    let mut exit_arms = Vec::new();
    let mut entry_arms = Vec::new();
    let mut terminal_arms = Vec::new();

    for (node_index, (child, reference)) in
        child_machines.iter().zip(&all_child_references).enumerate()
    {
        let variant = crate::parser::state_ident(&reference.to_string(), reference.span());
        let child_states_name = format_ident!("{}{}States", parent_name, variant);
        let field = format_ident!(
            "{}_state",
            string_morph::to_snake_case(&reference.to_string())
        );
        let callback = format_ident!(
            "{}_transition_callback",
            string_morph::to_snake_case(&reference.to_string())
        );
        let direct_children = tree_children(reference, &all_child_references, &node_parents)
            .into_iter()
            .map(|(_, child)| child.clone())
            .collect::<Vec<_>>();
        let start_count = child
            .transitions
            .iter()
            .filter(|transition| transition.in_state.start)
            .count();
        if start_count > 1 {
            let active = tree_active_condition(
                parent_name,
                root_orthogonal,
                node_index,
                &child_machines,
                &all_child_references,
                &node_parents,
            );
            let embedded = tree_embedded_orthogonal(
                parent_name,
                node_index,
                &child_machines,
                &all_child_references,
                &node_parents,
                &events_name,
                &error_name,
                quote! { self.context.#callback(region_index, &old_state, new_state); },
                &temporary_context_argument,
                has_deferred_events,
                async_queue,
            )?;
            let region_count = embedded.region_count;
            let variants = &embedded.state_variants;
            let initials = &embedded.initial_values;
            let regions = crate::orthogonal_codegen::discover_regions(child)?;
            let dispatch_regions = embedded.dispatch_regions.clone();
            let region_dispatch =
                dispatch_regions
                    .iter()
                    .enumerate()
                    .map(|(region_index, dispatch)| {
                        let descendant_flags = all_child_references
                            .iter()
                            .filter(|candidate| *candidate != reference)
                            .filter(|candidate| {
                                tree_descendant_region(
                                    candidate,
                                    reference,
                                    &node_parents,
                                    &regions,
                                ) == Some(region_index)
                            })
                            .map(tree_handled_flag);
                        quote! {
                            {
                                let descendant_handled = false #(|| #descendant_flags)*;
                                handled = false;
                                if !descendant_handled { #dispatch }
                                node_handled |= descendant_handled || handled;
                            }
                        }
                    });
            let handled_flag = tree_handled_flag(reference);
            let dispatch = quote! {
                let outer_handled = handled;
                let mut node_handled = false;
                #(#region_dispatch)*
                handled = outer_handled || node_handled;
            };
            let completion = embedded.completion;
            let exception = embedded.exception;
            let enter = embedded.enter_current;
            let exit = embedded.exit_current;
            let terminal = embedded.terminal;
            child_enum_definitions.push(quote! {
                #[allow(missing_docs)]
                pub enum #child_states_name { #(#variants),* }
                impl PartialEq for #child_states_name {
                    fn eq(&self, other: &Self) -> bool {
                        core::mem::discriminant(self) == core::mem::discriminant(other)
                    }
                }
            });
            child_fields.push(quote! { #field: [#child_states_name; #region_count], });
            child_initializers.push(quote! { #field: [#(#initials),*], });
            child_callback_methods.push(quote! {
                fn #callback(
                    &self,
                    region: usize,
                    old_state: &#child_states_name,
                    new_state: &#child_states_name,
                ) {}
            });
            let states_method = format_ident!(
                "{}_states",
                string_morph::to_snake_case(&reference.to_string())
            );
            let state_method = format_ident!(
                "{}_state",
                string_morph::to_snake_case(&reference.to_string())
            );
            let active_method = format_ident!(
                "{}_is_active",
                string_morph::to_snake_case(&reference.to_string())
            );
            child_api.push(quote! {
                pub fn #states_method(&self) -> &[#child_states_name; #region_count] {
                    &self.#field
                }
                pub fn #state_method(&self, region: usize) -> Option<&#child_states_name> {
                    self.#field.get(region)
                }
                pub fn #active_method(&self) -> bool { #active }
            });
            let depth = node_depths[&reference.to_string()];
            let root_region = root_regions.as_ref().and_then(|regions| {
                tree_root_region(reference, parent_name, &node_parents, regions)
            });
            child_dispatches.push((
                depth,
                root_region,
                quote! {
                    let #handled_flag;
                    if #active {
                        #dispatch
                        #handled_flag = node_handled;
                    } else {
                        #handled_flag = false;
                    }
                },
            ));
            child_completions.push((depth, quote! { if !handled && #active { #completion } }));
            child_exceptions.push((
                depth,
                quote! {
                    if #active { #exception }
                },
            ));
            if node_parents.get(&reference.to_string()) == Some(parent_name) {
                exit_arms.push(quote! { #parent_states_name::#variant => { #exit } });
                entry_arms.push(quote! {
                    #parent_states_name::#variant => {
                        self.#field = [#(#initials),*];
                        #enter
                    }
                });
                terminal_arms.push(quote! { #parent_states_name::#variant => #terminal });
            }
            continue;
        }
        let child_states = collect_states(child);
        let child_state_types = collect_state_types_multi(child, &direct_children)?;
        let child_variants = child_states.values().map(|state| {
            child_state_types
                .get(&state.to_string())
                .map_or_else(|| quote! { #state }, |ty| quote! { #state(#ty) })
        });
        let child_initial = initial_state(child)?.clone();
        let child_initial_value = child_state_types
            .contains_key(&child_initial.to_string())
            .then(|| quote! { (core::default::Default::default()) });
        let lifecycle = collect_lifecycle(child);
        let exit_nested = tree_exit_children(
            parent_name,
            reference,
            &child_states_name,
            quote! { self.#field },
            &child_machines,
            &all_child_references,
            &node_parents,
            &error_name,
            &temporary_context_argument,
        )?;
        let enter_nested = tree_enter_children(
            parent_name,
            reference,
            &child_states_name,
            quote! { self.#field },
            &child_machines,
            &all_child_references,
            &node_parents,
            &error_name,
            &temporary_context_argument,
        )?;
        let dispatch = dispatch_code(
            child,
            quote! { self.#field },
            &child_states_name,
            &events_name,
            &error_name,
            &lifecycle,
            exit_nested.clone(),
            enter_nested.clone(),
            quote! { self.context.#callback(&old_state, new_state); },
            &temporary_context_argument,
            has_deferred_events,
            async_queue,
        )?;
        let completion = completion_code(
            child,
            quote! { self.#field },
            &child_states_name,
            &events_name,
            &events,
            &error_name,
            &lifecycle,
            tree_child_terminal(
                parent_name,
                reference,
                &child_states_name,
                quote! { self.#field },
                &child_machines,
                &all_child_references,
                &node_parents,
            ),
            exit_nested.clone(),
            enter_nested.clone(),
            quote! { self.context.#callback(&old_state, new_state); },
            &temporary_context_argument,
            has_deferred_events,
            async_queue,
        )?;
        let exception = exception_code(
            child,
            quote! { self.#field },
            &child_states_name,
            &events_name,
            &error_name,
            &lifecycle,
            exit_nested,
            enter_nested,
            quote! { self.context.#callback(&old_state, new_state); },
            &temporary_context_argument,
            has_deferred_events,
            async_queue,
        )?;
        let terminal = if child_states.contains_key("X") {
            quote! { matches!(self.#field, #child_states_name::X) }
        } else {
            quote! { false }
        };
        child_enum_definitions.push(quote! {
            #[allow(missing_docs)]
            pub enum #child_states_name { #(#child_variants),* }
            impl PartialEq for #child_states_name {
                fn eq(&self, other: &Self) -> bool {
                    core::mem::discriminant(self) == core::mem::discriminant(other)
                }
            }
        });
        child_fields.push(quote! { #field: #child_states_name, });
        child_initializers
            .push(quote! { #field: #child_states_name::#child_initial #child_initial_value, });
        child_callback_methods.push(quote! {
            fn #callback(&self, old_state: &#child_states_name, new_state: &#child_states_name) {}
        });
        let state_method = format_ident!(
            "{}_state",
            string_morph::to_snake_case(&reference.to_string())
        );
        let is_method = format_ident!("is_{}", string_morph::to_snake_case(&reference.to_string()));
        let set_method = format_ident!(
            "set_{}_state",
            string_morph::to_snake_case(&reference.to_string())
        );
        let visit_method = format_ident!(
            "visit_{}_state",
            string_morph::to_snake_case(&reference.to_string())
        );
        let active_method = format_ident!(
            "{}_is_active",
            string_morph::to_snake_case(&reference.to_string())
        );
        let active = tree_active_condition(
            parent_name,
            root_orthogonal,
            node_index,
            &child_machines,
            &all_child_references,
            &node_parents,
        );
        child_api.push(quote! {
            pub fn #state_method(&self) -> &#child_states_name { &self.#field }
            pub fn #is_method(&self, expected: &#child_states_name) -> bool {
                self.#field == *expected
            }
            pub fn #set_method(&mut self, state: #child_states_name) -> #child_states_name {
                core::mem::replace(&mut self.#field, state)
            }
            pub fn #visit_method<R>(
                &self,
                visitor: impl FnOnce(&#child_states_name) -> R,
            ) -> R {
                visitor(&self.#field)
            }
            pub fn #active_method(&self) -> bool {
                #active
            }
        });
        let depth = node_depths[&reference.to_string()];
        let root_region = root_regions
            .as_ref()
            .and_then(|regions| tree_root_region(reference, parent_name, &node_parents, regions));
        let handled_flag = tree_handled_flag(reference);
        child_dispatches.push((
            depth,
            root_region,
            quote! {
                let outer_handled = handled;
                handled = false;
                if #active { #dispatch }
                let #handled_flag = handled;
                handled |= outer_handled;
            },
        ));
        child_completions.push((
            depth,
            quote! {
                if !handled && #active { #completion }
            },
        ));
        child_exceptions.push((
            depth,
            quote! {
                if !exception_handled && #active { #exception }
            },
        ));
        if node_parents.get(&reference.to_string()) == Some(parent_name) {
            let enter = tree_enter_node(
                parent_name,
                node_index,
                &child_machines,
                &all_child_references,
                &node_parents,
                &error_name,
                &temporary_context_argument,
            )?;
            let exit = tree_exit_node(
                parent_name,
                node_index,
                &child_machines,
                &all_child_references,
                &node_parents,
                &error_name,
                &temporary_context_argument,
            )?;
            exit_arms.push(quote! { #parent_states_name::#variant => { #exit } });
            entry_arms.push(quote! { #parent_states_name::#variant => { #enter } });
            terminal_arms.push(quote! { #parent_states_name::#variant => #terminal });
        }
    }

    child_dispatches.sort_by_key(|(depth, _, _)| core::cmp::Reverse(*depth));
    child_completions.sort_by_key(|(depth, _)| core::cmp::Reverse(*depth));
    child_exceptions.sort_by_key(|(depth, _)| core::cmp::Reverse(*depth));
    let child_dispatches = child_dispatches
        .into_iter()
        .map(|(_, region, tokens)| (region, tokens))
        .collect::<Vec<_>>();
    let child_completions = child_completions
        .into_iter()
        .map(|(_, tokens)| tokens)
        .collect::<Vec<_>>();
    let child_exceptions = child_exceptions
        .into_iter()
        .map(|(_, tokens)| tokens)
        .collect::<Vec<_>>();

    let scalar_exit_children = quote! {
        match &self.state { #(#exit_arms,)* _ => {} }
    };
    let scalar_enter_children = quote! {
        match &self.state { #(#entry_arms,)* _ => {} }
    };
    let scalar_child_terminal = quote! {
        match &self.state { #(#terminal_arms,)* _ => false }
    };
    let scalar_parent_dispatch = (!root_orthogonal)
        .then(|| {
            dispatch_code(
                parent,
                quote! { self.state },
                &parent_states_name,
                &events_name,
                &error_name,
                &parent_lifecycle,
                scalar_exit_children.clone(),
                scalar_enter_children.clone(),
                quote! { self.context.transition_callback(&old_state, new_state); },
                &temporary_context_argument,
                has_deferred_events,
                async_queue,
            )
        })
        .transpose()?
        .unwrap_or_default();
    let scalar_parent_completion = (!root_orthogonal)
        .then(|| {
            completion_code(
                parent,
                quote! { self.state },
                &parent_states_name,
                &events_name,
                &events,
                &error_name,
                &parent_lifecycle,
                scalar_child_terminal.clone(),
                scalar_exit_children.clone(),
                scalar_enter_children.clone(),
                quote! { self.context.transition_callback(&old_state, new_state); },
                &temporary_context_argument,
                has_deferred_events,
                async_queue,
            )
        })
        .transpose()?
        .unwrap_or_default();
    let scalar_parent_exception = (!root_orthogonal)
        .then(|| {
            exception_code(
                parent,
                quote! { self.state },
                &parent_states_name,
                &events_name,
                &error_name,
                &parent_lifecycle,
                scalar_exit_children.clone(),
                scalar_enter_children.clone(),
                quote! { self.context.transition_callback(&old_state, new_state); },
                &temporary_context_argument,
                has_deferred_events,
                async_queue,
            )
        })
        .transpose()?
        .unwrap_or_default();
    let scalar_parent_initial_entry = parent_lifecycle
        .get(&parent_initial.to_string())
        .and_then(|hooks| hooks.entry.as_ref())
        .map(|actions| {
            let state = parent_state_types
                .contains_key(&parent_initial.to_string())
                .then(|| quote! { state_data });
            let calls = action_calls(
                actions,
                callback_arguments(&temporary_context_argument, &[state]),
                &error_name,
            );
            if parent_state_types.contains_key(&parent_initial.to_string()) {
                quote! {
                    if let #parent_states_name::#parent_initial(state_data) = &self.state { #calls }
                }
            } else {
                calls
            }
        })
        .unwrap_or_default();

    let root_exit_children = quote! {
        match &self.states[region_index] { #(#exit_arms,)* _ => {} }
    };
    let root_enter_children = quote! {
        match &self.states[region_index] { #(#entry_arms,)* _ => {} }
    };
    let root_child_terminal = quote! {
        match &self.states[region_index] { #(#terminal_arms,)* _ => false }
    };
    let root_embedded = if root_orthogonal {
        Some(crate::orthogonal_codegen::generate_embedded(
            parent,
            &parent_states_name,
            &events_name,
            &error_name,
            quote! { self.states },
            quote! { self.context.transition_callback(region_index, &old_state, new_state); },
            &structural_children,
            root_exit_children,
            root_enter_children,
            root_child_terminal,
            &temporary_context_argument,
            has_deferred_events,
            async_queue,
        )?)
    } else {
        None
    };
    let (parent_completion, parent_exception, parent_initial_entry) =
        root_embedded.as_ref().map_or_else(
            || {
                (
                    scalar_parent_completion,
                    scalar_parent_exception,
                    scalar_parent_initial_entry,
                )
            },
            |embedded| {
                (
                    embedded.completion.clone(),
                    embedded.exception.clone(),
                    embedded.enter_current.clone(),
                )
            },
        );
    let parent_dispatch =
        if let Some(embedded) = &root_embedded {
            let region_dispatches = embedded.dispatch_regions.iter().enumerate().map(
                |(index, parent_region_dispatch)| {
                    let descendants = child_dispatches
                        .iter()
                        .filter(|(region, _)| *region == Some(index))
                        .map(|(_, tokens)| tokens);
                    quote! {
                        {
                            let mut handled = false;
                            #(#descendants)*
                            let descendant_handled = handled;
                            handled = false;
                            if !descendant_handled { #parent_region_dispatch }
                            any_handled |= descendant_handled || handled;
                        }
                    }
                },
            );
            quote! {
                let mut any_handled = false;
                #(#region_dispatches)*
                handled = any_handled;
            }
        } else {
            let descendants = child_dispatches.iter().map(|(_, tokens)| tokens);
            quote! { #(#descendants)* if !handled { #scalar_parent_dispatch } }
        };

    let has_exception_handlers = all_machines
        .iter()
        .flat_map(|machine| &machine.transitions)
        .any(|transition| transition.event.kind == EventKind::Exception);
    let dispatch_attempt = if has_exception_handlers {
        let attempt = if is_async_machine {
            quote! {
                async {
                    let mut handled = false;
                    #parent_dispatch
                    Ok::<bool, #generated_error>(handled)
                }.await
            }
        } else {
            quote! {
                (|| -> Result<bool, #generated_error> {
                    let mut handled = false;
                    #parent_dispatch
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
                    #(#child_exceptions)*
                    #parent_exception
                    if exception_handled { (true, true) }
                    else { return Err(#error_name::GuardFailed(error)); }
                }
                Err(#error_name::ActionFailed(error)) => {
                    let error_data = &error;
                    let mut exception_handled = false;
                    #(#child_exceptions)*
                    #parent_exception
                    if exception_handled { (true, true) }
                    else { return Err(#error_name::ActionFailed(error)); }
                }
                Err(error) => return Err(error),
            };
        }
    } else {
        quote! {
            let mut handled = false;
            #parent_dispatch
            let exception_recovered = false;
        }
    };
    let root_region_count = root_embedded.as_ref().map(|embedded| embedded.region_count);
    let root_initial_values = root_embedded
        .as_ref()
        .map(|embedded| embedded.initial_values.clone())
        .unwrap_or_default();
    let log_state = if root_orthogonal {
        quote! { &self.states }
    } else {
        quote! { &self.state }
    };
    let return_state = log_state.clone();
    let process_event_body = if async_queue {
        quote! {
            self.pending.defer(event.into()).map_err(|_| #error_name::QueueFull)?;
            while let Some(event) = self.pending.pop() {
                self.context.log_process_event(#log_state, &event);
                #dispatch_attempt
                if !handled { return Err(#error_name::InvalidEvent); }
                if exception_recovered {
                    self.stabilize(#temporary_context_call None).await?;
                } else {
                    self.stabilize(#temporary_context_call Some(&event)).await?;
                }
            }
            Ok(#return_state)
        }
    } else {
        quote! {
            let event = event.into();
            self.context.log_process_event(#log_state, &event);
            #dispatch_attempt
            if !handled { return Err(#error_name::InvalidEvent); }
            if exception_recovered {
                self.stabilize(#temporary_context_call None)#await_stabilize?;
            } else {
                self.stabilize(#temporary_context_call Some(&event))#await_stabilize?;
            }
            Ok(#return_state)
        }
    };

    let parent_terminated = root_embedded.as_ref().map_or_else(
        || {
            if parent_states.contains_key("X") {
                quote! { matches!(self.state, #parent_states_name::X) }
            } else {
                quote! { false }
            }
        },
        |embedded| embedded.terminal.clone(),
    );
    let parent_states_attr = &parent.states_attr;
    let parent_events_attr = &parent.events_attr;
    let deferred_field = has_deferred_events.then(|| {
        quote! { deferred: ::sml::utility::EventQueue<#events_name, 16>, }
    });
    let deferred_init =
        has_deferred_events.then(|| quote! { deferred: ::sml::utility::EventQueue::new(), });
    let pending_field = async_queue.then(|| {
        quote! { pending: ::sml::utility::EventQueue<#events_name, 16>, }
    });
    let pending_init = async_queue.then(|| quote! { pending: ::sml::utility::EventQueue::new(), });
    let new_const = (parent_state_types.is_empty()
        && child_machines
            .iter()
            .all(|child| collect_state_types(child, None).is_ok_and(|types| types.is_empty())))
    .then(|| quote! { const });
    let context_root_methods = if let Some(region_count) = root_region_count {
        quote! {
            fn log_process_event(&self, states: &[#parent_states_name; #region_count], event: &#events_name) {}
            fn transition_callback(&self, region: usize, old_state: &#parent_states_name, new_state: &#parent_states_name) {}
        }
    } else {
        quote! {
            fn log_process_event(&self, state: &#parent_states_name, event: &#events_name) {}
            fn transition_callback(&self, old_state: &#parent_states_name, new_state: &#parent_states_name) {}
        }
    };
    let root_state_field = if let Some(region_count) = root_region_count {
        quote! { states: [#parent_states_name; #region_count], }
    } else {
        quote! { state: #parent_states_name, }
    };
    let root_state_init = if root_orthogonal {
        quote! { states: [#(#root_initial_values),*], }
    } else {
        quote! { state: #parent_states_name::#parent_initial #parent_initial_value, }
    };
    let initialize_method = if let Some(region_count) = root_region_count {
        quote! {
            pub #async_keyword fn initialize(&mut self, #temporary_context_parameter) -> Result<&[#parent_states_name; #region_count], #generated_error> {
                #parent_initial_entry
                self.stabilize(#temporary_context_call None)#await_stabilize?;
                Ok(&self.states)
            }
        }
    } else {
        quote! {
            pub #async_keyword fn initialize(&mut self, #temporary_context_parameter) -> Result<&#parent_states_name, #generated_error> {
                #parent_initial_entry
                #scalar_enter_children
                self.stabilize(#temporary_context_call None)#await_stabilize?;
                Ok(&self.state)
            }
        }
    };
    let root_state_api = if let Some(region_count) = root_region_count {
        quote! {
            pub fn states(&self) -> &[#parent_states_name; #region_count] { &self.states }
            pub fn state(&self, region: usize) -> Option<&#parent_states_name> { self.states.get(region) }
            pub fn is(&self, expected: &[#parent_states_name; #region_count]) -> bool { self.states == *expected }
            pub fn is_region(&self, region: usize, expected: &#parent_states_name) -> bool {
                self.states.get(region).is_some_and(|state| state == expected)
            }
        }
    } else {
        quote! {
            pub fn state(&self) -> &#parent_states_name { &self.state }
            pub fn set_state(&mut self, state: #parent_states_name) -> #parent_states_name {
                core::mem::replace(&mut self.state, state)
            }
            pub fn is(&self, expected: &#parent_states_name) -> bool { self.state == *expected }
        }
    };
    let process_return = if let Some(region_count) = root_region_count {
        quote! { &[#parent_states_name; #region_count] }
    } else {
        quote! { &#parent_states_name }
    };

    Ok(quote! {
        pub trait #context_name {
            #context_error
            #guards
            #actions
            #context_root_methods
            fn log_guard(&self, guard: &'static str, result: bool) {}
            fn log_action(&self, action: &'static str) {}
            #(#child_callback_methods)*
        }

        #[allow(missing_docs)]
        #(#parent_states_attr)*
        pub enum #parent_states_name { #(#parent_state_variants),* }
        impl PartialEq for #parent_states_name {
            fn eq(&self, other: &Self) -> bool {
                core::mem::discriminant(self) == core::mem::discriminant(other)
            }
        }
        #(#child_enum_definitions)*

        #[allow(missing_docs)]
        #(#parent_events_attr)*
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
            #root_state_field
            #(#child_fields)*
            context: T,
            #deferred_field
            #pending_field
        }

        impl<T: #context_name> #machine_name<T> {
            pub #new_const fn new(context: T) -> Self {
                Self {
                    #root_state_init
                    #(#child_initializers)*
                    context,
                    #deferred_init
                    #pending_init
                }
            }

            #initialize_method

            #async_keyword fn stabilize(&mut self, #temporary_context_parameter origin: Option<&#events_name>) -> Result<(), #generated_error> {
                loop {
                    let mut handled = false;
                    #(#child_completions)*
                    if !handled { #parent_completion }
                    if !handled { return Ok(()); }
                }
            }

            #root_state_api
            #(#child_api)*
            pub fn is_terminated(&self) -> bool { #parent_terminated }
            pub fn context(&self) -> &T { &self.context }
            pub fn context_mut(&mut self) -> &mut T { &mut self.context }

            pub #async_keyword fn process_event<EventInput>(
                &mut self,
                #temporary_context_parameter
                event: EventInput,
            ) -> Result<#process_return, #generated_error>
            where EventInput: Into<#events_name>
            {
                #process_event_body
            }
        }

        impl<T: #context_name> ::sml::Terminated for #machine_name<T> {
            fn is_terminated(&self) -> bool { self.is_terminated() }
        }
    })
}

fn tree_states_name(root: &Ident, node: &Ident) -> Ident {
    let variant = crate::parser::state_ident(&node.to_string(), node.span());
    format_ident!("{}{}States", root, variant)
}

fn tree_field(node: &Ident) -> Ident {
    format_ident!("{}_state", string_morph::to_snake_case(&node.to_string()))
}

fn tree_place(node: &Ident) -> TokenStream {
    let field = tree_field(node);
    quote! { self.#field }
}

fn tree_children<'a>(
    owner: &Ident,
    references: &'a [Ident],
    parents: &HashMap<String, Ident>,
) -> Vec<(usize, &'a Ident)> {
    references
        .iter()
        .enumerate()
        .filter(|(_, reference)| parents.get(&reference.to_string()) == Some(owner))
        .collect()
}

#[allow(clippy::too_many_arguments)]
fn tree_embedded_orthogonal(
    root: &Ident,
    index: usize,
    nodes: &[&StateMachine],
    references: &[Ident],
    parents: &HashMap<String, Ident>,
    events_name: &Ident,
    error_name: &Ident,
    callback: TokenStream,
    temporary_context: &Option<TokenStream>,
    has_deferred_events: bool,
    async_queue: bool,
) -> parse::Result<crate::orthogonal_codegen::EmbeddedOrthogonal> {
    let node = nodes[index];
    let reference = &references[index];
    let states_name = tree_states_name(root, reference);
    let field = tree_field(reference);
    let direct = tree_children(reference, references, parents);
    let direct_children = direct
        .iter()
        .map(|(_, child)| (*child).clone())
        .collect::<Vec<_>>();
    let mut exit_arms = Vec::new();
    let mut entry_arms = Vec::new();
    for (child_index, child) in direct {
        let variant = crate::parser::state_ident(&child.to_string(), child.span());
        let exit = tree_exit_node(
            root,
            child_index,
            nodes,
            references,
            parents,
            error_name,
            temporary_context,
        )?;
        let enter = tree_enter_node(
            root,
            child_index,
            nodes,
            references,
            parents,
            error_name,
            temporary_context,
        )?;
        exit_arms.push(quote! { #states_name::#variant => { #exit } });
        entry_arms.push(quote! { #states_name::#variant => { #enter } });
    }
    let exit = quote! {
        match &self.#field[region_index] { #(#exit_arms,)* _ => {} }
    };
    let entry = quote! {
        match &self.#field[region_index] { #(#entry_arms,)* _ => {} }
    };
    let terminal = tree_child_terminal(
        root,
        reference,
        &states_name,
        quote! { self.#field[region_index] },
        nodes,
        references,
        parents,
    );
    crate::orthogonal_codegen::generate_embedded(
        node,
        &states_name,
        events_name,
        error_name,
        quote! { self.#field },
        callback,
        &direct_children,
        exit,
        entry,
        terminal,
        temporary_context,
        has_deferred_events,
        async_queue,
    )
}

fn tree_root_region(
    node: &Ident,
    root: &Ident,
    parents: &HashMap<String, Ident>,
    regions: &[crate::orthogonal_codegen::Region],
) -> Option<usize> {
    let mut top = node;
    while let Some(parent) = parents.get(&top.to_string()) {
        if parent == root {
            let variant = crate::parser::state_ident(&top.to_string(), top.span()).to_string();
            return regions
                .iter()
                .position(|region| region.states.contains(&variant));
        }
        top = parent;
    }
    None
}

fn tree_handled_flag(node: &Ident) -> Ident {
    format_ident!("{}_handled", string_morph::to_snake_case(&node.to_string()))
}

fn tree_descendant_region(
    node: &Ident,
    orthogonal_owner: &Ident,
    parents: &HashMap<String, Ident>,
    regions: &[crate::orthogonal_codegen::Region],
) -> Option<usize> {
    let mut top = node;
    while let Some(parent) = parents.get(&top.to_string()) {
        if parent == orthogonal_owner {
            let variant = crate::parser::state_ident(&top.to_string(), top.span()).to_string();
            return regions
                .iter()
                .position(|region| region.states.contains(&variant));
        }
        top = parent;
    }
    None
}

#[allow(clippy::too_many_arguments)]
fn tree_enter_children(
    root: &Ident,
    owner: &Ident,
    owner_states_name: &Ident,
    owner_place: TokenStream,
    nodes: &[&StateMachine],
    references: &[Ident],
    parents: &HashMap<String, Ident>,
    error_name: &Ident,
    temporary_context: &Option<TokenStream>,
) -> parse::Result<TokenStream> {
    let arms = tree_children(owner, references, parents)
        .into_iter()
        .map(|(index, reference)| {
            let variant = crate::parser::state_ident(&reference.to_string(), reference.span());
            let enter = tree_enter_node(
                root,
                index,
                nodes,
                references,
                parents,
                error_name,
                temporary_context,
            )?;
            Ok(quote! { #owner_states_name::#variant => { #enter } })
        })
        .collect::<parse::Result<Vec<_>>>()?;
    Ok(quote! { match &#owner_place { #(#arms,)* _ => {} } })
}

#[allow(clippy::too_many_arguments)]
fn tree_enter_node(
    root: &Ident,
    index: usize,
    nodes: &[&StateMachine],
    references: &[Ident],
    parents: &HashMap<String, Ident>,
    error_name: &Ident,
    temporary_context: &Option<TokenStream>,
) -> parse::Result<TokenStream> {
    let node = nodes[index];
    let reference = &references[index];
    let states_name = tree_states_name(root, reference);
    let field = tree_field(reference);
    if node
        .transitions
        .iter()
        .filter(|transition| transition.in_state.start)
        .count()
        > 1
    {
        let events_name = format_ident!("{}Events", root);
        let embedded = tree_embedded_orthogonal(
            root,
            index,
            nodes,
            references,
            parents,
            &events_name,
            error_name,
            TokenStream::new(),
            temporary_context,
            false,
            false,
        )?;
        let initials = embedded.initial_values;
        let enter = embedded.enter_current;
        return Ok(quote! {
            self.#field = [#(#initials),*];
            #enter
        });
    }
    let direct_children = tree_children(reference, references, parents)
        .into_iter()
        .map(|(_, child)| child.clone())
        .collect::<Vec<_>>();
    let state_types = collect_state_types_multi(node, &direct_children)?;
    let lifecycle = collect_lifecycle(node);
    let entry_hook = current_lifecycle_code(
        &lifecycle,
        true,
        &states_name,
        quote! { self.#field },
        &state_types,
        error_name,
        temporary_context,
    );
    let enter_descendant = tree_enter_children(
        root,
        reference,
        &states_name,
        quote! { self.#field },
        nodes,
        references,
        parents,
        error_name,
        temporary_context,
    )?;
    let history = node
        .transitions
        .iter()
        .any(|transition| transition.in_state.history);
    let reset = if history {
        TokenStream::new()
    } else {
        let initial = initial_state(node)?;
        let value = state_types
            .contains_key(&initial.to_string())
            .then(|| quote! { (core::default::Default::default()) });
        quote! { self.#field = #states_name::#initial #value; }
    };
    Ok(quote! { #reset #entry_hook #enter_descendant })
}

#[allow(clippy::too_many_arguments)]
fn tree_exit_children(
    root: &Ident,
    owner: &Ident,
    owner_states_name: &Ident,
    owner_place: TokenStream,
    nodes: &[&StateMachine],
    references: &[Ident],
    parents: &HashMap<String, Ident>,
    error_name: &Ident,
    temporary_context: &Option<TokenStream>,
) -> parse::Result<TokenStream> {
    let arms = tree_children(owner, references, parents)
        .into_iter()
        .map(|(index, reference)| {
            let variant = crate::parser::state_ident(&reference.to_string(), reference.span());
            let exit = tree_exit_node(
                root,
                index,
                nodes,
                references,
                parents,
                error_name,
                temporary_context,
            )?;
            Ok(quote! { #owner_states_name::#variant => { #exit } })
        })
        .collect::<parse::Result<Vec<_>>>()?;
    Ok(quote! { match &#owner_place { #(#arms,)* _ => {} } })
}

#[allow(clippy::too_many_arguments)]
fn tree_exit_node(
    root: &Ident,
    index: usize,
    nodes: &[&StateMachine],
    references: &[Ident],
    parents: &HashMap<String, Ident>,
    error_name: &Ident,
    temporary_context: &Option<TokenStream>,
) -> parse::Result<TokenStream> {
    let node = nodes[index];
    let reference = &references[index];
    let states_name = tree_states_name(root, reference);
    let field = tree_field(reference);
    if node
        .transitions
        .iter()
        .filter(|transition| transition.in_state.start)
        .count()
        > 1
    {
        let events_name = format_ident!("{}Events", root);
        let embedded = tree_embedded_orthogonal(
            root,
            index,
            nodes,
            references,
            parents,
            &events_name,
            error_name,
            TokenStream::new(),
            temporary_context,
            false,
            false,
        )?;
        return Ok(embedded.exit_current);
    }
    let direct_children = tree_children(reference, references, parents)
        .into_iter()
        .map(|(_, child)| child.clone())
        .collect::<Vec<_>>();
    let state_types = collect_state_types_multi(node, &direct_children)?;
    let lifecycle = collect_lifecycle(node);
    let exit_descendant = tree_exit_children(
        root,
        reference,
        &states_name,
        quote! { self.#field },
        nodes,
        references,
        parents,
        error_name,
        temporary_context,
    )?;
    let exit_hook = current_lifecycle_code(
        &lifecycle,
        false,
        &states_name,
        quote! { self.#field },
        &state_types,
        error_name,
        temporary_context,
    );
    Ok(quote! { #exit_descendant #exit_hook })
}

fn tree_active_condition(
    root: &Ident,
    root_orthogonal: bool,
    index: usize,
    nodes: &[&StateMachine],
    references: &[Ident],
    parents: &HashMap<String, Ident>,
) -> TokenStream {
    let mut conditions = Vec::new();
    let mut current = &references[index];
    while let Some(parent) = parents.get(&current.to_string()) {
        let parent_states = if parent == root {
            format_ident!("{}States", root)
        } else {
            tree_states_name(root, parent)
        };
        let parent_orthogonal = if parent == root {
            root_orthogonal
        } else {
            references
                .iter()
                .position(|reference| reference == parent)
                .is_some_and(|index| {
                    nodes[index]
                        .transitions
                        .iter()
                        .filter(|transition| transition.in_state.start)
                        .count()
                        > 1
                })
        };
        let parent_place = if parent == root {
            if parent_orthogonal {
                quote! { self.states }
            } else {
                quote! { self.state }
            }
        } else {
            tree_place(parent)
        };
        let variant = crate::parser::state_ident(&current.to_string(), current.span());
        if parent_orthogonal {
            conditions.push(quote! {
                #parent_place.iter().any(|state| matches!(state, #parent_states::#variant))
            });
        } else {
            conditions.push(quote! { matches!(#parent_place, #parent_states::#variant) });
        }
        if parent == root {
            break;
        }
        current = parent;
    }
    quote! { true #(&& #conditions)* }
}

fn tree_child_terminal(
    root: &Ident,
    owner: &Ident,
    owner_states_name: &Ident,
    owner_place: TokenStream,
    nodes: &[&StateMachine],
    references: &[Ident],
    parents: &HashMap<String, Ident>,
) -> TokenStream {
    let arms = tree_children(owner, references, parents)
        .into_iter()
        .map(|(index, reference)| {
            let variant = crate::parser::state_ident(&reference.to_string(), reference.span());
            let states_name = tree_states_name(root, reference);
            let place = tree_place(reference);
            let terminal = if collect_states(nodes[index]).contains_key("X") {
                if nodes[index]
                    .transitions
                    .iter()
                    .filter(|transition| transition.in_state.start)
                    .count()
                    > 1
                {
                    quote! { #place.iter().all(|state| matches!(state, #states_name::X)) }
                } else {
                    quote! { matches!(#place, #states_name::X) }
                }
            } else {
                quote! { false }
            };
            quote! { #owner_states_name::#variant => #terminal }
        })
        .collect::<Vec<_>>();
    quote! { match &#owner_place { #(#arms,)* _ => false } }
}

fn has_composite(machine: &StateMachine, machine_names: &[&Ident]) -> bool {
    machine.transitions.iter().any(|transition| {
        transition
            .in_state
            .composite
            .as_ref()
            .or(transition.out_state.composite.as_ref())
            .is_some_and(|reference| machine_names.contains(&reference))
    })
}

fn validate(
    parent: &StateMachine,
    child: &StateMachine,
    _child_reference: &Ident,
) -> parse::Result<()> {
    for machine in [parent, child] {
        let starts = machine
            .transitions
            .iter()
            .filter(|t| t.in_state.start)
            .count();
        if starts == 0 {
            return Err(parse::Error::new(
                Span::call_site(),
                "each table in a composite tree requires at least one initial state",
            ));
        }
        for transition in &machine.transitions {
            if transition.event.kind == EventKind::Completion && transition.internal_transition {
                return Err(parse::Error::new(
                    transition.in_state.ident.span(),
                    "an anonymous composite completion must leave its source state",
                ));
            }
        }
    }
    Ok(())
}

fn collect_states(machine: &StateMachine) -> BTreeMap<String, Ident> {
    let mut states = BTreeMap::new();
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
    states
}

fn collect_state_types(
    machine: &StateMachine,
    structural_child: Option<&Ident>,
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
            if composite.is_some_and(|child| structural_child == Some(child)) {
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

fn collect_state_types_multi(
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

fn initial_state(machine: &StateMachine) -> parse::Result<&Ident> {
    machine
        .transitions
        .iter()
        .find(|t| t.in_state.start)
        .map(|t| &t.in_state.ident)
        .ok_or_else(|| parse::Error::new(Span::call_site(), "missing initial state"))
}

struct EventInfo {
    ident: Ident,
    ty: Option<Type>,
    external: bool,
}

fn collect_events(
    parent: &StateMachine,
    child: &StateMachine,
) -> parse::Result<BTreeMap<String, EventInfo>> {
    collect_events_many(&[parent, child])
}

fn collect_events_many(machines: &[&StateMachine]) -> parse::Result<BTreeMap<String, EventInfo>> {
    let mut events = BTreeMap::<String, EventInfo>::new();
    for transition in machines.iter().flat_map(|machine| &machine.transitions) {
        if matches!(
            transition.event.kind,
            EventKind::Entry | EventKind::Exit | EventKind::Exception
        ) || transition.event.wildcard
            || transition.event.kind == EventKind::Completion
        {
            continue;
        }
        let key = transition.event.ident.to_string();
        let candidate = EventInfo {
            ident: transition.event.ident.clone(),
            ty: transition.event.data_type.clone(),
            external: transition.event.external,
        };
        if let Some(existing) = events.get(&key) {
            if existing.ty != candidate.ty || existing.external != candidate.external {
                return Err(parse::Error::new(
                    transition.event.ident.span(),
                    format!("event `{key}` has incompatible definitions in parent and child"),
                ));
            }
        } else {
            events.insert(key, candidate);
        }
    }
    Ok(events)
}

fn context_methods(
    parent: &StateMachine,
    child: &StateMachine,
    events: &BTreeMap<String, EventInfo>,
    error_type: &TokenStream,
    temporary_context_type: Option<&Type>,
) -> parse::Result<(TokenStream, TokenStream)> {
    context_methods_many(&[parent, child], events, error_type, temporary_context_type)
}

fn context_methods_many(
    machines: &[&StateMachine],
    events: &BTreeMap<String, EventInfo>,
    error_type: &TokenStream,
    temporary_context_type: Option<&Type>,
) -> parse::Result<(TokenStream, TokenStream)> {
    let mut guards = BTreeMap::new();
    let mut actions = BTreeMap::new();
    for transition in machines.iter().flat_map(|machine| &machine.transitions) {
        let event_ty = transition.event.data_type.as_ref().or_else(|| {
            (transition.event.kind == EventKind::Completion && !transition.event.wildcard)
                .then(|| {
                    events
                        .get(&transition.event.ident.to_string())
                        .and_then(|event| event.ty.as_ref())
                })
                .flatten()
        });
        let state_ty = transition
            .in_state
            .composite
            .is_none()
            .then_some(transition.in_state.data_type.as_ref())
            .flatten();
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
                insert_unique(&mut guards, ident, signature)
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
                && transition.out_state.composite.is_none()
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

fn current_lifecycle_code(
    lifecycle: &HashMap<String, Lifecycle>,
    entry: bool,
    states_name: &Ident,
    state_place: TokenStream,
    state_types: &BTreeMap<String, Type>,
    error_name: &Ident,
    temporary_context: &Option<TokenStream>,
) -> TokenStream {
    let arms = lifecycle.iter().filter_map(|(state, hooks)| {
        let actions = if entry {
            hooks.entry.as_ref()
        } else {
            hooks.exit.as_ref()
        }?;
        let state = format_ident!("{}", state);
        if state_types.contains_key(&state.to_string()) {
            let calls = action_calls(
                actions,
                callback_arguments(temporary_context, &[Some(quote! { state_data })]),
                error_name,
            );
            Some(quote! { #states_name::#state(state_data) => { #calls } })
        } else {
            let calls = action_calls(
                actions,
                callback_arguments(temporary_context, &[]),
                error_name,
            );
            Some(quote! { #states_name::#state => { #calls } })
        }
    });
    quote! {
        match &#state_place {
            #(#arms,)*
            _ => {}
        }
    }
}

fn insert_unique(
    map: &mut BTreeMap<String, TokenStream>,
    ident: &Ident,
    signature: TokenStream,
) -> parse::Result<()> {
    if let Some(existing) = map.get(&ident.to_string()) {
        if existing.to_string() != signature.to_string() {
            return Err(parse::Error::new(
                ident.span(),
                format!("callback `{ident}` has incompatible uses"),
            ));
        }
    } else {
        map.insert(ident.to_string(), signature);
    }
    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn dispatch_code(
    machine: &StateMachine,
    state_place: TokenStream,
    states_name: &Ident,
    events_name: &Ident,
    error_name: &Ident,
    lifecycle: &HashMap<String, Lifecycle>,
    composite_exit: TokenStream,
    composite_entry: TokenStream,
    callback: TokenStream,
    temporary_context: &Option<TokenStream>,
    has_deferred_events: bool,
    async_queue: bool,
) -> parse::Result<TokenStream> {
    let mut grouped = HashMap::<(String, String), Vec<&StateTransition>>::new();
    for transition in machine
        .transitions
        .iter()
        .filter(|transition| transition.event.kind == EventKind::Normal)
    {
        grouped
            .entry((
                transition.in_state.ident.to_string(),
                transition.event.ident.to_string(),
            ))
            .or_default()
            .push(transition);
    }
    let mut arms = Vec::new();
    for transitions in grouped.into_values() {
        let first = transitions[0];
        let source = &first.in_state.ident;
        let event = &first.event.ident;
        let event_pattern = if first.event.data_type.is_some() {
            quote! { #events_name::#event(event_data) }
        } else {
            quote! { #events_name::#event }
        };
        let branches = transitions
            .iter()
            .map(|transition| {
                let (exit_composite, enter_composite) =
                    composite_lifecycle(transition, &composite_exit, &composite_entry);
                transition_code(
                    transition,
                    &state_place,
                    states_name,
                    events_name,
                    error_name,
                    lifecycle,
                    exit_composite,
                    enter_composite,
                    &callback,
                    temporary_context,
                    has_deferred_events,
                    async_queue,
                )
            })
            .collect::<parse::Result<Vec<_>>>()?;
        let state_pattern =
            if first.in_state.data_type.is_some() && first.in_state.composite.is_none() {
                quote! { #states_name::#source(state_data) }
            } else {
                quote! { #states_name::#source }
            };
        arms.push(quote! {
            (#state_pattern, #event_pattern) => { #(#branches)* }
        });
    }
    for transition in machine.transitions.iter().filter(|transition| {
        transition.event.kind == EventKind::Unexpected && !transition.event.wildcard
    }) {
        let source = &transition.in_state.ident;
        let event = &transition.event.ident;
        let event_pattern = if transition.event.data_type.is_some() {
            quote! { #events_name::#event(event_data) }
        } else {
            quote! { #events_name::#event }
        };
        let (exit_composite, enter_composite) =
            composite_lifecycle(transition, &composite_exit, &composite_entry);
        let branch = transition_code(
            transition,
            &state_place,
            states_name,
            events_name,
            error_name,
            lifecycle,
            exit_composite,
            enter_composite,
            &callback,
            temporary_context,
            has_deferred_events,
            async_queue,
        )?;
        let state_pattern =
            if transition.in_state.data_type.is_some() && transition.in_state.composite.is_none() {
                quote! { #states_name::#source(state_data) }
            } else {
                quote! { #states_name::#source }
            };
        arms.push(quote! {
            (#state_pattern, #event_pattern) => { #branch }
        });
    }
    for transition in machine.transitions.iter().filter(|transition| {
        transition.event.kind == EventKind::Unexpected && transition.event.wildcard
    }) {
        let source = &transition.in_state.ident;
        let (exit_composite, enter_composite) =
            composite_lifecycle(transition, &composite_exit, &composite_entry);
        let branch = transition_code(
            transition,
            &state_place,
            states_name,
            events_name,
            error_name,
            lifecycle,
            exit_composite,
            enter_composite,
            &callback,
            temporary_context,
            has_deferred_events,
            async_queue,
        )?;
        let state_pattern =
            if transition.in_state.data_type.is_some() && transition.in_state.composite.is_none() {
                quote! { #states_name::#source(state_data) }
            } else {
                quote! { #states_name::#source }
            };
        arms.push(quote! {
            (#state_pattern, _) => { #branch }
        });
    }
    Ok(quote! {
        match (&#state_place, &event) {
            #(#arms,)*
            _ => {}
        }
    })
}

#[allow(clippy::too_many_arguments)]
fn transition_code(
    transition: &StateTransition,
    state_place: &TokenStream,
    states_name: &Ident,
    events_name: &Ident,
    error_name: &Ident,
    lifecycle: &HashMap<String, Lifecycle>,
    composite_exit: TokenStream,
    composite_entry: TokenStream,
    callback: &TokenStream,
    temporary_context: &Option<TokenStream>,
    has_deferred_events: bool,
    async_queue: bool,
) -> parse::Result<TokenStream> {
    let state_arg = (transition.in_state.data_type.is_some()
        && transition.in_state.composite.is_none())
    .then(|| quote! { state_data });
    let event_arg = transition
        .event
        .data_type
        .is_some()
        .then(|| quote! { event_data });
    let callback_args = callback_arguments(temporary_context, &[state_arg.clone(), event_arg]);
    let guard = transition
        .guard
        .as_ref()
        .map(|guard| guard_code(guard, &callback_args, error_name))
        .transpose()?;
    let actions = transition
        .action
        .iter()
        .chain(&transition.additional_actions)
        .cloned()
        .collect::<Vec<_>>();
    let produces_state = !transition.internal_transition
        && transition.out_state.composite.is_none()
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
        && transition.out_state.composite.is_none()
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
        let event_value = if transition.event.data_type.is_some() {
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
    let source = &transition.in_state.ident;
    let body = if transition.internal_transition {
        quote! {
            #action_code
            #defer_code
            #(#process_code)*
            handled = true;
        }
    } else {
        let exit = lifecycle
            .get(&source.to_string())
            .and_then(|hooks| hooks.exit.as_ref())
            .map(|actions| {
                action_calls(
                    actions,
                    callback_arguments(temporary_context, core::slice::from_ref(&state_arg)),
                    error_name,
                )
            })
            .unwrap_or_default();
        let entry_actions = lifecycle
            .get(&target.to_string())
            .and_then(|hooks| hooks.entry.as_ref())
            .cloned()
            .unwrap_or_default();
        let target_has_data =
            transition.out_state.data_type.is_some() && transition.out_state.composite.is_none();
        let target_expression = if target_has_data {
            quote! { #states_name::#target(output_data) }
        } else {
            quote! { #states_name::#target }
        };
        let entry = if target_has_data {
            let calls = action_calls(
                &entry_actions,
                callback_arguments(temporary_context, &[Some(quote! { new_state_data })]),
                error_name,
            );
            quote! {
                if let #states_name::#target(new_state_data) = &#state_place { #calls }
            }
        } else {
            action_calls(
                &entry_actions,
                callback_arguments(temporary_context, &[]),
                error_name,
            )
        };
        quote! {
            #composite_exit
            #exit
            #action_code
            #output_data
            let new_state_value = #target_expression;
            let old_state = core::mem::replace(&mut #state_place, new_state_value);
            let new_state = &#state_place;
            #callback
            #entry
            #composite_entry
            #defer_code
            #(#process_code)*
            #drain_deferred
            handled = true;
        }
    };
    Ok(if let Some(guard) = guard {
        quote! { if !handled && #guard { #body } }
    } else {
        quote! { if !handled { #body } }
    })
}

fn composite_lifecycle(
    transition: &StateTransition,
    exit: &TokenStream,
    entry: &TokenStream,
) -> (TokenStream, TokenStream) {
    let exit =
        if transition.in_state.composite.is_some() && transition.out_state.composite.is_none() {
            exit.clone()
        } else {
            TokenStream::new()
        };
    let entry =
        if transition.out_state.composite.is_some() && transition.in_state.composite.is_none() {
            entry.clone()
        } else {
            TokenStream::new()
        };
    (exit, entry)
}

#[allow(clippy::too_many_arguments)]
fn exception_code(
    machine: &StateMachine,
    state_place: TokenStream,
    states_name: &Ident,
    events_name: &Ident,
    error_name: &Ident,
    lifecycle: &HashMap<String, Lifecycle>,
    composite_exit: TokenStream,
    composite_entry: TokenStream,
    callback: TokenStream,
    temporary_context: &Option<TokenStream>,
    has_deferred_events: bool,
    async_queue: bool,
) -> parse::Result<TokenStream> {
    let mut grouped = HashMap::<String, Vec<&StateTransition>>::new();
    for transition in machine
        .transitions
        .iter()
        .filter(|transition| transition.event.kind == EventKind::Exception)
    {
        if transition.defer || !transition.process_events.is_empty() {
            return Err(parse::Error::new(
                transition.event.ident.span(),
                "exception handlers cannot defer or process events",
            ));
        }
        grouped
            .entry(transition.in_state.ident.to_string())
            .or_default()
            .push(transition);
    }
    let mut arms = Vec::new();
    for mut transitions in grouped.into_values() {
        transitions.sort_by_key(|transition| transition.event.wildcard);
        let source = &transitions[0].in_state.ident;
        let state_pattern = if transitions[0].in_state.data_type.is_some()
            && transitions[0].in_state.composite.is_none()
        {
            quote! { #states_name::#source(state_data) }
        } else {
            quote! { #states_name::#source }
        };
        let branches = transitions
            .iter()
            .map(|transition| {
                let (exit_composite, enter_composite) =
                    composite_lifecycle(transition, &composite_exit, &composite_entry);
                let branch = transition_code(
                    transition,
                    &state_place,
                    states_name,
                    events_name,
                    error_name,
                    lifecycle,
                    exit_composite,
                    enter_composite,
                    &callback,
                    temporary_context,
                    has_deferred_events,
                    async_queue,
                )?;
                Ok(if transition.event.wildcard {
                    branch
                } else {
                    quote! { let event_data = error_data; #branch }
                })
            })
            .collect::<parse::Result<Vec<_>>>()?;
        arms.push(quote! { #state_pattern => { #(#branches)* } });
    }
    Ok(quote! {
        if !exception_handled {
            let mut handled = false;
            match &#state_place {
                #(#arms,)*
                _ => {}
            }
            exception_handled = handled;
        }
    })
}

#[allow(clippy::too_many_arguments)]
fn completion_code(
    machine: &StateMachine,
    state_place: TokenStream,
    states_name: &Ident,
    events_name: &Ident,
    events: &BTreeMap<String, EventInfo>,
    error_name: &Ident,
    lifecycle: &HashMap<String, Lifecycle>,
    composite_terminal: TokenStream,
    composite_exit: TokenStream,
    composite_entry: TokenStream,
    callback: TokenStream,
    temporary_context: &Option<TokenStream>,
    has_deferred_events: bool,
    async_queue: bool,
) -> parse::Result<TokenStream> {
    let mut grouped = HashMap::<String, Vec<&StateTransition>>::new();
    for transition in machine.transitions.iter().filter(|transition| {
        transition.event.kind == EventKind::Completion && transition.event.wildcard
    }) {
        grouped
            .entry(transition.in_state.ident.to_string())
            .or_default()
            .push(transition);
    }
    let mut anonymous_arms = Vec::new();
    for transitions in grouped.into_values() {
        let source = &transitions[0].in_state.ident;
        let eligible = if transitions[0].in_state.composite.is_some() {
            composite_terminal.clone()
        } else {
            quote! { true }
        };
        let branches = transitions
            .iter()
            .map(|transition| {
                transition_code(
                    transition,
                    &state_place,
                    states_name,
                    events_name,
                    error_name,
                    lifecycle,
                    if transition.in_state.composite.is_some()
                        && transition.out_state.composite.is_none()
                    {
                        composite_exit.clone()
                    } else {
                        TokenStream::new()
                    },
                    if transition.out_state.composite.is_some()
                        && transition.in_state.composite.is_none()
                    {
                        composite_entry.clone()
                    } else {
                        TokenStream::new()
                    },
                    &callback,
                    temporary_context,
                    has_deferred_events,
                    async_queue,
                )
            })
            .collect::<parse::Result<Vec<_>>>()?;
        let state_pattern = if transitions[0].in_state.data_type.is_some()
            && transitions[0].in_state.composite.is_none()
        {
            quote! { #states_name::#source(state_data) }
        } else {
            quote! { #states_name::#source }
        };
        anonymous_arms.push(quote! {
            #state_pattern if #eligible => { #(#branches)* }
        });
    }
    let mut origin_grouped = HashMap::<(String, String), Vec<&StateTransition>>::new();
    for transition in machine.transitions.iter().filter(|transition| {
        transition.event.kind == EventKind::Completion && !transition.event.wildcard
    }) {
        origin_grouped
            .entry((
                transition.in_state.ident.to_string(),
                transition.event.ident.to_string(),
            ))
            .or_default()
            .push(transition);
    }
    let mut origin_arms = Vec::new();
    for transitions in origin_grouped.into_values() {
        let first = transitions[0];
        let source = &first.in_state.ident;
        let event = &first.event.ident;
        let event_type = events
            .get(&event.to_string())
            .and_then(|event| event.ty.clone());
        let event_pattern = if event_type.is_some() {
            quote! { #events_name::#event(event_data) }
        } else {
            quote! { #events_name::#event }
        };
        let eligible = if first.in_state.composite.is_some() {
            composite_terminal.clone()
        } else {
            quote! { true }
        };
        let branches = transitions
            .iter()
            .map(|transition| {
                let mut effective = (*transition).clone();
                effective.event.data_type.clone_from(&event_type);
                transition_code(
                    &effective,
                    &state_place,
                    states_name,
                    events_name,
                    error_name,
                    lifecycle,
                    if transition.in_state.composite.is_some()
                        && transition.out_state.composite.is_none()
                    {
                        composite_exit.clone()
                    } else {
                        TokenStream::new()
                    },
                    if transition.out_state.composite.is_some()
                        && transition.in_state.composite.is_none()
                    {
                        composite_entry.clone()
                    } else {
                        TokenStream::new()
                    },
                    &callback,
                    temporary_context,
                    has_deferred_events,
                    async_queue,
                )
            })
            .collect::<parse::Result<Vec<_>>>()?;
        let state_pattern =
            if first.in_state.data_type.is_some() && first.in_state.composite.is_none() {
                quote! { #states_name::#source(state_data) }
            } else {
                quote! { #states_name::#source }
            };
        origin_arms.push(quote! {
            (#state_pattern, #event_pattern) if #eligible => { #(#branches)* }
        });
    }
    Ok(quote! {
        if let Some(origin) = origin {
            match (&#state_place, origin) {
                #(#origin_arms,)*
                _ => {}
            }
        }
        if !handled {
            match &#state_place {
                #(#anonymous_arms,)*
                _ => {}
            }
        }
    })
}

fn callback_arguments(
    temporary_context: &Option<TokenStream>,
    arguments: &[Option<TokenStream>],
) -> Option<TokenStream> {
    let arguments = temporary_context
        .iter()
        .cloned()
        .chain(arguments.iter().flatten().cloned())
        .collect::<Vec<_>>();
    (!arguments.is_empty()).then(|| quote! { #(#arguments),* })
}

fn action_sequence(
    actions: &[AsyncIdent],
    eval_actions: &[EvalAction],
    callback_args: &Option<TokenStream>,
    error_name: &Ident,
    produces_state: bool,
) -> parse::Result<TokenStream> {
    let mut output = TokenStream::new();
    let mut action_index = 0;
    let total = actions.len() + eval_actions.len();
    for position in 0..total {
        if let Some(eval) = eval_actions.iter().find(|eval| eval.position == position) {
            let guard = guard_code(&eval.guard, callback_args, error_name)?;
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

fn action_code(
    action: &AsyncIdent,
    event_arg: &Option<TokenStream>,
    error_name: &Ident,
) -> TokenStream {
    let ident = &action.ident;
    let action_await = action.is_async.then(|| quote! { .await });
    quote! {
        self.context.#ident(#event_arg)#action_await.map_err(#error_name::ActionFailed)?;
        self.context.log_action(stringify!(#ident));
    }
}

fn action_calls(
    actions: &[AsyncIdent],
    event_arg: Option<TokenStream>,
    error_name: &Ident,
) -> TokenStream {
    actions
        .iter()
        .map(|action| action_code(action, &event_arg, error_name))
        .collect()
}

fn guard_code(
    guard: &GuardExpression,
    event_arg: &Option<TokenStream>,
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
