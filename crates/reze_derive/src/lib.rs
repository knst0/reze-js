//! `#[derive(reze::Data)]`.

use proc_macro::TokenStream;
use quote::quote;
use syn::{Data, DeriveInput, Fields, parse_macro_input};

/// Implements `reze::Data` and `reze::Input` for a struct with named fields, and also `Variant`
/// for an enum of unit variants.
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
            let wires = fields.named.iter().enumerate().map(|(index, field)| {
                let ident = field.ident.as_ref().unwrap();
                let comma = (index > 0).then(|| quote! { out.push(','); });
                quote! {
                    #comma
                    ::reze::Data::wire(&self.#ident, out);
                }
            });
            let idents: Vec<_> = fields.named.iter().map(|f| f.ident.as_ref().unwrap()).collect();
            let types: Vec<_> = fields.named.iter().map(|f| &f.ty).collect();
            let keys: Vec<String> = idents.iter().map(|ident| ident.to_string()).collect();
            quote! {
                impl #impl_generics ::reze::Input for #name #type_generics #where_clause {
                    const FORM: bool = true #(&& <#types as ::reze::Input>::FIELD)*;

                    fn describe() -> ::std::string::String {
                        "an object".into()
                    }

                    fn json(
                        value: ::std::option::Option<&::reze::Json>,
                        path: &[::reze::Key],
                        issues: &mut ::std::vec::Vec<::reze::Issue>,
                    ) -> ::std::option::Option<Self> {
                        let ::std::option::Option::Some(value @ ::reze::Json::Object(_)) = value else {
                            issues.push(::reze::Issue { message: "Expected an object".into(), path: path.to_vec() });
                            return ::std::option::Option::None;
                        };
                        #(
                            let #idents = <#types as ::reze::Input>::json(
                                ::reze::input::field(value, #keys),
                                &::reze::input::child(path, ::reze::Key::Field(#keys.into())),
                                issues,
                            );
                        )*
                        ::std::option::Option::Some(Self { #(#idents: #idents?,)* })
                    }

                    fn form(
                        form: &::reze::input::Form,
                        issues: &mut ::std::vec::Vec<::reze::Issue>,
                    ) -> ::std::option::Option<Self> {
                        #(
                            let #idents = <#types as ::reze::Input>::field(
                                &::reze::input::values(form, #keys),
                                #keys,
                                issues,
                            );
                        )*
                        ::std::option::Option::Some(Self { #(#idents: #idents?,)* })
                    }
                }

                impl #impl_generics ::reze::Data for #name #type_generics #where_clause {
                    fn write(&self, out: &mut ::std::string::String) {
                        out.push('{');
                        #(#writes)*
                        out.push('}');
                    }

                    fn wire(&self, out: &mut ::std::string::String) {
                        out.push('[');
                        #(#wires)*
                        out.push(']');
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
            let indexes = data.variants.iter().enumerate().map(|(index, variant)| {
                let ident = &variant.ident;
                let text = index.to_string();
                quote! { Self::#ident => #text }
            });
            let idents: Vec<_> = data.variants.iter().map(|variant| &variant.ident).collect();
            let names: Vec<String> = idents.iter().map(|ident| ident.to_string()).collect();
            let describe =
                names.iter().map(|name| format!("{name:?}")).collect::<Vec<_>>().join(" or ");
            quote! {
                impl #impl_generics ::reze::Input for #name #type_generics #where_clause {
                    const SCALAR: bool = true;

                    fn describe() -> ::std::string::String {
                        #describe.into()
                    }

                    fn json(
                        value: ::std::option::Option<&::reze::Json>,
                        path: &[::reze::Key],
                        issues: &mut ::std::vec::Vec<::reze::Issue>,
                    ) -> ::std::option::Option<Self> {
                        match value {
                            ::std::option::Option::Some(::reze::Json::String(text)) => {
                                <Self as ::reze::Input>::scalar(text, path, issues)
                            }
                            _ => {
                                issues.push(::reze::Issue { message: format!("Expected {}", #describe), path: path.to_vec() });
                                ::std::option::Option::None
                            }
                        }
                    }

                    fn scalar(
                        value: &str,
                        path: &[::reze::Key],
                        issues: &mut ::std::vec::Vec<::reze::Issue>,
                    ) -> ::std::option::Option<Self> {
                        match value {
                            #(#names => ::std::option::Option::Some(Self::#idents),)*
                            _ => {
                                issues.push(::reze::Issue { message: format!("Expected {}", #describe), path: path.to_vec() });
                                ::std::option::Option::None
                            }
                        }
                    }
                }

                impl #impl_generics ::reze::Variant for #name #type_generics #where_clause {
                    fn name(&self) -> &'static str {
                        match self { #(#arms,)* }
                    }
                }
                impl #impl_generics ::reze::Data for #name #type_generics #where_clause {
                    fn write(&self, out: &mut ::std::string::String) {
                        ::reze::js::push_json_string(out, ::reze::Variant::name(self));
                    }

                    fn wire(&self, out: &mut ::std::string::String) {
                        out.push_str(match self { #(#indexes,)* });
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
