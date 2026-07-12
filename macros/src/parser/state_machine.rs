use super::transition::{StateTransition, StateTransitions};
use syn::{braced, parse, spanned::Spanned, token, Attribute, Ident, Token, Type};

#[derive(Debug, Clone)]
pub struct StateMachine {
    pub temporary_context_type: Option<Type>,
    pub custom_error: bool,
    pub transitions: Vec<StateTransition>,
    pub name: Option<Ident>,
    pub states_attr: Vec<Attribute>,
    pub events_attr: Vec<Attribute>,
    pub entry_exit_async: bool,
    pub fixed_error_type: Option<Type>,
}

impl StateMachine {
    pub fn new() -> Self {
        StateMachine {
            temporary_context_type: None,
            custom_error: false,
            transitions: Vec::new(),
            name: None,
            states_attr: Vec::new(),
            events_attr: Vec::new(),
            entry_exit_async: false,
            fixed_error_type: None,
        }
    }

    pub fn add_transitions(&mut self, transitions: StateTransitions) {
        for in_state in transitions.in_states {
            let internal_transition = transitions.out_state.internal_transition;
            let transition = StateTransition {
                in_state,
                event: transitions.event.clone(),
                guard: transitions.guard.clone(),
                action: transitions.action.clone(),
                additional_actions: transitions.additional_actions.clone(),
                process_events: transitions.process_events.clone(),
                defer: transitions.defer,
                eval_actions: transitions.eval_actions.clone(),
                out_state: transitions.out_state.clone(),
                internal_transition,
            };
            self.transitions.push(transition);
        }
    }
}

impl parse::Parse for StateMachine {
    fn parse(input: parse::ParseStream) -> parse::Result<Self> {
        let mut statemachine = StateMachine::new();

        loop {
            // If the last line ends with a comma this is true
            if input.is_empty() {
                break;
            }

            match input.parse::<Ident>()?.to_string().as_str() {
                "transitions" => {
                    input.parse::<Token![:]>()?;
                    if input.peek(token::Brace) {
                        let content;
                        braced!(content in input);
                        loop {
                            if content.is_empty() {
                                break;
                            }

                            let transitions: StateTransitions = content.parse()?;
                            statemachine.add_transitions(transitions);

                            // No comma at end of line, no more transitions
                            if content.is_empty() {
                                break;
                            }

                            if content.parse::<Token![,]>().is_err() {
                                break;
                            };
                        }
                    }
                }
                "custom_error" => {
                    input.parse::<Token![:]>()?;
                    let custom_error: syn::LitBool = input.parse()?;
                    if custom_error.value {
                        statemachine.custom_error = true
                    }
                }
                "temporary_context" => {
                    input.parse::<Token![:]>()?;
                    let temporary_context_type: Type = input.parse()?;

                    // Check so the type is supported
                    match &temporary_context_type {
                        Type::Array(_)
                        | Type::Path(_)
                        | Type::Ptr(_)
                        | Type::Reference(_)
                        | Type::Slice(_)
                        | Type::Tuple(_) => (),
                        _ => {
                            return Err(parse::Error::new(
                                temporary_context_type.span(),
                                "This is an invalid type for the temporary state.",
                            ))
                        }
                    }

                    // Store the temporary context type
                    statemachine.temporary_context_type = Some(temporary_context_type);
                }
                "name" => {
                    input.parse::<Token![:]>()?;
                    statemachine.name = Some(input.parse::<Ident>()?);
                }

                "states_attr" => {
                    input.parse::<Token![:]>()?;
                    statemachine.states_attr = Attribute::parse_outer(input)?;
                }

                "events_attr" => {
                    input.parse::<Token![:]>()?;
                    statemachine.events_attr = Attribute::parse_outer(input)?;
                }

                "entry_exit_async" => {
                    input.parse::<Token![:]>()?;
                    let entry_exit_async: syn::LitBool = input.parse()?;
                    if entry_exit_async.value {
                        statemachine.entry_exit_async = true;
                    }
                }

                keyword => {
                    return Err(parse::Error::new(
                        input.span(),
                        format!(
                            "Unknown keyword {}. Support keywords: [\"name\", \
                                \"transitions\", \
                                \"temporary_context\", \
                                \"custom_error\", \
                                \"states_attr\", \
                                \"events_attr\", \
                                \"entry_exit_async\"
                                ]",
                            keyword
                        ),
                    ))
                }
            }

            // No comma at end of line, no more transitions
            if input.is_empty() {
                break;
            }

            if input.parse::<Token![,]>().is_err() {
                break;
            };
        }

        Ok(statemachine)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_every_native_configuration_field() {
        let machine: StateMachine = syn::parse_str(
            r#"
            name: Player,
            custom_error: true,
            temporary_context: (&'static str, [u8; 2]),
            states_attr: #[derive(Debug)] #[repr(u8)],
            events_attr: #[derive(Clone)],
            entry_exit_async: true,
            transitions: {
                *Idle + Start = Running,
                Running + Stop = Idle,
            },
            "#,
        )
        .unwrap();

        assert_eq!(machine.name.unwrap(), "Player");
        assert!(machine.custom_error);
        assert!(machine.temporary_context_type.is_some());
        assert_eq!(machine.states_attr.len(), 2);
        assert_eq!(machine.events_attr.len(), 1);
        assert!(machine.entry_exit_async);
        assert_eq!(machine.transitions.len(), 2);
    }

    #[test]
    fn false_flags_and_missing_trailing_commas_remain_false() {
        let machine: StateMachine = syn::parse_str(
            "custom_error: false, entry_exit_async: false, transitions: {*Idle + Start = Idle}",
        )
        .unwrap();
        assert!(!machine.custom_error);
        assert!(!machine.entry_exit_async);
        assert_eq!(machine.transitions.len(), 1);

        let empty: StateMachine = syn::parse_str("").unwrap();
        assert!(empty.transitions.is_empty());
    }

    #[test]
    fn accepts_every_supported_temporary_context_shape() {
        for ty in [
            "[u8; 4]",
            "Context",
            "*mut u8",
            "&'static mut u8",
            "[u8]",
            "(u8, u16)",
        ] {
            let source = format!("temporary_context: {ty}");
            assert!(syn::parse_str::<StateMachine>(&source).is_ok(), "{}", ty);
        }
    }

    #[test]
    fn rejects_invalid_context_and_unknown_keyword() {
        let invalid = syn::parse_str::<StateMachine>("temporary_context: fn()").unwrap_err();
        assert!(invalid.to_string().contains("invalid type"));

        let unknown = syn::parse_str::<StateMachine>("mystery: true").unwrap_err();
        assert!(unknown.to_string().contains("Unknown keyword mystery"));
    }
}
