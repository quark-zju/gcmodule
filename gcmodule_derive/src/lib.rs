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
//!     #[skip_trace] // ignore this field for Trace.
//!     c: MyType,
//! }
//!
//! struct MyType;
//! ```
extern crate proc_macro;

use quote::quote;
use syn::Attribute;
use synstructure::{decl_derive, AddBounds, BindStyle, Structure};

decl_derive!([Trace, attributes(skip_trace, ignore_tracking, force_tracking)] => derive_trace);

fn has_attr(attrs: &[Attribute], attr: &str) -> bool {
    attrs.iter().any(|a| a.path.is_ident(attr))
}

fn derive_trace(mut s: Structure<'_>) -> proc_macro2::TokenStream {
    if has_attr(&s.ast().attrs, "skip_trace") {
        s.filter(|_| false);
        return s.bound_impl(
            quote! {::gcmodule::Trace},
            quote! {
                fn trace(&self, _tracer: &mut ::gcmodule::Tracer) {}
                fn is_type_tracked() -> bool {
                    false
                }
            },
        );
    }
    let force_tracking = has_attr(&s.ast().attrs, "force_tracking");

    s.filter_variants(|f| !has_attr(f.ast().attrs, "skip_trace"));
    s.filter(|f| !has_attr(&f.ast().attrs, "skip_trace"));
    s.add_bounds(AddBounds::Fields);
    s.bind_with(|_| BindStyle::Ref);

    let trace_body = s.each(|bi| quote!(::gcmodule::Trace::trace(#bi, tracer)));

    let is_type_tracked_body = if force_tracking {
        quote! {
            true
        }
    } else {
        s.filter(|f| !has_attr(&f.ast().attrs, "ignore_tracking"));
        let ty = s
            .variants()
            .iter()
            .flat_map(|v| v.bindings().iter())
            .map(|bi| &bi.ast().ty);
        quote! {
            #(
            if <#ty>::is_type_tracked() {
                return true;
            }
            )*
            false
        }
    };

    s.bound_impl(
        quote! {::gcmodule::Trace},
        quote! {
            fn trace(&self, tracer: &mut ::gcmodule::Tracer) {
                match *self { #trace_body }
            }
            fn is_type_tracked() -> bool {
                #is_type_tracked_body
            }
        },
    )
}
