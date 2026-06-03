use proc_macro::TokenStream;
use quote::quote;
use syn::{parse_macro_input, DeriveInput};

#[proc_macro_derive(thingdb_attribute)]
pub fn derive_attribute(input: TokenStream) -> TokenStream {
    let input = parse_macro_input!(input as DeriveInput);

    let name = &input.ident;
    let (impl_generics, ty_generics, where_clause) = &input.generics.split_for_impl();

    let expanded = quote! {
        impl #impl_generics thingdb::Attribute for #name #ty_generics #where_clause {
            const NAME: &'static str = stringify!(#name);
        }
    };

    TokenStream::from(expanded)
}