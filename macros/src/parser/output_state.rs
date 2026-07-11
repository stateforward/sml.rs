use proc_macro2::Span;
use syn::{parenthesized, parse, spanned::Spanned, token, Ident, LitStr, Token, Type};

#[derive(Debug, Clone)]
pub struct OutputState {
    pub ident: Ident,
    pub internal_transition: bool,
    pub data_type: Option<Type>,
    pub composite: Option<Ident>,
}

impl parse::Parse for OutputState {
    fn parse(input: parse::ParseStream) -> syn::Result<Self> {
        if input.peek(Token![=]) {
            input.parse::<Token![=]>()?;
            let (internal_transition, mut ident) = if input.peek(Token![_]) {
                // Underscore ident here is used to represent an internal transition
                let underscore = input.parse::<Token![_]>()?;
                (true, underscore.into())
            } else if input.peek(LitStr) {
                let state: LitStr = input.parse()?;
                (
                    false,
                    crate::parser::state_ident(&state.value(), state.span()),
                )
            } else {
                (false, input.parse()?)
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

            // Possible type on the output state
            let data_type = if !internal_transition && input.peek(token::Paren) {
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
                            "This is an invalid type for states.",
                        ))
                    }
                }

                Some(input)
            } else {
                inferred_data_type
            };

            Ok(Self {
                ident,
                internal_transition,
                data_type,
                composite,
            })
        } else {
            // Internal transition
            Ok(Self {
                ident: Ident::new("_", Span::call_site()),
                internal_transition: true,
                data_type: None,
                composite: None,
            })
        }
    }
}
