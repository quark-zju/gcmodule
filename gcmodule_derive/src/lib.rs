//! Provide `derive(Trace)` support for structures to implement
//! `gcmodule::Trace` interface.
//!
//! # Example
//!
//! ```
//! use gcmodule_derive::Trace;
//!
//! #[derive(Trace)]
//! struct S<T: gcmodule::Trace> {
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
use proc_macro2::TokenStream as TokenStream2;
use quote::{format_ident, quote};
use syn::{
    parenthesized,
    parse::{Parse, ParseStream},
    parse_macro_input,
    spanned::Spanned,
    Attribute, Data, DeriveInput, Error, Field, Fields, Ident, Path, Result,
};

mod kw {
    syn::custom_keyword!(trace);
    syn::custom_keyword!(skip);
    syn::custom_keyword!(with);
    syn::custom_keyword!(tracking);
    syn::custom_keyword!(ignore);
    syn::custom_keyword!(force);
}

enum TraceAttr {
    Skip,
    With(Path),
    TrackingForce(bool),
}
impl TraceAttr {
    fn force_is_type_tracked(&self) -> Option<TokenStream2> {
        match self {
            Self::TrackingForce(v) => Some(quote! {#v}),
            Self::Skip => Some(quote! {false}),
            Self::With(_) => Some(quote! {true}),
        }
    }
}
impl Parse for TraceAttr {
    fn parse(input: ParseStream) -> Result<Self> {
        let lookahead = input.lookahead1();
        if lookahead.peek(kw::skip) {
            input.parse::<kw::skip>()?;
            Ok(Self::Skip)
        } else if lookahead.peek(kw::tracking) {
            input.parse::<kw::tracking>()?;
            let content;
            parenthesized!(content in input);
            let lookahead = content.lookahead1();
            if lookahead.peek(kw::ignore) {
                content.parse::<kw::ignore>()?;
                Ok(Self::TrackingForce(false))
            } else if lookahead.peek(kw::force) {
                content.parse::<kw::force>()?;
                Ok(Self::TrackingForce(true))
            } else {
                Err(lookahead.error())
            }
        } else if lookahead.peek(kw::with) {
            input.parse::<kw::with>()?;
            let content;
            parenthesized!(content in input);
            Ok(Self::With(content.parse()?))
        } else {
            Err(lookahead.error())
        }
    }
}

fn parse_attr<A: Parse, I>(attrs: &[Attribute], ident: I) -> Result<Option<A>>
where
    Ident: PartialEq<I>,
{
    let attrs = attrs
        .iter()
        .filter(|a| a.path.is_ident(&ident))
        .collect::<Vec<_>>();
    if attrs.len() > 1 {
        return Err(Error::new(
            attrs[1].span(),
            "this attribute may be specified only once",
        ));
    } else if attrs.is_empty() {
        return Ok(None);
    }
    let attr = attrs[0];
    let attr = attr.parse_args::<A>()?;

    Ok(Some(attr))
}

/// Returns impl for (trace, is_type_tracked)
fn derive_fields(
    trace_attr: &Option<TraceAttr>,
    fields: &Fields,
) -> Result<(TokenStream2, TokenStream2)> {
    fn inner<'a>(names: &[Ident], fields: Vec<&Field>) -> Result<(TokenStream2, TokenStream2)> {
        let attrs = fields
            .iter()
            .map(|f| parse_attr::<TraceAttr, _>(&f.attrs, "trace"))
            .collect::<Result<Vec<_>>>()?;

        let trace = names.iter().zip(attrs.iter()).filter_map(|(name, attr)| {
            match attr {
                Some(TraceAttr::Skip) => return None,
                Some(TraceAttr::With(w)) => return Some(quote! {#w(#name, tracer)}),
                _ => {}
            }
            Some(quote! {
                ::gcmodule::Trace::trace(#name, tracer)
            })
        });
        let is_type_tracked = fields.iter().zip(attrs.iter()).filter_map(|(field, attr)| {
            match attr {
                Some(TraceAttr::Skip | TraceAttr::TrackingForce(false)) => return None,
                Some(TraceAttr::With(_) | TraceAttr::TrackingForce(true)) => {
                    return Some(quote! {true})
                }
                _ => {}
            }
            let ty = &field.ty;
            Some(quote! {
                <#ty as ::gcmodule::Trace>::is_type_tracked()
            })
        });

        let trace = quote! {
            #(#trace;)*
        };

        Ok((
            trace,
            quote! {
                #(if #is_type_tracked {return true;})*
            },
        ))
    }
    match fields {
        Fields::Named(named) => {
            if matches!(trace_attr, Some(TraceAttr::Skip)) {
                return Ok((
                    quote! {
                        {...} => {}
                    },
                    quote! {},
                ));
            }
            let force_is_type_tracked = trace_attr.as_ref().and_then(|a| a.force_is_type_tracked());

            let names = named
                .named
                .iter()
                .map(|i| i.ident.clone().unwrap())
                .collect::<Vec<_>>();
            let (trace, is_type_tracked) = inner(&names, named.named.iter().collect())?;
            let is_type_tracked = force_is_type_tracked.unwrap_or(is_type_tracked);

            Ok((
                quote! {
                    {#(#names),*} => {#trace}
                },
                is_type_tracked,
            ))
        }
        Fields::Unnamed(unnamed) => {
            if matches!(trace_attr, Some(TraceAttr::Skip)) {
                return Ok((quote! {(...) => {}}, quote! {}));
            }
            let force_is_type_tracked = trace_attr.as_ref().and_then(|a| a.force_is_type_tracked());

            let names = (0..unnamed.unnamed.len())
                .map(|i| format_ident!("field_{}", i))
                .collect::<Vec<_>>();
            let (trace, is_type_tracked) = inner(&names, unnamed.unnamed.iter().collect())?;
            let is_type_tracked = force_is_type_tracked.unwrap_or(is_type_tracked);

            Ok((
                quote! {
                    (#(#names,)*) => {#trace}
                },
                is_type_tracked,
            ))
        }
        Fields::Unit => Ok((
            quote! {
                => {}
            },
            quote! {},
        )),
    }
}

fn derive_trace(input: DeriveInput) -> Result<TokenStream2> {
    let trace_attr = parse_attr::<TraceAttr, _>(&input.attrs, "trace")?;
    if matches!(trace_attr, Some(TraceAttr::With(_))) {
        return Err(Error::new(input.span(), "implement Trace instead"));
    }
    let ident = &input.ident;
    let (impl_generics, type_generics, where_clause) = input.generics.split_for_impl();
    if matches!(trace_attr, Some(TraceAttr::Skip)) {
        return Ok(quote! {
            impl #impl_generics ::gcmodule::Trace for #ident #type_generics #where_clause {
                fn trace(&self, _tracer: &mut ::gcmodule::Tracer) {
                }
                fn is_type_tracked() -> bool {
                    false
                }
            }
        });
    }
    let force_is_type_tracked = trace_attr.and_then(|a| a.force_is_type_tracked());
    let (trace, is_type_tracked) = match &input.data {
        Data::Struct(s) => {
            let (trace, is_type_tracked) = derive_fields(&None, &s.fields)?;

            (
                quote! {
                    Self#trace
                },
                quote! {
                    #is_type_tracked
                    false
                },
            )
        }
        Data::Enum(e) if e.variants.is_empty() => (quote! {_=>unreachable!()}, quote! {false}),
        Data::Enum(e) => {
            let variants = e
                .variants
                .iter()
                .map(|v| {
                    let name = &v.ident;
                    let attr = parse_attr::<TraceAttr, _>(&v.attrs, "trace")?;
                    let impls = derive_fields(&attr, &v.fields)?;
                    Ok((name, impls)) as Result<_>
                })
                .collect::<Result<Vec<_>>>()?;

            let trace = variants.iter().map(|(name, (trace, _))| {
                quote! {
                    Self::#name #trace
                }
            });
            let is_type_tracked = variants.iter().map(|(_, (_, v))| v);

            (
                quote! {
                    #(#trace),*
                },
                quote! {
                    #(#is_type_tracked)*
                    false
                },
            )
        }

        Data::Union(_) => return Err(Error::new(input.span(), "union is not supported")),
    };
    let is_type_tracked = force_is_type_tracked.unwrap_or(is_type_tracked);
    Ok(quote! {
        impl #impl_generics ::gcmodule::Trace for #ident #type_generics #where_clause {
            fn trace(&self, tracer: &mut ::gcmodule::Tracer) {
                match self {
                    #trace
                }
            }
            fn is_type_tracked() -> bool {
                #is_type_tracked
            }
        }
    })
}

#[proc_macro_derive(Trace, attributes(trace))]
pub fn derive_trace_real(input: TokenStream) -> TokenStream {
    let input = parse_macro_input!(input as DeriveInput);
    match derive_trace(input) {
        Ok(v) => v.into(),
        Err(e) => e.to_compile_error().into(),
    }
}
