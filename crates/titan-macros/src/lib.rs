//! Derive macros for Titan game code.

use proc_macro::TokenStream;
use quote::quote;
use syn::{DeriveInput, parse_macro_input};

/// Implements `titan::Component` while preserving generic bounds.
#[proc_macro_derive(Component)]
pub fn derive_component(input: TokenStream) -> TokenStream {
    let input = parse_macro_input!(input as DeriveInput);
    let name = input.ident;
    let generics = input.generics;
    let (impl_generics, type_generics, where_clause) = generics.split_for_impl();

    quote! {
        impl #impl_generics ::titan::Component for #name #type_generics #where_clause {}
    }
    .into()
}

/// Generates opt-in inspection registration for annotated `i32` fields.
/// Named, nongeneric structs are supported. Unannotated fields remain opaque;
/// annotated fields are read-only unless `writable` is specified. Field docs
/// become descriptions, with optional `minimum`, `maximum`, and `unit` metadata.
#[proc_macro_derive(Inspect, attributes(inspect))]
pub fn derive_inspect(input: TokenStream) -> TokenStream {
    let input = parse_macro_input!(input as DeriveInput);
    expand_inspect(input)
        .unwrap_or_else(syn::Error::into_compile_error)
        .into()
}

fn expand_inspect(input: DeriveInput) -> syn::Result<proc_macro2::TokenStream> {
    if !input.generics.params.is_empty() || input.generics.where_clause.is_some() {
        return Err(syn::Error::new_spanned(
            &input.generics,
            "Inspect supports only nongeneric structs",
        ));
    }
    for attr in &input.attrs {
        if attr.path().is_ident("inspect") {
            return Err(syn::Error::new_spanned(attr, "inspect belongs on fields"));
        }
    }
    let syn::Data::Struct(data) = &input.data else {
        return Err(syn::Error::new_spanned(
            &input,
            "Inspect requires a named-field struct",
        ));
    };
    let syn::Fields::Named(fields) = &data.fields else {
        return Err(syn::Error::new_spanned(
            &input,
            "Inspect requires a named-field struct",
        ));
    };
    let mut registrations = Vec::new();
    for field in &fields.named {
        let attrs: Vec<_> = field
            .attrs
            .iter()
            .filter(|a| a.path().is_ident("inspect"))
            .collect();
        if attrs.is_empty() {
            continue;
        }
        if !matches!(&field.ty, syn::Type::Path(ty) if ty.qself.is_none() && ty.path.is_ident("i32"))
        {
            return Err(syn::Error::new_spanned(
                &field.ty,
                "Inspect currently supports only i32 fields",
            ));
        }
        let mut writable = false;
        let mut minimum: Option<syn::Expr> = None;
        let mut maximum: Option<syn::Expr> = None;
        let mut unit: Option<syn::LitStr> = None;
        let mut seen = std::collections::BTreeSet::new();
        for attr in attrs {
            attr.parse_nested_meta(|meta| {
                let Some(key) = meta.path.get_ident() else {
                    return Err(meta.error("expected writable, minimum, maximum, or unit"));
                };
                if !seen.insert(key.to_string()) {
                    return Err(meta.error("duplicate inspection option"));
                }
                match key.to_string().as_str() {
                    "writable" => writable = true,
                    "minimum" => minimum = Some(meta.value()?.parse()?),
                    "maximum" => maximum = Some(meta.value()?.parse()?),
                    "unit" => unit = Some(meta.value()?.parse()?),
                    _ => return Err(meta.error("expected writable, minimum, maximum, or unit")),
                }
                Ok(())
            })?;
        }
        let description = field
            .attrs
            .iter()
            .filter_map(|attr| {
                if !attr.path().is_ident("doc") {
                    return None;
                }
                let syn::Meta::NameValue(value) = &attr.meta else {
                    return None;
                };
                let syn::Expr::Lit(lit) = &value.value else {
                    return None;
                };
                let syn::Lit::Str(text) = &lit.lit else {
                    return None;
                };
                Some(text.value().trim().to_owned())
            })
            .collect::<Vec<_>>()
            .join("\n");
        let minimum = minimum.map_or_else(
            || quote!(::core::option::Option::None),
            |v| quote!(::core::option::Option::Some(::core::primitive::f64::from(#v))),
        );
        let maximum = maximum.map_or_else(
            || quote!(::core::option::Option::None),
            |v| quote!(::core::option::Option::Some(::core::primitive::f64::from(#v))),
        );
        let unit = unit.map_or_else(
            || quote!(::core::option::Option::None),
            |v| quote!(::core::option::Option::Some(#v.into())),
        );
        let field = field.ident.as_ref().unwrap();
        let field_name = field.to_string().trim_start_matches("r#").to_owned();
        let metadata = quote! {
            ::titan::inspection::FieldMetadata {
                type_name: "i32".into(), description: #description.into(),
                writable: #writable, minimum: #minimum, maximum: #maximum, unit: #unit,
            }
        };
        registrations.push(if writable {
            quote! {
                inspector.register_field::<Self, ::core::primitive::i32>(#field_name, #metadata,
                    |component| component.#field, |_, _| ::core::result::Result::Ok(()),
                    |component, value| component.#field = value)?;
            }
        } else {
            quote! {
                inspector.register_read_only_field::<Self, ::core::primitive::i32>(#field_name, #metadata,
                    |component| component.#field)?;
            }
        });
    }
    let name = input.ident;
    Ok(quote! {
        impl ::titan::inspection::Inspect for #name {
            fn register_inspection(inspector: &mut ::titan::inspection::Inspector)
                -> ::core::result::Result<(), ::titan::inspection::ProtocolError> {
                #(#registrations)*
                ::core::result::Result::Ok(())
            }
        }
    })
}

#[cfg(test)]
mod tests {
    use super::expand_inspect;

    #[test]
    fn unsupported_shapes_and_attributes_have_targeted_errors() {
        for (source, expected) in [
            ("enum Bad { A }", "named-field struct"),
            ("struct Bad(i32);", "named-field struct"),
            ("struct Bad<T> { value: T }", "nongeneric"),
            (
                "#[inspect(writable)] struct Bad { x: i32 }",
                "belongs on fields",
            ),
            (
                "struct Bad { #[inspect(unit = \"m\")] x: String }",
                "only i32",
            ),
            (
                "struct Bad { #[inspect(typo)] x: i32 }",
                "expected writable",
            ),
            (
                "struct Bad { #[inspect(writable, writable)] x: i32 }",
                "duplicate",
            ),
            (
                "struct Bad { #[inspect(unit = 3)] x: i32 }",
                "expected string literal",
            ),
            (
                "struct Bad { #[inspect(writable = false)] x: i32 }",
                "expected `,`",
            ),
        ] {
            let result = expand_inspect(syn::parse_str(source).unwrap());
            assert!(result.is_err(), "accepted {source}");
            assert!(
                result.unwrap_err().to_string().contains(expected),
                "{source}: expected {expected}"
            );
        }
    }
}
