use crate::proc_macro::TokenStream;

use proc_macro2::Span;
use quote::quote;
use syn::parse_macro_input;

use crate::parse::{CmdAttributes, ParseInput};

pub fn atat_cmd(input: TokenStream) -> TokenStream {
    let ParseInput {
        ident,
        at_cmd,
        generics,
        variants,
        ..
    } = parse_macro_input!(input as ParseInput);

    let CmdAttributes {
        cmd,
        resp,
        parse,
        timeout_ms,
        attempts,
        reattempt_on_parse_err,
        abortable,
        response_code,
        expects_prompt,
        value_sep,
        cmd_prefix,
        termination,
        escape_strings,
    } = at_cmd.expect("missing #[at_cmd(...)] attribute");

    let ident_str = ident.to_string();

    let cmd_variants: Vec<_> = variants
        .iter()
        .filter(|field| field.attrs.at_data.is_none())
        .cloned()
        .collect();
    let data_variants: Vec<_> = variants
        .iter()
        .filter(|field| field.attrs.at_data.is_some())
        .cloned()
        .collect();

    let data_variant = data_variants.first();
    let inferred_prompt = data_variant.is_some();
    let effective_prompt = expects_prompt || inferred_prompt;

    if data_variants.len() > 1 {
        return syn::Error::new(Span::call_site(), "only one #[at_data] field is supported")
            .to_compile_error()
            .into();
    }

    if effective_prompt && matches!(response_code, Some(false)) {
        return syn::Error::new(
            Span::call_site(),
            "expects_prompt = true requires response_code to remain enabled",
        )
        .to_compile_error()
        .into();
    }

    let data_position =
        data_variant.and_then(|field| field.attrs.at_data.as_ref().map(|data| data.position));
    let n_fields = cmd_variants.len() + usize::from(data_position.is_some());

    let (impl_generics, ty_generics, where_clause) = generics.split_for_impl();

    let timeout = match timeout_ms {
        Some(timeout_ms) => {
            quote! {
                const MAX_TIMEOUT_MS: u32 = #timeout_ms;
            }
        }
        None => quote! {},
    };

    let abortable = match abortable {
        Some(abortable) => {
            quote! {
                const CAN_ABORT: bool = #abortable;
            }
        }
        None => quote! {},
    };

    let attempts = match attempts {
        Some(attempts) => {
            quote! {
                const ATTEMPTS: u8 = #attempts;
            }
        }
        None => quote! {},
    };

    let response = match response_code {
        Some(is_resp) => {
            quote! {
                const EXPECTS_RESPONSE_CODE: bool = #is_resp;
            }
        }
        None => quote! {},
    };

    let reattempt_on_parse_err = match reattempt_on_parse_err {
        Some(reattempt_on_parse_err) => {
            quote! {
                const REATTEMPT_ON_PARSE_ERR: bool = #reattempt_on_parse_err;
            }
        }
        None => quote! {},
    };

    let expects_prompt = if effective_prompt {
        quote! {
            const EXPECTS_PROMPT: bool = true;
        }
    } else {
        quote! {}
    };

    let serialized_fields: Vec<_> = cmd_variants
        .iter()
        .map(|field| {
            let field_name = field.ident.clone().unwrap();
            let field_name_str = field_name.to_string();
            let position = field
                .attrs
                .at_arg
                .as_ref()
                .and_then(|arg| arg.position)
                .unwrap_or(usize::MAX);
            (
                position,
                quote! {
                    atat::serde_at::serde::ser::SerializeStruct::serialize_field(
                        &mut serde_state,
                        #field_name_str,
                        &self.#field_name,
                    )?;
                },
            )
        })
        .collect();

    let mut serialized_fields = serialized_fields;
    if let Some(data_field) = data_variant {
        let field_name = data_field.ident.clone().unwrap();
        let position = data_field.attrs.at_data.as_ref().unwrap().position;
        serialized_fields.push((
            position,
            quote! {
                let at_data_len = self.#field_name.as_ref().len();
                atat::serde_at::serde::ser::SerializeStruct::serialize_field(
                    &mut serde_state,
                    stringify!(#field_name),
                    &at_data_len,
                )?;
            },
        ));
    }
    serialized_fields.sort_by_key(|(position, _)| *position);
    let serialized_fields: Vec<_> = serialized_fields
        .into_iter()
        .map(|(_, tokens)| tokens)
        .collect();

    let payload = if let Some(data_field) = data_variant {
        let field_name = data_field.ident.clone().unwrap();
        quote! {
            #[inline]
            fn payload(&self) -> &[u8] {
                self.#field_name.as_ref()
            }
        }
    } else {
        quote! {}
    };

    let parse = if let Some(parse) = parse {
        quote! {
            #[inline]
            fn parse(&self, res: Result<&[u8], atat::InternalError>) -> core::result::Result<Self::Response, atat::Error> {
                match res {
                    Ok(resp) => #parse(resp).map_err(|e| {
                        atat::Error::Parse
                    }),
                    Err(e) => Err(e.into())
                }
            }
        }
    } else {
        quote! {
            #[inline]
           fn parse(&self, res: Result<&[u8], atat::InternalError>) -> core::result::Result<Self::Response, atat::Error> {
               match res {
                   Ok(resp) => atat::serde_at::from_slice::<#resp>(resp).map_err(|e| {
                       atat::Error::Parse
                   }),
                   Err(e) => Err(e.into())
               }
           }
        }
    };

    TokenStream::from(quote! {
        #[automatically_derived]
        impl #impl_generics atat::AtatCmd for #ident #ty_generics #where_clause {
            type Response = #resp;

            #timeout

            #abortable

            #attempts

            #response

            #expects_prompt

            #reattempt_on_parse_err

            #[inline]
            fn write(&self, buf: &mut [u8]) -> usize {
                match atat::serde_at::to_slice(self, #cmd, buf, atat::serde_at::SerializeOptions {
                    value_sep: #value_sep,
                    cmd_prefix: #cmd_prefix,
                    termination: #termination,
                    escape_strings: #escape_strings
                }) {
                    Ok(s) => s,
                    Err(_) => panic!("Failed to serialize command")
                }
            }

            #parse

            #payload
        }

        #[automatically_derived]
        impl #impl_generics atat::serde_at::serde::Serialize for #ident #ty_generics #where_clause {
            #[inline]
            fn serialize<S>(
                &self,
                serializer: S,
            ) -> core::result::Result<S::Ok, S::Error>
            where
                S: atat::serde_at::serde::Serializer,
            {
                let mut serde_state = atat::serde_at::serde::Serializer::serialize_struct(
                    serializer,
                    #ident_str,
                    #n_fields,
                )?;

                #(#serialized_fields)*

                atat::serde_at::serde::ser::SerializeStruct::end(serde_state)
            }
        }
    })
}
