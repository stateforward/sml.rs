use super::event::Event;
use super::input_state::InputState;
use super::output_state::OutputState;
use super::AsyncIdent;
use proc_macro2::TokenStream;
use quote::quote;
use std::fmt;
use syn::{bracketed, parse, token, Expr, Ident, Token};

#[derive(Debug, Clone)]
pub struct StateTransition {
    pub in_state: InputState,
    pub event: Event,
    pub guard: Option<GuardExpression>,
    pub action: Option<AsyncIdent>,
    pub additional_actions: Vec<AsyncIdent>,
    pub process_events: Vec<Expr>,
    pub defer: bool,
    pub eval_actions: Vec<EvalAction>,
    pub out_state: OutputState,
    pub internal_transition: bool,
}

#[derive(Debug)]
pub struct StateTransitions {
    pub in_states: Vec<InputState>,
    pub event: Event,
    pub guard: Option<GuardExpression>,
    pub action: Option<AsyncIdent>,
    pub additional_actions: Vec<AsyncIdent>,
    pub process_events: Vec<Expr>,
    pub defer: bool,
    pub eval_actions: Vec<EvalAction>,
    pub out_state: OutputState,
}

#[derive(Debug, Clone)]
pub struct EvalAction {
    pub position: usize,
    pub guard: GuardExpression,
    pub action: AsyncIdent,
}

impl parse::Parse for StateTransitions {
    fn parse(input: parse::ParseStream) -> syn::Result<Self> {
        // parse the input pattern
        let mut in_states = Vec::new();
        loop {
            let in_state: InputState = input.parse()?;
            in_states.push(in_state);
            if input.parse::<Token![|]>().is_err() {
                break;
            };
        }

        // Make sure that if a wildcard is used, it is the only input state
        if in_states.len() > 1 {
            for in_state in &in_states {
                if in_state.wildcard {
                    return Err(parse::Error::new(
                        in_state.ident.span(),
                        "Wildcards already include all states, so should not be used with input state patterns.",
                    ));
                }
            }
        }
        // Event
        let event: Event = input.parse()?;

        // Possible guard
        let guard = if input.peek(token::Bracket) {
            let content;
            bracketed!(content in input);
            Some(GuardExpression::parse(&content)?)
        } else {
            None
        };

        // Possible action
        let (action, additional_actions, process_events, defer, eval_actions) =
            if input.parse::<Token![/]>().is_ok() {
                let mut actions = Vec::new();
                let mut process_events = Vec::new();
                let mut defer = false;
                let mut eval_actions = Vec::new();
                let mut position = 0;
                if input.peek(token::Paren) {
                    let content;
                    syn::parenthesized!(content in input);
                    while !content.is_empty() {
                        let is_async = content.parse::<token::Async>().is_ok();
                        let ident: Ident = content.parse()?;
                        if ident == "eval" && content.peek(token::Bracket) {
                            if is_async {
                                return Err(parse::Error::new(
                                    ident.span(),
                                    "`eval` is an action-sequence operator, not an async callback",
                                ));
                            }
                            let guard_content;
                            bracketed!(guard_content in content);
                            let guard = GuardExpression::parse(&guard_content)?;
                            content.parse::<Token![/]>()?;
                            let action_async = content.parse::<token::Async>().is_ok();
                            eval_actions.push(EvalAction {
                                position,
                                guard,
                                action: AsyncIdent {
                                    ident: content.parse()?,
                                    is_async: action_async,
                                },
                            });
                        } else if ident == "defer" {
                            if is_async {
                                return Err(parse::Error::new(
                                    ident.span(),
                                    "`defer` is a queue operation, not an async callback",
                                ));
                            }
                            defer = true;
                        } else if ident == "process" && content.peek(token::Paren) {
                            if is_async {
                                return Err(parse::Error::new(
                                    ident.span(),
                                    "`process(...)` is a queue operation, not an async callback",
                                ));
                            }
                            let event;
                            syn::parenthesized!(event in content);
                            process_events.push(event.parse()?);
                        } else {
                            actions.push(AsyncIdent { ident, is_async });
                        }
                        position += 1;
                        if content.is_empty() {
                            break;
                        }
                        content.parse::<Token![,]>()?;
                    }
                } else {
                    let is_async = input.parse::<token::Async>().is_ok();
                    let ident: Ident = input.parse()?;
                    if ident == "defer" {
                        if is_async {
                            return Err(parse::Error::new(
                                ident.span(),
                                "`defer` is a queue operation, not an async callback",
                            ));
                        }
                        defer = true;
                    } else if ident == "process" && input.peek(token::Paren) {
                        if is_async {
                            return Err(parse::Error::new(
                                ident.span(),
                                "`process(...)` is a queue operation, not an async callback",
                            ));
                        }
                        let content;
                        syn::parenthesized!(content in input);
                        process_events.push(content.parse()?);
                    } else {
                        actions.push(AsyncIdent { ident, is_async });
                    }
                }
                let action = actions.first().cloned();
                let additional = actions.into_iter().skip(1).collect();
                (action, additional, process_events, defer, eval_actions)
            } else {
                (None, Vec::new(), Vec::new(), false, Vec::new())
            };

        let out_state: OutputState = input.parse()?;

        Ok(Self {
            in_states,
            event,
            guard,
            action,
            additional_actions,
            process_events,
            defer,
            eval_actions,
            out_state,
        })
    }
}
#[derive(Debug, Clone)]
pub enum GuardExpression {
    Guard(AsyncIdent),
    Not(Box<GuardExpression>),
    Group(Box<GuardExpression>),
    And(Box<GuardExpression>, Box<GuardExpression>),
    Or(Box<GuardExpression>, Box<GuardExpression>),
}
impl fmt::Display for GuardExpression {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            GuardExpression::Guard(async_ident) => write!(f, "{}", async_ident),
            GuardExpression::Not(expr) => write!(f, "!{}", expr),
            GuardExpression::Group(expr) => write!(f, "({})", expr),
            GuardExpression::And(lhs, rhs) => {
                write!(f, "{} && {}", lhs, rhs)
            }
            GuardExpression::Or(lhs, rhs) => {
                write!(f, "{} || {}", lhs, rhs)
            }
        }
    }
}
impl GuardExpression {
    pub fn to_token_stream<F>(&self, visit: &mut F) -> TokenStream
    where
        F: FnMut(&AsyncIdent) -> TokenStream,
    {
        match self {
            GuardExpression::Guard(async_ident) => async_ident.to_token_stream(visit),
            GuardExpression::Not(expr) => {
                let expr_tokens = expr.to_token_stream(visit);
                quote! { !#expr_tokens }
            }
            GuardExpression::Group(expr) => {
                let expr_tokens = expr.to_token_stream(visit);
                quote! { (#expr_tokens) }
            }
            GuardExpression::And(lhs, rhs) => {
                let lhs_tokens = lhs.to_token_stream(visit);
                let rhs_tokens = rhs.to_token_stream(visit);
                quote! { #lhs_tokens && #rhs_tokens }
            }
            GuardExpression::Or(lhs, rhs) => {
                let lhs_tokens = lhs.to_token_stream(visit);
                let rhs_tokens = rhs.to_token_stream(visit);
                quote! { #lhs_tokens || #rhs_tokens }
            }
        }
    }
}

pub fn visit_guards<F>(expr: &GuardExpression, mut visit_guard: F) -> Result<(), parse::Error>
where
    F: FnMut(&AsyncIdent) -> Result<(), parse::Error>,
{
    let mut stack = vec![expr];
    while let Some(node) = stack.pop() {
        match node {
            GuardExpression::Guard(guard) => {
                visit_guard(guard)?;
            }
            GuardExpression::Not(inner) | GuardExpression::Group(inner) => {
                stack.push(inner.as_ref());
            }
            GuardExpression::And(left, right) | GuardExpression::Or(left, right) => {
                stack.push(left.as_ref());
                stack.push(right.as_ref());
            }
        }
    }
    Ok(())
}

impl parse::Parse for GuardExpression {
    fn parse(input: parse::ParseStream) -> syn::Result<Self> {
        parse_or(input)
    }
}

fn parse_or(input: parse::ParseStream) -> syn::Result<GuardExpression> {
    let mut left = parse_and(input)?;
    while input.peek(Token![||]) {
        let _or: Token![||] = input.parse()?;
        let right = parse_and(input)?;
        left = GuardExpression::Or(Box::new(left), Box::new(right));
    }
    Ok(left)
}

fn parse_and(input: parse::ParseStream) -> syn::Result<GuardExpression> {
    let mut left = parse_not(input)?;
    while input.peek(Token![&&]) {
        let _and: Token![&&] = input.parse()?;
        let right = parse_not(input)?;
        left = GuardExpression::And(Box::new(left), Box::new(right));
    }
    Ok(left)
}

fn parse_not(input: parse::ParseStream) -> syn::Result<GuardExpression> {
    if input.peek(Token![!]) {
        let _not: Token![!] = input.parse()?;
        let expr = parse_primary(input)?;
        return Ok(GuardExpression::Not(Box::new(expr)));
    }
    parse_primary(input)
}

fn parse_primary(input: parse::ParseStream) -> syn::Result<GuardExpression> {
    if input.peek(token::Paren) {
        let content;
        syn::parenthesized!(content in input);
        let expr = parse_or(&content)?;
        return Ok(GuardExpression::Group(Box::new(expr)));
    }

    if input.peek(Token![async]) {
        let _async: Token![async] = input.parse()?;
        let ident: Ident = input.parse()?;
        return Ok(GuardExpression::Guard(AsyncIdent {
            ident,
            is_async: true,
        }));
    }

    let ident: Ident = input.parse()?;
    Ok(GuardExpression::Guard(AsyncIdent {
        ident,
        is_async: false,
    }))
}

#[cfg(test)]
mod test {
    use crate::parser::transition::{visit_guards, GuardExpression, StateTransitions};
    use quote::quote;
    use syn::parse_str;

    #[test]
    fn bad_guard_expression() {
        let guard_expression = "a && b c";
        assert!(parse_str::<GuardExpression>(guard_expression).is_err());
    }
    #[test]
    fn guard_expressions() -> Result<(), syn::Error> {
        for (guard_expression_str, expected) in vec![
            ("guard", "guard()"),
            ("async guard", "guard().await"),
            ("async a || async b", "a().await || b().await"),
            ("!guard", "!guard()"),
            ("a && b", "a() && b()"),
            ("a || b", "a() || b()"),
            ("a || b || c", "a() || b() || c()"),
            ("a || b && c || d", "a() || b() && c() || d()"),
            ("(a || b) && (c || d)", "(a() || b()) && (c() || d())"),
            ("a && b || c && d", "a() && b() || c() && d()"),
            (
                "a && ( !b && c ) || d && e",
                "a() && (!b() && c()) || d() && e()",
            ),
        ] {
            let guard_expression: GuardExpression = parse_str(guard_expression_str)?;
            assert_eq!(guard_expression.to_string(), expected);
        }
        Ok(())
    }

    #[test]
    fn guard_tokens_and_visitor_cover_the_expression_tree() {
        let expression: GuardExpression = parse_str("!(async a || b) && c").unwrap();
        let tokens = expression.to_token_stream(&mut |guard| {
            let ident = &guard.ident;
            if guard.is_async {
                quote!(#ident().await)
            } else {
                quote!(#ident())
            }
        });
        assert_eq!(tokens.to_string(), "! (a () . await || b ()) && c ()");

        let mut visited = Vec::new();
        visit_guards(&expression, |guard| {
            visited.push(guard.ident.to_string());
            Ok(())
        })
        .unwrap();
        visited.sort();
        assert_eq!(visited, ["a", "b", "c"]);

        assert!(visit_guards(&expression, |_guard| {
            Err(syn::Error::new(proc_macro2::Span::call_site(), "stop"))
        })
        .is_err());
    }

    #[test]
    fn parses_action_sequence_queue_operations_and_eval() {
        let transition: StateTransitions = parse_str(
            "Idle | Ready + Start [allowed] / (first, async second, process(Next {}), defer, eval[ready] / async third) = Running",
        )
        .unwrap();
        assert_eq!(transition.in_states.len(), 2);
        assert_eq!(transition.action.unwrap().ident, "first");
        assert_eq!(transition.additional_actions.len(), 1);
        assert_eq!(transition.process_events.len(), 1);
        assert!(transition.defer);
        assert_eq!(transition.eval_actions.len(), 1);
        assert_eq!(transition.eval_actions[0].position, 4);
        assert!(transition.eval_actions[0].action.is_async);
    }

    #[test]
    fn parses_single_actions_and_internal_transitions() {
        let asynchronous: StateTransitions =
            parse_str("Idle + Start / async begin = Ready").unwrap();
        assert!(asynchronous.action.unwrap().is_async);

        let process: StateTransitions = parse_str("Idle + Start / process(Next {})").unwrap();
        assert_eq!(process.process_events.len(), 1);
        assert!(process.out_state.internal_transition);

        let defer: StateTransitions = parse_str("Idle + Start / defer").unwrap();
        assert!(defer.defer);

        let bare: StateTransitions = parse_str("Idle + Start").unwrap();
        assert!(bare.action.is_none());
        assert!(bare.out_state.internal_transition);
    }

    #[test]
    fn rejects_wildcard_patterns_and_async_queue_operators() {
        assert!(parse_str::<StateTransitions>("_ | Idle + Start").is_err());
        for source in [
            "Idle + Start / async defer",
            "Idle + Start / async process(Next {})",
            "Idle + Start / (async defer)",
            "Idle + Start / (async process(Next {}))",
            "Idle + Start / (async eval[ready] / run)",
        ] {
            assert!(parse_str::<StateTransitions>(source).is_err(), "{}", source);
        }
    }
}
