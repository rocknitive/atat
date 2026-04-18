use crate::proc_macro::TokenStream;

use quote::quote;
use syn::{parse_macro_input, Fields};

use crate::parse::{ParseInput, UrcAttributes};

pub fn atat_urc(input: TokenStream) -> TokenStream {
    let ParseInput {
        ident,
        generics,
        variants,
        ..
    } = parse_macro_input!(input as ParseInput);

    let (impl_generics, ty_generics, where_clause) = generics.split_for_impl();

    assert!(!variants.is_empty(), "there must be at least one variant");

    let (dispatch_arms, digest_arms): (Vec<_>, Vec<_>) = variants.iter().map(|variant| {
        let UrcAttributes {
            code,
            parse,
            digest,
        } = variant.attrs.at_urc.clone().unwrap_or_else(|| {
            panic!(
                "missing #[at_urc(...)] attribute",
            )
        });

        let variant_ident = variant.ident.clone();
        let dispatch_arm = match variant.fields.clone() {
            Some(Fields::Named(_)) => {
                panic!("cannot handle named enum variants")
            }
            Some(Fields::Unnamed(f)) => {
                let mut field_iter = f.unnamed.iter();
                let first_field = field_iter.next().expect("variant must have exactly one field");
                assert!(field_iter.next().is_none(), "cannot handle variants with more than one field");
                quote! {
                    if resp.starts_with(&#code[..]) {
                        return Some(#ident::#variant_ident(atat::serde_at::from_slice::<#first_field>(&resp).ok()?));
                    }
                }
            }
            Some(Fields::Unit) => {
                quote! {
                    if resp.starts_with(&#code[..]) {
                        return Some(#ident::#variant_ident);
                    }
                }
            }
            None => {
                panic!()
            }
        };

        let digest_arm = if let Some(digest_fn) = digest {
            quote! {
                match #digest_fn(buf) {
                    Ok(r) => return Ok(r),
                    Err(atat::digest::ParseError::Incomplete) => {
                        return Err(atat::digest::ParseError::Incomplete)
                    }
                    Err(atat::digest::ParseError::NoMatch) => {}
                }
            }
        } else if let Some(parse_fn) = parse {
            quote! {
                match #parse_fn(&#code[..])(buf) {
                    Ok((_, r)) => return Ok(r),
                    Err(e) => match atat::digest::ParseError::from(e) {
                        atat::digest::ParseError::Incomplete => {
                            return Err(atat::digest::ParseError::Incomplete)
                        }
                        atat::digest::ParseError::NoMatch => {}
                    }
                }
            }
        } else {
            quote! {
                match atat::digest::parser::urc_helper(&#code[..])(buf) {
                    Ok((_, r)) => return Ok(r),
                    Err(e) => match atat::digest::ParseError::from(e) {
                        atat::digest::ParseError::Incomplete => {
                            return Err(atat::digest::ParseError::Incomplete)
                        }
                        atat::digest::ParseError::NoMatch => {}
                    }
                }
            }
        };

        (dispatch_arm, digest_arm)
    }).unzip();

    TokenStream::from(quote! {
        #[automatically_derived]
        impl #impl_generics atat::AtatUrc for #ident #ty_generics #where_clause {
            type Response = #ident;

            #[inline]
            fn parse(resp: &[u8]) -> Option<Self::Response> {
                #(
                    #dispatch_arms
                )*

                None
            }
        }

        #[automatically_derived]
        impl #impl_generics atat::Parser for #ident #ty_generics #where_clause {
            fn parse<'a>(
                buf: &'a [u8],
            ) -> Result<(&'a [u8], usize), atat::digest::ParseError> {
                #(#digest_arms)*

                Err(atat::digest::ParseError::NoMatch)
            }
        }
    })
}
