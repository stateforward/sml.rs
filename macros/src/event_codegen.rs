use crate::parser::event::EventKind;
use proc_macro2::{Ident, TokenStream};
use quote::{quote, ToTokens};
use std::collections::BTreeMap;
use syn::Type;

#[derive(Clone)]
pub(crate) struct EventSpec {
    pub(crate) ty: Type,
    pub(crate) name: String,
}

pub(crate) type EventSpecs = BTreeMap<String, EventSpec>;

pub(crate) fn is_query_event(kind: EventKind, wildcard: bool) -> bool {
    matches!(kind, EventKind::Normal | EventKind::Unexpected) && !wildcard
}

pub(crate) fn is_static_type(ty: &Type) -> bool {
    crate::parser::lifetimes::Lifetimes::from_type(ty)
        .map(|lifetimes| {
            lifetimes
                .as_slice()
                .iter()
                .all(|lifetime| lifetime.ident == "static")
        })
        .unwrap_or(false)
}

pub(crate) fn add_spec(
    specs: &mut EventSpecs,
    ident: &Ident,
    kind: EventKind,
    wildcard: bool,
    ty: Option<&Type>,
) {
    if !is_query_event(kind, wildcard) {
        return;
    }
    let Some(ty) = ty else { return };
    if !is_static_type(ty) {
        return;
    }

    let key = ty.to_token_stream().to_string();
    let name = ident.to_string();
    match specs.get(&key) {
        Some(existing) if existing.name <= name => {}
        _ => {
            specs.insert(
                key,
                EventSpec {
                    ty: ty.clone(),
                    name,
                },
            );
        }
    }
}

pub(crate) fn visit_calls(specs: &EventSpecs) -> TokenStream {
    specs
        .values()
        .map(|spec| {
            let ty = &spec.ty;
            quote! { visitor.visit::<#ty>(); }
        })
        .collect()
}

pub(crate) fn names_table(
    specs: &EventSpecs,
    events_name: &Ident,
    generics: &syn::Generics,
) -> TokenStream {
    let names = specs.values().map(|spec| &spec.name);
    let (impl_generics, type_generics, where_clause) = generics.split_for_impl();
    quote! {
        impl #impl_generics #events_name #type_generics #where_clause {
            /// The generated variant names for the typed event payloads
            /// in this machine, in deterministic order.
            pub const EVENT_NAMES: &'static [&'static str] = &[#(#names),*];
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use syn::parse_quote;

    #[test]
    fn accepts_static_types_and_rejects_borrowed_types() {
        assert!(is_static_type(&parse_quote!(Event)));
        assert!(is_static_type(&parse_quote!(&'static Event)));
        assert!(!is_static_type(&parse_quote!(&'event Event)));
    }

    #[test]
    fn excludes_lifecycle_and_wildcard_triggers() {
        let mut specs = EventSpecs::new();
        let ident = Ident::new("Event", proc_macro2::Span::call_site());
        add_spec(
            &mut specs,
            &ident,
            EventKind::Entry,
            false,
            Some(&parse_quote!(Event)),
        );
        add_spec(
            &mut specs,
            &ident,
            EventKind::Unexpected,
            true,
            Some(&parse_quote!(Event)),
        );
        assert!(specs.is_empty());
    }
}
