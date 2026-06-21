use proc_macro::TokenStream;
use quote::quote;
use syn::{parse_macro_input, DeriveInput};

#[proc_macro_derive(Attribute)]
pub fn derive_attribute(input: TokenStream) -> TokenStream {
    let input = parse_macro_input!(input as DeriveInput);

    let name = &input.ident;
    let (impl_generics, ty_generics, where_clause) = &input.generics.split_for_impl();

    let expanded = quote! {
        impl #impl_generics thingdb::attribute::Attribute for #name #ty_generics #where_clause {
            const NAME: &'static str = stringify!(#name);
        }
    };

    TokenStream::from(expanded)
}

#[proc_macro_derive(Edge)]
pub fn derive_edge(input: TokenStream) -> TokenStream {
    let input = parse_macro_input!(input as DeriveInput);

    let name = &input.ident;
    let (impl_generics, ty_generics, where_clause) = &input.generics.split_for_impl();

    let expanded = quote! {
        impl #impl_generics thingdb::edge::Edge for #name #ty_generics #where_clause {
            const NAME: &'static str = stringify!(#name);
        }
    };

    TokenStream::from(expanded)
}
