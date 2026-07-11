use syn::{parenthesized, parse, spanned::Spanned, token, Ident, LitStr, Token, Type};

#[derive(Debug, Clone)]
pub struct InputState {
    pub start: bool,
    pub wildcard: bool,
    pub ident: Ident,
    pub data_type: Option<Type>,
    pub composite: Option<Ident>,
    pub history: bool,
}

impl parse::Parse for InputState {
    fn parse(input: parse::ParseStream) -> syn::Result<Self> {
        // Check for starting state definition
        let start = input.parse::<Token![*]>().is_ok();

        if input.peek(token::Paren) {
            let content;
            parenthesized!(content in input);
            let mut grouped: InputState = content.parse()?;
            if !content.is_empty() || grouped.start {
                return Err(parse::Error::new(
                    content.span(),
                    "a parenthesized state must contain exactly one state expression",
                ));
            }
            grouped.start = start;
            return Ok(grouped);
        }

        // check to see if this is a wildcard state, which is denoted with "underscore"
        let underscore = input.parse::<Token![_]>();
        let wildcard = underscore.is_ok();

        // wildcards can't be used as starting states
        if start && wildcard {
            return Err(parse::Error::new(
                input.span(),
                "Wildcards can't be used as the starting state.",
            ));
        }

        // Input State
        let mut ident: Ident = if let Ok(underscore) = underscore {
            underscore.into()
        } else if input.peek(LitStr) {
            let state: LitStr = input.parse()?;
            crate::parser::state_ident(&state.value(), state.span())
        } else {
            input.parse()?
        };
        let composite = if ident == "state" && input.peek(Token![<]) {
            input.parse::<Token![<]>()?;
            let child: Ident = input.parse()?;
            input.parse::<Token![>]>()?;
            ident = crate::parser::state_ident(&child.to_string(), child.span());
            Some(child)
        } else {
            None
        };
        let inferred_data_type = composite
            .as_ref()
            .map(|state_type| syn::parse_quote_spanned!(state_type.span()=> #state_type));
        if ident == "sml" && input.peek(Token![::]) {
            input.parse::<Token![::]>()?;
            ident = input.parse()?;
            if ident != "X" {
                return Err(parse::Error::new(
                    ident.span(),
                    "only the terminal state `sml::X` may be namespace-qualified",
                ));
            }
        }

        // Possible type on the input state
        let (data_type, history) = if input.peek(token::Paren) {
            let content;
            parenthesized!(content in input);
            let input: Type = content.parse()?;

            let history = matches!(
                &input,
                Type::Path(path)
                    if path.qself.is_none()
                        && path.path.segments.len() == 1
                        && path.path.is_ident("H")
            );

            // Wildcards should not have data or history associated.
            if wildcard {
                return Err(parse::Error::new(
                    input.span(),
                    "Wildcard states cannot have data associated with it.",
                ));
            }

            // Check so the type is supported
            match &input {
                Type::Array(_)
                | Type::Path(_)
                | Type::Ptr(_)
                | Type::Reference(_)
                | Type::Slice(_)
                | Type::Tuple(_) => (),
                _ => {
                    return Err(parse::Error::new(
                        input.span(),
                        "This is an invalid type for states.",
                    ))
                }
            }

            if history {
                (None, true)
            } else {
                (Some(input), false)
            }
        } else {
            (inferred_data_type, false)
        };

        Ok(Self {
            start,
            wildcard,
            ident,
            data_type,
            composite,
            history,
        })
    }
}

#[cfg(test)]
mod tests {

    use super::*;
    use syn::parse_quote;

    #[test]
    #[should_panic(expected = "Wildcards can't be used as the starting state.")]
    fn wildcard_used_as_start() {
        let _: InputState = parse_quote! {
            *_
        };
    }

    #[test]
    fn input_state_with_data() {
        let state: InputState = parse_quote! {
            *Start(u8)
        };

        assert!(state.start);
        assert!(!state.wildcard);
        assert!(state.data_type.is_some());
    }

    #[test]
    fn history_marker_is_not_state_data() {
        let state: InputState = parse_quote! { "idle"_s(H) };
        assert!(state.history);
        assert!(state.data_type.is_none());
    }

    #[test]
    fn parenthesized_initial_state_matches_cpp_spelling() {
        let state: InputState = parse_quote! { *("idle"_s) };
        assert!(state.start);
        assert_eq!(state.ident, "Idle");
    }

    #[test]
    #[should_panic(expected = "Wildcard states cannot have data associated with it.")]
    fn wildcard_with_data() {
        let _: InputState = parse_quote! {
            _(u8)
        };
    }

    #[test]
    #[should_panic(expected = "This is an invalid type for states.")]
    fn invalid_type() {
        let _: InputState = parse_quote! {
            State1(!)
        };
    }

    #[test]
    fn wildcard() {
        let wildcard: InputState = parse_quote! {
            _
        };

        assert!(wildcard.wildcard);
        assert!(!wildcard.start);
        assert!(wildcard.data_type.is_none());
    }

    #[test]
    fn start() {
        let start: InputState = parse_quote! {
            *Start
        };

        assert!(start.start);
        assert!(!start.wildcard);
        assert!(start.data_type.is_none());
    }

    #[test]
    fn state_without_data() {
        let state: InputState = parse_quote! {
            State
        };

        assert!(!state.start);
        assert!(!state.wildcard);
        assert!(state.data_type.is_none());
    }

    #[test]
    fn cpp_string_state_literal() {
        let state: InputState = syn::parse_str("\"fin wait 1\"_s").unwrap();
        assert_eq!(state.ident, "FinWait1");
    }

    #[test]
    fn state_with_data() {
        let state: InputState = parse_quote! {
            State(u8)
        };

        assert!(!state.start);
        assert!(!state.wildcard);
        assert!(state.data_type.is_some());
    }
}
