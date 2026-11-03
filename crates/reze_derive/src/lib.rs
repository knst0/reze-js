//! `#[derive(reze::Data)]`.

use proc_macro::TokenStream;
use quote::quote;
use syn::{Data, DeriveInput, Fields, parse_macro_input};

/// Implements `reze::Data` for a struct with named fields, or `Data` and `Variant` for an enum of
/// unit variants.
#[proc_macro_derive(Data)]
pub fn derive(input: TokenStream) -> TokenStream {
    let input = parse_macro_input!(input as DeriveInput);
    let name = &input.ident;
    let (impl_generics, type_generics, where_clause) = input.generics.split_for_impl();
    let expanded = match &input.data {
        Data::Struct(data) => {
            let Fields::Named(fields) = &data.fields else {
                return error(&input, "reze::Data needs a struct with named fields");
            };
            let writes = fields.named.iter().enumerate().map(|(index, field)| {
                let ident = field.ident.as_ref().unwrap();
                let key = format!("{}\"{}\":", if index == 0 { "" } else { "," }, ident);
                quote! {
                    out.push_str(#key);
                    ::reze::Data::write(&self.#ident, out);
                }
            });
            quote! {
                impl #impl_generics ::reze::Data for #name #type_generics #where_clause {
                    fn write(&self, out: &mut ::std::string::String) {
                        out.push('{');
                        #(#writes)*
                        out.push('}');
                    }
                }
            }
        }
        Data::Enum(data) => {
            if data.variants.iter().any(|variant| !matches!(variant.fields, Fields::Unit)) {
                return error(&input, "reze::Data needs an enum of unit variants");
            }
            let arms = data.variants.iter().map(|variant| {
                let ident = &variant.ident;
                let text = ident.to_string();
                quote! { Self::#ident => #text }
            });
            quote! {
                impl #impl_generics ::reze::Variant for #name #type_generics #where_clause {
                    fn name(&self) -> &'static str {
                        match self { #(#arms,)* }
                    }
                }
                impl #impl_generics ::reze::Data for #name #type_generics #where_clause {
                    fn write(&self, out: &mut ::std::string::String) {
                        ::reze::js::push_json_string(out, ::reze::Variant::name(self));
                    }
                }
            }
        }
        Data::Union(_) => return error(&input, "reze::Data does not support unions"),
    };
    expanded.into()
}

fn error(input: &DeriveInput, message: &str) -> TokenStream {
    syn::Error::new_spanned(&input.ident, message).to_compile_error().into()
}
