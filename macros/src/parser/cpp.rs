use super::state_machine::StateMachine;
use super::transition::StateTransitions;
use proc_macro2::{Delimiter, Punct, Spacing, TokenStream, TokenTree};
use quote::quote;
use syn::{braced, bracketed, parse, token, Attribute, Ident, Token, Type};

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

        while !content.is_empty() {
            let mut transition = TokenStream::new();
            while !content.is_empty() && !content.peek(Token![,]) {
                transition.extend([content.parse::<TokenTree>()?]);
            }
            let transitions: StateTransitions = syn::parse2(normalize_transition(transition))?;
            machine.add_transitions(transitions);
            if content.is_empty() {
                break;
            }
            content.parse::<Token![,]>()?;
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
    use super::{normalize_direction, normalize_transition, SmlDefinition, SmlDefinitions};
    use quote::quote;

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
