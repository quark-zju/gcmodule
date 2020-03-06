//! Provide `derive(Trace)` support for structures to implement
//! `gcmodule::Trace` interface.
//!
//! # Example
//!
//! ```
//! use gcmodule_derive::Trace;
//! use gcmodule::Trace;
//!
//! #[derive(Trace)]
//! struct S<T: Trace> {
//!     a: String,
//!     b: Option<T>,
//!
//!     #[trace(skip)] // ignore this field for Trace.
//!     c: MyType,
//! }
//!
//! struct MyType;
//! ```
extern crate proc_macro;

use proc_macro::TokenStream;
use quote::quote;
use quote::ToTokens;
use syn::Data;

#[proc_macro_derive(Trace, attributes(trace))]
pub fn gcmodule_trace_derive(input: TokenStream) -> TokenStream {
    let input = syn::parse_macro_input!(input as syn::DeriveInput);
    let (impl_generics, ty_generics, where_clause) = input.generics.split_for_impl();
    let ident = input.ident;
    let mut trace_fn_body = Vec::new();
    let mut is_type_tracked_fn_body = Vec::new();
    match input.data {
        Data::Struct(data) => {
            for (i, field) in data.fields.into_iter().enumerate() {
                if field.attrs.into_iter().any(is_skipped) {
                    continue;
                }
                let trace_field = match field.ident {
                    Some(ident) => quote! { self.#ident.trace(tracer); },
                    None => {
                        let i = syn::Index::from(i);
                        quote! { self.#i.trace(tracer); }
                    }
                };
                trace_fn_body.push(trace_field);
                let ty = field.ty;
                is_type_tracked_fn_body.push(quote! {
                    if <#ty as _gcmodule::Trace>::is_type_tracked() {
                        return true;
                    }
                });
            }
        }
        Data::Enum(_) | Data::Union(_) => {
            trace_fn_body.push(quote! {
                compile_error!("enum or union are not supported");
            });
        }
    };
    let generated = quote! {
        const _: () = {
            extern crate gcmodule as _gcmodule;
            impl #impl_generics _gcmodule::Trace for #ident #ty_generics #where_clause {
                fn trace(&self, tracer: &mut _gcmodule::Tracer) {
                    #( #trace_fn_body )*
                }
                fn is_type_tracked() -> bool {
                    #( #is_type_tracked_fn_body )*
                    false
                }
            }
        };
    };
    generated.into()
}

fn is_skipped(attr: syn::Attribute) -> bool {
    // check if `#[trace(skip)]` exists.
    if attr.path.to_token_stream().to_string() == "trace" {
        for token in attr.tokens {
            if token.to_string() == "(skip)" {
                return true;
            }
        }
    }
    false
}
