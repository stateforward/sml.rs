pub mod cpp;
pub mod data;
pub mod event;
pub mod input_state;
pub mod lifetimes;
pub mod output_state;
pub mod state_machine;
pub mod transition;

use data::DataDefinitions;
use event::{EventKind, EventMapping};
use state_machine::StateMachine;

use input_state::InputState;
use proc_macro2::{Span, TokenStream};
use syn::spanned::Spanned;

use crate::parser::event::Transition;
use std::collections::{hash_map, HashMap, HashSet};
use std::fmt;
use syn::visit::{self, Visit};
use syn::{
    parse, Attribute, BoundLifetimes, GenericParam, Generics, Ident, Lifetime, LifetimeParam,
    TraitBound, Type, TypeBareFn,
};
use transition::StateTransition;
pub type TransitionMap = HashMap<String, HashMap<String, EventMapping>>;

pub fn state_ident(value: &str, span: Span) -> Ident {
    let mut ident = String::new();
    let mut uppercase = true;
    for character in value.chars() {
        if character.is_ascii_alphanumeric() {
            if uppercase {
                ident.extend(character.to_uppercase());
                uppercase = false;
            } else {
                ident.push(character);
            }
        } else {
            uppercase = true;
        }
    }
    if ident.is_empty() {
        ident.push_str("State");
    }
    if ident.as_bytes()[0].is_ascii_digit() {
        ident.insert(0, 'S');
    }
    Ident::new(&ident, span)
}

#[derive(Debug, Clone)]
pub struct AsyncIdent {
    pub ident: Ident,
    pub is_async: bool,
}
impl AsyncIdent {
    pub fn to_token_stream<F>(&self, visit: &mut F) -> TokenStream
    where
        F: FnMut(&AsyncIdent) -> TokenStream,
    {
        visit(self)
    }
}
impl fmt::Display for AsyncIdent {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        if self.is_async {
            write!(f, "{}().await", self.ident)
        } else {
            write!(f, "{}()", self.ident)
        }
    }
}

#[derive(Debug)]
pub struct ParsedStateMachine {
    pub name: Option<Ident>,
    pub states_attr: Vec<Attribute>,
    pub events_attr: Vec<Attribute>,
    pub temporary_context_type: Option<Type>,
    pub custom_error: bool,
    pub states: HashMap<String, Ident>,
    pub starting_state: Ident,
    pub state_data: DataDefinitions,
    pub events: HashMap<String, Ident>,
    pub event_data: DataDefinitions,
    pub states_events_mapping: HashMap<String, HashMap<String, EventMapping>>,
    pub entry_exit_async: bool,
    pub fixed_error_type: Option<Type>,
    pub event_generics: Generics,
}

fn event_key(event: &event::Event) -> String {
    match event.kind {
        EventKind::Normal => event.ident.to_string(),
        EventKind::Unexpected if event.wildcard => "unexpected::*".to_string(),
        EventKind::Unexpected => format!("unexpected::{}", event.ident),
        EventKind::Completion if event.wildcard => "completion::*".to_string(),
        EventKind::Completion => format!("completion::{}", event.ident),
        EventKind::Entry => "lifecycle::entry".to_string(),
        EventKind::Exit => "lifecycle::exit".to_string(),
        EventKind::Exception if event.wildcard => "exception::*".to_string(),
        EventKind::Exception => format!("exception::{}", event.ident),
    }
}

// helper function for adding a transition to a transition event map
fn add_transition(
    transition: &StateTransition,
    transition_map: &mut TransitionMap,
    state_data: &DataDefinitions,
) -> Result<(), parse::Error> {
    let p = transition_map
        .get_mut(&transition.in_state.ident.to_string())
        .unwrap();

    match p.entry(event_key(&transition.event)) {
        hash_map::Entry::Vacant(entry) => {
            let mapping = EventMapping {
                in_state: transition.in_state.ident.clone(),
                event: transition.event.ident.clone(),
                event_kind: transition.event.kind,
                event_wildcard: transition.event.wildcard,
                event_external: transition.event.external,
                transitions: vec![Transition {
                    guard: transition.guard.clone(),
                    action: transition.action.clone(),
                    additional_actions: transition.additional_actions.clone(),
                    process_events: transition.process_events.clone(),
                    defer: transition.defer,
                    eval_actions: transition.eval_actions.clone(),
                    default_output: transition.out_state.composite.is_some(),
                    out_state: transition.out_state.ident.clone(),
                    internal_transition: transition.internal_transition,
                }],
            };
            entry.insert(mapping);
        }
        hash_map::Entry::Occupied(mut entry) => {
            let mapping = entry.get_mut();
            mapping.transitions.push(Transition {
                guard: transition.guard.clone(),
                action: transition.action.clone(),
                additional_actions: transition.additional_actions.clone(),
                process_events: transition.process_events.clone(),
                defer: transition.defer,
                eval_actions: transition.eval_actions.clone(),
                default_output: transition.out_state.composite.is_some(),
                out_state: transition.out_state.ident.clone(),
                internal_transition: transition.internal_transition,
            });
        }
    }

    // Check for actions when states have data a
    if state_data
        .data_types
        .contains_key(&transition.out_state.ident.to_string())
    {
        // This transition goes to a state that has data associated, check so it has an
        // action

        if transition.action.is_none() && transition.out_state.composite.is_none() {
            return Err(parse::Error::new(
                transition.out_state.ident.span(),
                "This state has data associated, but not action is define here to provide it.",
            ));
        }
    }
    Ok(())
}

impl ParsedStateMachine {
    fn type_uses_generic_param(event_type: &Type, generic: &GenericParam) -> bool {
        struct Usage<'a> {
            generic: &'a GenericParam,
            used: bool,
        }
        impl<'ast> Visit<'ast> for Usage<'_> {
            fn visit_type_path(&mut self, path: &'ast syn::TypePath) {
                match self.generic {
                    GenericParam::Type(param) => {
                        self.used |= path.qself.is_none()
                            && path.path.leading_colon.is_none()
                            && path
                                .path
                                .segments
                                .first()
                                .is_some_and(|segment| segment.ident == param.ident);
                    }
                    // In an angle-bracketed argument, syn intentionally parses an
                    // unbraced identifier such as `N` as a type until Rust resolves
                    // the target parameter's kind.
                    GenericParam::Const(param) => {
                        self.used |= path.qself.is_none()
                            && path.path.leading_colon.is_none()
                            && path.path.segments.len() == 1
                            && path
                                .path
                                .segments
                                .first()
                                .is_some_and(|segment| segment.ident == param.ident);
                    }
                    GenericParam::Lifetime(_) => {}
                }
                syn::visit::visit_type_path(self, path);
            }

            fn visit_expr_path(&mut self, path: &'ast syn::ExprPath) {
                if let GenericParam::Const(param) = self.generic {
                    self.used |= path.qself.is_none()
                        && path.path.leading_colon.is_none()
                        && path.path.segments.len() == 1
                        && path
                            .path
                            .segments
                            .first()
                            .is_some_and(|segment| segment.ident == param.ident);
                }
                syn::visit::visit_expr_path(self, path);
            }

            fn visit_lifetime(&mut self, lifetime: &'ast Lifetime) {
                self.used |= matches!(
                    self.generic,
                    GenericParam::Lifetime(param) if param.lifetime == *lifetime
                );
            }
        }

        let mut usage = Usage {
            generic,
            used: false,
        };
        usage.visit_type(event_type);
        usage.used
    }

    fn generics_with_lifetimes(
        mut generics: Generics,
        lifetimes: &lifetimes::Lifetimes,
    ) -> Generics {
        let mut missing = Vec::new();
        for lifetime in lifetimes.as_slice() {
            let already_declared = generics.params.iter().any(|param| {
                matches!(param, GenericParam::Lifetime(known) if known.lifetime == *lifetime)
            });
            if !already_declared {
                missing.push(GenericParam::Lifetime(LifetimeParam::new(lifetime.clone())));
            }
        }
        if !missing.is_empty() {
            let mut params = syn::punctuated::Punctuated::new();
            params.extend(
                generics
                    .params
                    .iter()
                    .filter(|param| matches!(param, GenericParam::Lifetime(_)))
                    .cloned(),
            );
            params.extend(missing);
            params.extend(
                generics
                    .params
                    .iter()
                    .filter(|param| !matches!(param, GenericParam::Lifetime(_)))
                    .cloned(),
            );
            generics.params = params;
        }
        if !generics.params.is_empty() {
            generics.lt_token.get_or_insert_with(Default::default);
            generics.gt_token.get_or_insert_with(Default::default);
        }
        generics
    }

    pub fn event_generics_with_lifetimes(&self, lifetimes: &lifetimes::Lifetimes) -> Generics {
        Self::generics_with_lifetimes(self.event_generics.clone(), lifetimes)
    }

    pub fn callback_generics_with_lifetimes(
        &self,
        lifetimes: &lifetimes::Lifetimes,
        event_type: Option<&Type>,
    ) -> Generics {
        let temporary_context_uses_event_generics = self.temporary_context_uses_event_generics();
        let uses_event_generics = event_type
            .is_some_and(|event_type| self.type_uses_event_generics(event_type))
            || temporary_context_uses_event_generics;
        let generics = if uses_event_generics {
            self.event_generics.clone()
        } else {
            Generics::default()
        };
        let mut callback_lifetimes = lifetimes.clone();
        if temporary_context_uses_event_generics {
            let context = self
                .temporary_context_type
                .as_ref()
                .expect("generic temporary context exists");
            for param in self.event_generics.lifetimes() {
                if Self::type_uses_generic_param(context, &GenericParam::Lifetime(param.clone())) {
                    callback_lifetimes.insert(&param.lifetime);
                }
            }
        }
        let generics = Self::generics_with_lifetimes(generics, &callback_lifetimes);
        if uses_event_generics {
            self.filter_event_generic_lifetimes(generics, &callback_lifetimes)
        } else {
            generics
        }
    }

    pub fn completion_generics_with_lifetimes(
        &self,
        lifetimes: &lifetimes::Lifetimes,
        event_type: Option<&Type>,
    ) -> Generics {
        let uses_event_generics =
            event_type.is_some_and(|event_type| self.type_uses_event_generics(event_type));
        let generics = if uses_event_generics {
            self.event_generics.clone()
        } else {
            Generics::default()
        };
        let mut generics = Self::generics_with_lifetimes(generics, lifetimes);
        if !uses_event_generics {
            return generics;
        }

        generics = self.filter_event_generic_lifetimes(generics, lifetimes);
        generics
    }

    fn filter_event_generic_lifetimes(
        &self,
        mut generics: Generics,
        lifetimes: &lifetimes::Lifetimes,
    ) -> Generics {
        let retained_lifetimes: HashSet<_> = lifetimes
            .as_slice()
            .iter()
            .map(|lifetime| lifetime.ident.to_string())
            .collect();
        let omitted_lifetimes: HashSet<_> = self
            .event_generics
            .lifetimes()
            .map(|param| param.lifetime.ident.to_string())
            .filter(|lifetime| !retained_lifetimes.contains(lifetime))
            .collect();
        if omitted_lifetimes.is_empty() {
            return generics;
        }

        struct OmittedLifetime<'a> {
            omitted: &'a HashSet<String>,
            found: bool,
            bound_lifetime_scopes: Vec<HashSet<String>>,
        }
        impl OmittedLifetime<'_> {
            fn push_bound_lifetimes(&mut self, lifetimes: &BoundLifetimes) {
                self.bound_lifetime_scopes.push(
                    lifetimes
                        .lifetimes
                        .iter()
                        .filter_map(|param| match param {
                            GenericParam::Lifetime(param) => Some(param.lifetime.ident.to_string()),
                            GenericParam::Type(_) | GenericParam::Const(_) => None,
                        })
                        .collect(),
                );
            }

            fn lifetime_is_bound(&self, lifetime: &Lifetime) -> bool {
                self.bound_lifetime_scopes
                    .iter()
                    .rev()
                    .any(|scope| scope.contains(&lifetime.ident.to_string()))
            }
        }
        impl<'ast> Visit<'ast> for OmittedLifetime<'_> {
            fn visit_lifetime(&mut self, lifetime: &'ast Lifetime) {
                if !self.lifetime_is_bound(lifetime) {
                    self.found |= self.omitted.contains(&lifetime.ident.to_string());
                }
            }

            fn visit_type_bare_fn(&mut self, bare_fn: &'ast TypeBareFn) {
                if let Some(lifetimes) = &bare_fn.lifetimes {
                    self.push_bound_lifetimes(lifetimes);
                    visit::visit_type_bare_fn(self, bare_fn);
                    self.bound_lifetime_scopes.pop();
                } else {
                    visit::visit_type_bare_fn(self, bare_fn);
                }
            }

            fn visit_trait_bound(&mut self, trait_bound: &'ast TraitBound) {
                if let Some(lifetimes) = &trait_bound.lifetimes {
                    self.push_bound_lifetimes(lifetimes);
                    visit::visit_trait_bound(self, trait_bound);
                    self.bound_lifetime_scopes.pop();
                } else {
                    visit::visit_trait_bound(self, trait_bound);
                }
            }
        }
        fn bound_uses_omitted_lifetime(
            omitted: &HashSet<String>,
            bound: &syn::TypeParamBound,
        ) -> bool {
            let mut usage = OmittedLifetime {
                omitted,
                found: false,
                bound_lifetime_scopes: Vec::new(),
            };
            usage.visit_type_param_bound(bound);
            usage.found
        }
        fn type_uses_omitted_lifetime(omitted: &HashSet<String>, data_type: &Type) -> bool {
            let mut usage = OmittedLifetime {
                omitted,
                found: false,
                bound_lifetime_scopes: Vec::new(),
            };
            usage.visit_type(data_type);
            usage.found
        }

        generics.params = generics
            .params
            .into_iter()
            .filter_map(|mut param| match &mut param {
                GenericParam::Lifetime(lifetime) => retained_lifetimes
                    .contains(&lifetime.lifetime.ident.to_string())
                    .then_some(param),
                GenericParam::Type(type_param) => {
                    type_param.bounds = type_param
                        .bounds
                        .clone()
                        .into_iter()
                        .filter(|bound| !bound_uses_omitted_lifetime(&omitted_lifetimes, bound))
                        .collect();
                    Some(param)
                }
                GenericParam::Const(_) => Some(param),
            })
            .collect();

        if let Some(where_clause) = &mut generics.where_clause {
            where_clause.predicates = where_clause
                .predicates
                .clone()
                .into_iter()
                .filter_map(|mut predicate| {
                    let retain = match &mut predicate {
                        syn::WherePredicate::Lifetime(lifetime_predicate) => {
                            if omitted_lifetimes
                                .contains(&lifetime_predicate.lifetime.ident.to_string())
                            {
                                return None;
                            }
                            lifetime_predicate.bounds = lifetime_predicate
                                .bounds
                                .clone()
                                .into_iter()
                                .filter(|bound| {
                                    !omitted_lifetimes.contains(&bound.ident.to_string())
                                })
                                .collect();
                            !lifetime_predicate.bounds.is_empty()
                        }
                        syn::WherePredicate::Type(type_predicate) => {
                            if type_uses_omitted_lifetime(
                                &omitted_lifetimes,
                                &type_predicate.bounded_ty,
                            ) {
                                return None;
                            }
                            type_predicate.bounds = type_predicate
                                .bounds
                                .clone()
                                .into_iter()
                                .filter(|bound| {
                                    !bound_uses_omitted_lifetime(&omitted_lifetimes, bound)
                                })
                                .collect();
                            !type_predicate.bounds.is_empty()
                        }
                        _ => true,
                    };
                    retain.then_some(predicate)
                })
                .collect();
            if where_clause.predicates.is_empty() {
                generics.where_clause = None;
            }
        }
        generics
    }

    pub fn type_uses_event_generics(&self, event_type: &Type) -> bool {
        self.event_generics
            .params
            .iter()
            .any(|generic| Self::type_uses_generic_param(event_type, generic))
    }

    pub fn temporary_context_uses_event_generics(&self) -> bool {
        self.temporary_context_type
            .as_ref()
            .is_some_and(|context| self.type_uses_event_generics(context))
    }

    pub fn new(mut sm: StateMachine) -> parse::Result<Self> {
        if !sm.event_generics.params.is_empty()
            && sm
                .transitions
                .iter()
                .any(|transition| transition.defer || !transition.process_events.is_empty())
        {
            return Err(parse::Error::new(
                sm.name
                    .as_ref()
                    .map_or_else(Span::call_site, Ident::span),
                "generic events cannot use `defer` or `process(...)` because their dispatch-scoped parameters cannot be stored in the machine's fixed event queue",
            ));
        }
        for transition in &sm.transitions {
            for (state, data_type) in [
                (
                    &transition.in_state.ident,
                    transition.in_state.data_type.as_ref(),
                ),
                (
                    &transition.out_state.ident,
                    transition.out_state.data_type.as_ref(),
                ),
            ] {
                let Some(data_type) = data_type else {
                    continue;
                };
                for generic in sm.event_generics.params.iter().filter(|generic| {
                    matches!(generic, GenericParam::Type(_) | GenericParam::Const(_))
                }) {
                    if Self::type_uses_generic_param(data_type, generic) {
                        let name = match generic {
                            GenericParam::Type(param) => param.ident.to_string(),
                            GenericParam::Const(param) => param.ident.to_string(),
                            GenericParam::Lifetime(_) => unreachable!(),
                        };
                        return Err(parse::Error::new(
                            state.span(),
                            format!(
                                "stored state data cannot use dispatch-scoped generic event parameter `{name}`; only lifetimes may be shared with machine state"
                            ),
                        ));
                    }
                }
            }
        }
        for transition in &sm.transitions {
            let Some(event_type) = transition.event.data_type.as_ref() else {
                continue;
            };
            if transition.event.kind == EventKind::Exception {
                if let Some(generic) = sm
                    .event_generics
                    .params
                    .iter()
                    .find(|generic| Self::type_uses_generic_param(event_type, generic))
                {
                    let name = match generic {
                        GenericParam::Lifetime(param) => param.lifetime.to_string(),
                        GenericParam::Type(param) => param.ident.to_string(),
                        GenericParam::Const(param) => param.ident.to_string(),
                    };
                    return Err(parse::Error::new(
                        transition.event.ident.span(),
                        format!(
                            "typed exception payloads cannot use dispatch-scoped generic event parameter `{name}`"
                        ),
                    ));
                }
            }
            let uses_declared_generic = sm
                .event_generics
                .params
                .iter()
                .any(|generic| Self::type_uses_generic_param(event_type, generic));
            if !transition.event.external && !uses_declared_generic {
                continue;
            }
            for generic in
                sm.event_generics.params.iter().filter(|generic| {
                    matches!(generic, GenericParam::Type(_) | GenericParam::Const(_))
                })
            {
                if !Self::type_uses_generic_param(event_type, generic) {
                    let name = match generic {
                        GenericParam::Type(param) => param.ident.to_string(),
                        GenericParam::Const(param) => param.ident.to_string(),
                        GenericParam::Lifetime(_) => unreachable!(),
                    };
                    return Err(parse::Error::new(
                        transition.event.ident.span(),
                        format!(
                            "generic event `{}` must use declared parameter `{name}` so generated dispatch and callbacks can infer the complete event family",
                            transition.event.ident
                        ),
                    ));
                }
            }
        }
        for generic in &sm.event_generics.params {
            let used_by_event = sm.transitions.iter().any(|transition| {
                transition
                    .event
                    .data_type
                    .as_ref()
                    .is_some_and(|event_type| Self::type_uses_generic_param(event_type, generic))
            });
            if !used_by_event {
                let (name, span) = match generic {
                    GenericParam::Lifetime(param) => {
                        (param.lifetime.to_string(), param.lifetime.span())
                    }
                    GenericParam::Type(param) => (param.ident.to_string(), param.ident.span()),
                    GenericParam::Const(param) => (param.ident.to_string(), param.ident.span()),
                };
                return Err(parse::Error::new(
                    span,
                    format!(
                        "declared generic event parameter `{name}` must be used by at least one event payload"
                    ),
                ));
            }
        }
        if let Some(context) = sm.temporary_context_type.as_ref().filter(|context| {
            sm.event_generics
                .params
                .iter()
                .any(|generic| Self::type_uses_generic_param(context, generic))
        }) {
            for generic in
                sm.event_generics.params.iter().filter(|generic| {
                    matches!(generic, GenericParam::Type(_) | GenericParam::Const(_))
                })
            {
                if !Self::type_uses_generic_param(context, generic) {
                    let name = match generic {
                        GenericParam::Type(param) => param.ident.to_string(),
                        GenericParam::Const(param) => param.ident.to_string(),
                        GenericParam::Lifetime(_) => unreachable!(),
                    };
                    return Err(parse::Error::new(
                        context.span(),
                        format!(
                            "a generic temporary context must use declared parameter `{name}` so generated callbacks can infer the complete event family"
                        ),
                    ));
                }
            }
        }
        for transition in sm
            .transitions
            .iter()
            .filter(|transition| transition.event.kind == EventKind::Completion)
        {
            if matches!(
                transition.event.data_type,
                Some(Type::Reference(ref reference)) if reference.mutability.is_some()
            ) {
                return Err(parse::Error::new(
                    transition.event.ident.span(),
                    "Completion origin data cannot be a mutable reference.",
                ));
            }
        }

        // Derive out_state for internal non-wildcard transitions
        for transition in sm.transitions.iter_mut() {
            if transition.out_state.internal_transition && !transition.in_state.wildcard {
                transition.out_state.ident = transition.in_state.ident.clone();
                transition
                    .out_state
                    .data_type
                    .clone_from(&transition.in_state.data_type);
                transition.out_state.internal_transition = false;
            }
        }

        // Check the initial state definition
        let mut starting_transitions_iter = sm.transitions.iter().filter(|sm| sm.in_state.start);

        let starting_transition = starting_transitions_iter.next().ok_or(parse::Error::new(
            Span::call_site(),
            "No starting state defined, indicate the starting state with a *.",
        ))?;

        if starting_transitions_iter.next().is_some() {
            return Err(parse::Error::new(
                Span::call_site(),
                "More than one starting state defined (indicated with *), remove duplicates.",
            ));
        }

        // Extract the starting state
        let starting_state = starting_transition.in_state.ident.clone();

        let mut states = HashMap::new();
        let mut state_data = DataDefinitions::new();
        let mut events = HashMap::new();
        let mut event_data = DataDefinitions::new();
        let mut states_events_mapping = TransitionMap::new();

        for transition in sm.transitions.iter() {
            // Collect states
            let in_state_name = transition.in_state.ident.to_string();
            if !transition.in_state.wildcard {
                states.insert(in_state_name.clone(), transition.in_state.ident.clone());
                state_data.collect(in_state_name.clone(), transition.in_state.data_type.clone())?;
            }
            if !transition.out_state.internal_transition {
                let out_state_name = transition.out_state.ident.to_string();
                states.insert(out_state_name.clone(), transition.out_state.ident.clone());
                state_data.collect(
                    out_state_name.clone(),
                    transition.out_state.data_type.clone(),
                )?;
            }

            // Collect events
            if !transition.event.wildcard
                && !matches!(
                    transition.event.kind,
                    EventKind::Entry | EventKind::Exit | EventKind::Exception
                )
            {
                let event_name = transition.event.ident.to_string();
                events.insert(event_name.clone(), transition.event.ident.clone());
                if transition.event.kind != EventKind::Completion
                    || transition.event.data_type.is_some()
                {
                    event_data.collect(event_name.clone(), transition.event.data_type.clone())?;
                }
            }

            // add input and output states to the mapping HashMap
            if !transition.in_state.wildcard {
                states_events_mapping.insert(transition.in_state.ident.to_string(), HashMap::new());
            }
            if !transition.out_state.internal_transition {
                states_events_mapping
                    .insert(transition.out_state.ident.to_string(), HashMap::new());
            }
        }

        for transition in sm.transitions.iter() {
            // if input state is a wildcard, we need to add this transition for all states
            if transition.in_state.wildcard {
                let mut transition_added = false;

                for (name, in_state) in &states {
                    // skip already set input state
                    let p = states_events_mapping
                        .get_mut(&in_state.to_string())
                        .unwrap();

                    if p.contains_key(&event_key(&transition.event)) {
                        continue;
                    }

                    // create a new input state from wildcard
                    let in_state = InputState {
                        start: false,
                        wildcard: false,
                        ident: in_state.clone(),
                        data_type: state_data.data_types.get(name).cloned(),
                        composite: None,
                        history: false,
                    };

                    // create the transition
                    let mut out_state = transition.out_state.clone();
                    if out_state.internal_transition {
                        out_state.ident = in_state.ident.clone();
                        out_state.data_type.clone_from(&in_state.data_type);
                    }
                    let wildcard_transition = StateTransition {
                        in_state,
                        event: transition.event.clone(),
                        guard: transition.guard.clone(),
                        action: transition.action.clone(),
                        additional_actions: transition.additional_actions.clone(),
                        process_events: transition.process_events.clone(),
                        defer: transition.defer,
                        eval_actions: transition.eval_actions.clone(),
                        out_state,
                        internal_transition: transition.internal_transition,
                    };

                    // add the wildcard transition to the transition map
                    // The wildcard causes this validation error, so use its available span.
                    // but won't show up at that line
                    add_transition(
                        &wildcard_transition,
                        &mut states_events_mapping,
                        &state_data,
                    )?;

                    transition_added = true;
                }

                // No transitions were added by expanding the wildcard,
                // so emit an error to the user
                if !transition_added {
                    return Err(parse::Error::new(
                        transition.in_state.ident.span(),
                        "Wildcard has no effect",
                    ));
                }
            } else {
                add_transition(transition, &mut states_events_mapping, &state_data)?;
            }
        }

        let external_events: std::collections::HashSet<_> = sm
            .transitions
            .iter()
            .filter(|transition| transition.event.external)
            .map(|transition| transition.event.ident.to_string())
            .collect();
        for event_mappings in states_events_mapping.values_mut() {
            for mapping in event_mappings.values_mut() {
                mapping.event_external = external_events.contains(&mapping.event.to_string());
            }
        }

        Ok(ParsedStateMachine {
            name: sm.name,
            states_attr: sm.states_attr,
            events_attr: sm.events_attr,
            temporary_context_type: sm.temporary_context_type,
            custom_error: sm.custom_error,
            states,
            starting_state,
            state_data,
            events,
            event_data,
            states_events_mapping,
            entry_exit_async: sm.entry_exit_async,
            fixed_error_type: sm.fixed_error_type,
            event_generics: sm.event_generics,
        })
    }
}
