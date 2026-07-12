use crate::parser::transition::GuardExpression;
use crate::parser::AsyncIdent;
use syn::{parenthesized, parse, spanned::Spanned, token, Ident, LitStr, Token, Type};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EventKind {
    Normal,
    Unexpected,
    Completion,
    Entry,
    Exit,
    Exception,
}

#[derive(Debug, Clone)]
pub struct Event {
    pub ident: Ident,
    pub data_type: Option<Type>,
    pub kind: EventKind,
    pub wildcard: bool,
    pub external: bool,
}

#[derive(Debug)]
pub struct EventMapping {
    pub in_state: Ident,
    pub event: Ident,
    pub event_kind: EventKind,
    pub event_wildcard: bool,
    pub event_external: bool,
    pub transitions: Vec<Transition>,
}

#[derive(Debug)]
pub struct Transition {
    pub guard: Option<GuardExpression>,
    pub action: Option<AsyncIdent>,
    pub additional_actions: Vec<AsyncIdent>,
    pub process_events: Vec<syn::Expr>,
    pub defer: bool,
    pub eval_actions: Vec<crate::parser::transition::EvalAction>,
    pub default_output: bool,
    pub out_state: Ident,
    pub internal_transition: bool,
}

impl parse::Parse for Event {
    fn parse(input: parse::ParseStream) -> syn::Result<Self> {
        // Event
        input.parse::<Token![+]>()?;
        let mut first: Ident = if input.peek(LitStr) {
            let event: LitStr = input.parse()?;
            if event.suffix() != "_e" {
                return Err(parse::Error::new(
                    event.span(),
                    "named events use the sml.cpp suffix `_e`",
                ));
            }
            crate::parser::state_ident(&event.value(), event.span())
        } else {
            input.parse()?
        };
        if first == "sml" && input.peek(Token![::]) {
            input.parse::<Token![::]>()?;
            first = input.parse()?;
        }
        let (ident, kind, wildcard, external) = if first == "event" && input.peek(Token![<]) {
            input.parse::<Token![<]>()?;
            let ident = input.parse::<Ident>()?;
            input.parse::<Token![>]>()?;
            (ident, EventKind::Normal, false, true)
        } else if (first == "unexpected" || first == "unexpected_event") && input.peek(Token![<]) {
            input.parse::<Token![<]>()?;
            let (ident, wildcard) = if input.peek(Token![_]) {
                input.parse::<Token![_]>()?;
                (Ident::new("__AnyEvent", first.span()), true)
            } else {
                (input.parse::<Ident>()?, false)
            };
            input.parse::<Token![>]>()?;
            let external = first == "unexpected_event" && !wildcard;
            (ident, EventKind::Unexpected, wildcard, external)
        } else if (first == "on_entry" || first == "on_exit") && input.peek(Token![<]) {
            input.parse::<Token![<]>()?;
            input.parse::<Token![_]>()?;
            input.parse::<Token![>]>()?;
            let kind = if first == "on_entry" {
                EventKind::Entry
            } else {
                EventKind::Exit
            };
            (first, kind, false, false)
        } else if first == "completion" && input.peek(Token![<]) {
            input.parse::<Token![<]>()?;
            let (ident, wildcard) = if input.peek(Token![_]) {
                input.parse::<Token![_]>()?;
                (Ident::new("__AnyCompletion", first.span()), true)
            } else {
                (input.parse::<Ident>()?, false)
            };
            input.parse::<Token![>]>()?;
            (ident, EventKind::Completion, wildcard, false)
        } else if first == "exception" && input.peek(Token![<]) {
            input.parse::<Token![<]>()?;
            let (ident, wildcard) = if input.peek(Token![_]) {
                input.parse::<Token![_]>()?;
                (Ident::new("__AnyException", first.span()), true)
            } else {
                (input.parse::<Ident>()?, false)
            };
            input.parse::<Token![>]>()?;
            (ident, EventKind::Exception, wildcard, false)
        } else {
            (first, EventKind::Normal, false, false)
        };

        // Possible type on the event
        let data_type = if input.peek(token::Paren) {
            let content;
            parenthesized!(content in input);
            let input: Type = content.parse()?;

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
                        "This is an invalid type for events.",
                    ))
                }
            }

            Some(input)
        } else if external || (kind == EventKind::Exception && !wildcard) {
            let event_type: Type = syn::parse_quote_spanned!(ident.span()=> #ident);
            Some(event_type)
        } else {
            None
        };

        if wildcard && data_type.is_some() {
            return Err(parse::Error::new(
                ident.span(),
                "A wildcard trigger cannot carry event data.",
            ));
        }

        Ok(Self {
            ident,
            data_type,
            kind,
            wildcard,
            external,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::{Event, EventKind};
    use syn::parse_str;

    #[test]
    fn parses_normal_unexpected_and_completion_events() {
        let normal: Event = parse_str("+ Start").unwrap();
        assert_eq!(normal.kind, EventKind::Normal);
        assert_eq!(normal.ident, "Start");

        let named: Event = parse_str("+ \"connection established\"_e").unwrap();
        assert_eq!(named.kind, EventKind::Normal);
        assert_eq!(named.ident, "ConnectionEstablished");

        let unexpected: Event = parse_str("+ unexpected<Reset>").unwrap();
        assert_eq!(unexpected.kind, EventKind::Unexpected);
        assert!(!unexpected.wildcard);
        assert_eq!(unexpected.ident, "Reset");

        let wildcard: Event = parse_str("+ unexpected<_>").unwrap();
        assert_eq!(wildcard.kind, EventKind::Unexpected);
        assert!(wildcard.wildcard);

        let completion: Event = parse_str("+ completion<Start>(u32)").unwrap();
        assert_eq!(completion.kind, EventKind::Completion);
        assert_eq!(completion.ident, "Start");
        assert!(completion.data_type.is_some());

        let anonymous: Event = parse_str("+ completion<_>").unwrap();
        assert_eq!(anonymous.kind, EventKind::Completion);
        assert!(anonymous.wildcard);

        let qualified_entry: Event = parse_str("+ sml::on_entry<_>").unwrap();
        assert_eq!(qualified_entry.kind, EventKind::Entry);

        let exception: Event = parse_str("+ exception<_>").unwrap();
        assert_eq!(exception.kind, EventKind::Exception);
        assert!(exception.wildcard);
    }

    #[test]
    fn parses_all_event_kinds_and_external_types() {
        let external: Event = parse_str("+ event<Start>").unwrap();
        assert!(external.external);
        assert!(external.data_type.is_some());

        let unexpected: Event = parse_str("+ unexpected_event<Start>").unwrap();
        assert!(unexpected.external);
        assert!(unexpected.data_type.is_some());

        let wildcard: Event = parse_str("+ unexpected_event<_>").unwrap();
        assert!(!wildcard.external);
        assert!(wildcard.data_type.is_none());

        let exit: Event = parse_str("+ on_exit<_>").unwrap();
        assert_eq!(exit.kind, EventKind::Exit);

        let exception: Event = parse_str("+ exception<MyError>").unwrap();
        assert_eq!(exception.kind, EventKind::Exception);
        assert!(exception.data_type.is_some());
    }

    #[test]
    fn accepts_supported_payload_shapes() {
        for ty in [
            "[u8; 4]",
            "Payload",
            "*mut u8",
            "&'static u8",
            "[u8]",
            "(u8, u16)",
        ] {
            let source = format!("+ Start({ty})");
            assert!(parse_str::<Event>(&source).is_ok(), "{}", ty);
        }
    }

    #[test]
    fn rejects_bad_named_suffix_invalid_payload_and_wildcard_data() {
        assert!(parse_str::<Event>("+ \"bad name\"_s")
            .unwrap_err()
            .to_string()
            .contains("suffix `_e`"));
        assert!(parse_str::<Event>("+ Start(fn())")
            .unwrap_err()
            .to_string()
            .contains("invalid type"));
        assert!(parse_str::<Event>("+ unexpected<_>(u8)")
            .unwrap_err()
            .to_string()
            .contains("wildcard trigger"));
    }
}
