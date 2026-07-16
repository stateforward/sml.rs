use proc_macro2::TokenStream;
use quote::{quote, ToTokens};
use std::ops::Sub;
use syn::visit::{self, Visit};
use syn::{parse, spanned::Spanned, Lifetime, Type, TypeReference};

#[derive(Default, Debug, Clone)]
pub struct Lifetimes {
    lifetimes: Vec<Lifetime>,
}

impl Lifetimes {
    pub fn new() -> Lifetimes {
        Lifetimes {
            lifetimes: Vec::new(),
        }
    }

    pub fn from_type(data_type: &Type) -> Result<Lifetimes, parse::Error> {
        let mut lifetimes = Lifetimes::new();
        lifetimes.insert_from_type(data_type)?;
        Ok(lifetimes)
    }

    pub fn insert(&mut self, lifetime: &Lifetime) {
        // `'static` is a concrete lifetime argument, not a lifetime parameter that
        // can be repeated on a generated item.
        if lifetime.ident == "static" {
            return;
        }
        if !self.lifetimes.contains(lifetime) {
            self.lifetimes.push(lifetime.to_owned());
        }
    }

    pub fn extend(&mut self, other: &Lifetimes) {
        for lifetime in other.lifetimes.iter() {
            self.insert(lifetime);
        }
    }

    pub fn is_empty(&self) -> bool {
        self.lifetimes.is_empty()
    }

    pub fn as_slice(&self) -> &[Lifetime] {
        &self.lifetimes[..]
    }

    /// Extracts lifetimes from a [`Type`]
    pub fn insert_from_type(&mut self, data_type: &Type) -> Result<(), parse::Error> {
        struct Collector<'a> {
            lifetimes: &'a mut Lifetimes,
            missing_reference_lifetime: Option<proc_macro2::Span>,
        }

        impl<'ast> Visit<'ast> for Collector<'_> {
            fn visit_lifetime(&mut self, lifetime: &'ast Lifetime) {
                self.lifetimes.insert(lifetime);
            }

            fn visit_type_reference(&mut self, reference: &'ast TypeReference) {
                if reference.lifetime.is_none() && self.missing_reference_lifetime.is_none() {
                    self.missing_reference_lifetime = Some(reference.span());
                }
                visit::visit_type_reference(self, reference);
            }
        }

        let mut collector = Collector {
            lifetimes: self,
            missing_reference_lifetime: None,
        };
        collector.visit_type(data_type);
        collector.missing_reference_lifetime.map_or(Ok(()), |span| {
            Err(parse::Error::new(
                span,
                "This event's data lifetime is not defined, consider adding a lifetime.",
            ))
        })
    }
}

impl ToTokens for Lifetimes {
    fn to_tokens(&self, tokens: &mut TokenStream) {
        if self.is_empty() {
            return;
        }

        let lifetimes = self.as_slice();
        tokens.extend(quote! { #(#lifetimes),* ,});
    }
}

impl Sub<&Lifetimes> for Lifetimes {
    type Output = Lifetimes;

    fn sub(mut self, rhs: &Lifetimes) -> Lifetimes {
        self.lifetimes.retain(|lt| !rhs.lifetimes.contains(lt));
        self
    }
}

impl Sub for Lifetimes {
    type Output = Lifetimes;

    fn sub(self, rhs: Lifetimes) -> Lifetimes {
        self.sub(&rhs)
    }
}

impl Sub<&Lifetimes> for &Lifetimes {
    type Output = Lifetimes;

    fn sub(self, rhs: &Lifetimes) -> Lifetimes {
        self.to_owned().sub(rhs)
    }
}

impl Sub<Lifetimes> for &Lifetimes {
    type Output = Lifetimes;

    fn sub(self, rhs: Lifetimes) -> Lifetimes {
        self.to_owned().sub(&rhs)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use quote::quote;

    fn ty(source: &str) -> Type {
        syn::parse_str(source).unwrap()
    }

    #[test]
    fn extracts_nested_and_generic_lifetimes_without_duplicates() {
        let lifetimes =
            Lifetimes::from_type(&ty("(&'a str, Wrapper<'b, Option<&'c u8>, &'a u8>)")).unwrap();
        assert_eq!(lifetimes.as_slice().len(), 3);
        assert_eq!(quote!(#lifetimes).to_string(), "'a , 'b , 'c ,");
    }

    #[test]
    fn rejects_reference_without_explicit_lifetime() {
        let error = Lifetimes::from_type(&ty("&str")).unwrap_err();
        assert!(error.to_string().contains("lifetime is not defined"));
    }

    #[test]
    fn subtraction_forms_remove_shared_lifetimes() {
        let left = Lifetimes::from_type(&ty("Pair<'a, 'b>")).unwrap();
        let right = Lifetimes::from_type(&ty("Item<'b>")).unwrap();
        assert_eq!((left.clone() - right.clone()).as_slice().len(), 1);
        assert_eq!((left.clone() - &right).as_slice().len(), 1);
        assert_eq!((&left - right.clone()).as_slice().len(), 1);
        assert_eq!((&left - &right).as_slice().len(), 1);
    }

    #[test]
    fn empty_lifetimes_emit_no_tokens_and_other_types_are_ignored() {
        let empty = Lifetimes::from_type(&ty("u8")).unwrap();
        assert!(empty.is_empty());
        assert!(quote!(#empty).is_empty());
    }

    #[test]
    fn static_is_kept_concrete_instead_of_becoming_a_parameter() {
        let lifetimes = Lifetimes::from_type(&ty("Message<'static>")).unwrap();
        assert!(lifetimes.is_empty());
    }
}
