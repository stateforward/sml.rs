use super::state_machine::StateMachine;
use super::transition::StateTransitions;
use proc_macro2::{Delimiter, Punct, Spacing, TokenStream, TokenTree};
use quote::quote;
use syn::{
    braced, bracketed, parse, spanned::Spanned, token, Attribute, Generics, Ident, Token, Type,
    WhereClause,
};

/// Rust spelling of an sml.cpp transition table:
///
/// `MachineName { *source + event<Event> / action = target, ... }`
pub struct SmlDefinition {
    pub machine: StateMachine,
}

/// One or more adjacent sml.cpp-shaped machine definitions.
pub struct SmlDefinitions {
    pub machines: Vec<StateMachine>,
}

impl parse::Parse for SmlDefinitions {
    fn parse(input: parse::ParseStream) -> syn::Result<Self> {
        let mut machines = Vec::new();
        while !input.is_empty() {
            machines.push(input.parse::<SmlDefinition>()?.machine);
            let _ = input.parse::<Token![,]>();
        }
        Ok(Self { machines })
    }
}

impl parse::Parse for SmlDefinition {
    fn parse(input: parse::ParseStream) -> syn::Result<Self> {
        let name = if input.peek(Token![_]) {
            input.parse::<Token![_]>()?;
            None
        } else {
            Some(input.parse::<Ident>()?)
        };
        let mut machine = StateMachine::new();
        machine.name = name;
        let mut event_generics: Generics = input.parse()?;
        if input.peek(Token![where]) {
            event_generics.where_clause = Some(input.parse::<WhereClause>()?);
        }
        if let Some(defaulted) = event_generics.params.iter().find(|param| match param {
            syn::GenericParam::Type(param) => param.default.is_some(),
            syn::GenericParam::Const(param) => param.default.is_some(),
            syn::GenericParam::Lifetime(_) => false,
        }) {
            return Err(syn::Error::new(
                defaulted.span(),
                "generic event parameters cannot have defaults because they are propagated to generated methods",
            ));
        }
        machine.event_generics = event_generics;

        if input.peek(token::Bracket) {
            let options;
            bracketed!(options in input);
            while !options.is_empty() {
                let option: Ident = options.parse()?;
                match option.to_string().as_str() {
                    "custom_error" => machine.custom_error = true,
                    "entry_exit_async" => machine.entry_exit_async = true,
                    "temporary_context" => {
                        options.parse::<Token![:]>()?;
                        machine.temporary_context_type = Some(options.parse::<Type>()?);
                    }
                    "states_attr" => {
                        options.parse::<Token![:]>()?;
                        machine.states_attr = Attribute::parse_outer(&options)?;
                    }
                    "events_attr" => {
                        options.parse::<Token![:]>()?;
                        machine.events_attr = Attribute::parse_outer(&options)?;
                    }
                    _ => {
                        return Err(syn::Error::new(
                            option.span(),
                            "supported sml! options: custom_error, entry_exit_async, temporary_context, states_attr, events_attr",
                        ));
                    }
                }
                if options.is_empty() {
                    break;
                }
                options.parse::<Token![,]>()?;
            }
        }
        let content;
        braced!(content in input);
        let body = content.parse::<TokenStream>()?;

        for transition in split_transitions(body) {
            let transitions: StateTransitions = syn::parse2(normalize_transition(transition))?;
            machine.add_transitions(transitions);
        }

        let mut fixed_error_type = None;
        for transition in machine.transitions.iter().filter(|transition| {
            transition.event.kind == crate::parser::event::EventKind::Exception
                && !transition.event.wildcard
        }) {
            let error_type = transition
                .event
                .data_type
                .clone()
                .expect("typed exceptions carry their type");
            if fixed_error_type
                .as_ref()
                .is_some_and(|known| known != &error_type)
            {
                return Err(syn::Error::new(
                    transition.event.ident.span(),
                    "one machine cannot route multiple unrelated Rust callback error types",
                ));
            }
            fixed_error_type = Some(error_type);
        }
        if fixed_error_type.is_some() {
            machine.custom_error = true;
            machine.fixed_error_type = fixed_error_type;
        }

        Ok(Self { machine })
    }
}

fn split_transitions(body: TokenStream) -> Vec<TokenStream> {
    let tokens = body.into_iter().collect::<Vec<_>>();
    let mut transitions = Vec::new();
    let mut current = TokenStream::new();
    let mut angle_depth = 0usize;
    for (index, token) in tokens.iter().enumerate() {
        let punct = match token {
            TokenTree::Punct(punct) => Some(punct.as_char()),
            _ => None,
        };
        match punct {
            Some('<')
                if !matches!(
                    tokens.get(index + 1),
                    Some(TokenTree::Punct(next)) if next.as_char() == '='
                ) =>
            {
                angle_depth += 1;
                current.extend([token.clone()]);
            }
            Some('>') if angle_depth > 0 => {
                angle_depth -= 1;
                current.extend([token.clone()]);
            }
            Some(',') if angle_depth == 0 => {
                if !current.is_empty() {
                    transitions.push(current);
                    current = TokenStream::new();
                }
            }
            _ => current.extend([token.clone()]),
        }
    }
    if !current.is_empty() {
        transitions.push(current);
    }
    transitions
}

fn normalize_direction(transition: TokenStream) -> TokenStream {
    let tokens: Vec<_> = transition.into_iter().collect();
    let reverse = tokens.windows(2).position(|window| {
        matches!(&window[0], TokenTree::Punct(punct) if punct.as_char() == '<')
            && matches!(&window[1], TokenTree::Punct(punct) if punct.as_char() == '=')
    });

    let Some(index) = reverse else {
        return tokens.into_iter().collect();
    };

    let mut normalized: TokenStream = tokens[index + 2..].iter().cloned().collect();
    normalized.extend([TokenTree::Punct(Punct::new('=', Spacing::Alone))]);
    normalized.extend(tokens[..index].iter().cloned());
    normalized
}

fn normalize_transition(transition: TokenStream) -> TokenStream {
    let directed = normalize_direction(transition);
    let tokens: Vec<_> = directed.into_iter().collect();
    if tokens
        .iter()
        .any(|token| matches!(token, TokenTree::Punct(punct) if punct.as_char() == '+'))
    {
        return tokens.into_iter().collect();
    }

    let insertion = tokens.iter().position(|token| {
        matches!(token, TokenTree::Punct(punct) if matches!(punct.as_char(), '/' | '='))
            || matches!(token, TokenTree::Group(group) if group.delimiter() == Delimiter::Bracket)
    });
    let Some(insertion) = insertion else {
        return tokens.into_iter().collect();
    };

    let mut normalized: TokenStream = tokens[..insertion].iter().cloned().collect();
    normalized.extend(quote! { + completion<_> });
    normalized.extend(tokens[insertion..].iter().cloned());
    normalized
}

#[cfg(test)]
mod tests {
    use super::{
        normalize_direction, normalize_transition, split_transitions, SmlDefinition, SmlDefinitions,
    };
    use quote::quote;
    use syn::Type;

    #[test]
    fn reverse_transition_is_normalized_to_forward_form() {
        let normalized = normalize_direction(quote! {
            "open"_s <= *"empty"_s + event<OpenClose> / open_drawer
        });
        assert_eq!(
            normalized.to_string(),
            quote! {
                *"empty"_s + event<OpenClose> / open_drawer = "open"_s
            }
            .to_string()
        );
    }

    #[test]
    fn anonymous_transition_gets_completion_trigger() {
        let normalized = normalize_transition(quote! {
            *"idle"_s / start = "ready"_s
        });
        assert_eq!(
            normalized.to_string(),
            quote! {
                *"idle"_s + completion<_> / start = "ready"_s
            }
            .to_string()
        );
    }

    #[test]
    fn transition_split_ignores_generic_argument_commas() {
        let transitions = split_transitions(quote! {
            *Idle + event<Message<'a, T, N>> = Ready,
            Ready + event<Done> = X,
        });
        assert_eq!(transitions.len(), 2);
    }

    #[test]
    fn parses_native_machine_configuration() {
        let definition: SmlDefinition = syn::parse2(quote! {
            Configured[
                custom_error,
                entry_exit_async,
                temporary_context: &mut u32,
                states_attr: #[derive(Debug)],
                events_attr: #[derive(Clone)]
            ] {
                *"idle"_s + event<Start> = X,
            }
        })
        .unwrap();

        assert!(definition.machine.custom_error);
        assert!(definition.machine.entry_exit_async);
        assert!(definition.machine.temporary_context_type.is_some());
        assert_eq!(definition.machine.states_attr.len(), 1);
        assert_eq!(definition.machine.events_attr.len(), 1);
    }

    #[test]
    fn parses_event_generics_and_where_clause() {
        let definition: SmlDefinition = syn::parse2(quote! {
            Generic<'a, T: Clone, const N: usize>
            where
                T: core::fmt::Debug,
            {
                *Idle + event<Message<'a, T, N>> = X,
            }
        })
        .unwrap();

        assert_eq!(definition.machine.event_generics.params.len(), 3);
        assert_eq!(
            definition
                .machine
                .event_generics
                .where_clause
                .as_ref()
                .unwrap()
                .predicates
                .len(),
            1
        );
        let event = &definition.machine.transitions[0].event;
        assert_eq!(event.ident, "Message");
        assert!(matches!(event.data_type, Some(Type::Path(_))));
    }

    #[test]
    fn parses_unnamed_and_adjacent_definitions() {
        let definitions: SmlDefinitions = syn::parse2(quote! {
            _ { *Idle + Start = X },
            Named { *Idle + Stop = X },
        })
        .unwrap();
        assert_eq!(definitions.machines.len(), 2);
        assert!(definitions.machines[0].name.is_none());
        assert_eq!(definitions.machines[1].name.as_ref().unwrap(), "Named");
    }

    #[test]
    fn rejects_unknown_option_and_unrelated_exception_types() {
        assert!(
            syn::parse2::<SmlDefinition>(quote! { Bad[unknown] { *Idle + Start = X } }).is_err()
        );
        assert!(syn::parse2::<SmlDefinition>(quote! {
            Bad {
                *Idle + exception<First> = X,
                 Idle + exception<Second> = X,
            }
        })
        .is_err());
    }

    #[test]
    fn typed_exception_sets_fixed_error_and_normalization_handles_noop_forms() {
        let definition: SmlDefinition = syn::parse2(quote! {
            Errors { *Idle + exception<MyError> = X }
        })
        .unwrap();
        assert!(definition.machine.custom_error);
        assert!(definition.machine.fixed_error_type.is_some());

        assert_eq!(
            normalize_transition(quote!(Idle + Start = Ready)).to_string(),
            quote!(Idle + Start = Ready).to_string()
        );
        assert_eq!(
            normalize_transition(quote!(Idle)).to_string(),
            quote!(Idle).to_string()
        );
    }
}
