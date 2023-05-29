use proc_macro2::{Span, TokenStream};
use quote::quote;
use syn::{
    parse_quote, punctuated::Punctuated, spanned::Spanned, Data, DeriveInput, Error, Fields,
    Generics, Ident, Index, Result,
};

use crate::util::{parse_top_attrs, with_inner, with_ty};

pub fn derive(mut input: DeriveInput) -> Result<TokenStream> {
    let _ = input.generics.make_where_clause();
    let (_, ty_generics, where_clause) = input.generics.split_for_impl();
    let where_clause = where_clause.unwrap();

    let mut impl_input_params = Punctuated::default();
    impl_input_params.push(parse_quote! { __D: Fallible + ?Sized });

    for param in input.generics.params.iter() {
        impl_input_params.push(param.clone());
    }

    let impl_input_generics = Generics {
        lt_token: Some(Default::default()),
        params: impl_input_params,
        gt_token: Some(Default::default()),
        where_clause: input.generics.where_clause.clone(),
    };

    let (impl_generics, _, _) = impl_input_generics.split_for_impl();

    let from_tys = parse_top_attrs(&input.attrs)?;

    if from_tys.is_empty() {
        let msg = "requires top level attribute `#[archive_with(from(TypeName))]`";

        return Err(Error::new(Span::call_site(), msg));
    }

    let name = &input.ident;

    let deserialize_impl: TokenStream = match input.data {
        Data::Struct(ref data) => match data.fields {
            Fields::Named(ref fields) => {
                let mut deserialize_where = where_clause.clone();

                for field in fields.named.iter() {
                    let (ty, _) = with_ty(field)?;

                    deserialize_where
                        .predicates
                        .push(parse_quote! { #ty: Archive });

                    deserialize_where
                        .predicates
                        .push(parse_quote! { Archived<#ty>: Deserialize<#ty, __D> });
                }

                let deserialize_fields: Vec<_> = fields
                    .named
                    .iter()
                    .map(|field| {
                        let name = &field.ident;
                        let (ty, attrs) = with_ty(field).unwrap();

                        let value = with_inner(
                            field,
                            &attrs,
                            parse_quote! {
                                Deserialize::<#ty, __D>::deserialize(
                                    &field.#name,
                                    deserializer,
                                )?
                            },
                        )
                        .unwrap();

                        quote! { #name: #value }
                    })
                    .collect();

                from_tys
                    .iter()
                    .map(|from_ty| {
                        quote! {
                            impl #impl_generics
                            DeserializeWith<<Self as Archive>::Archived, #from_ty, __D>
                            for #name #ty_generics #deserialize_where {
                                #[inline]
                                fn deserialize_with(
                                    field: &<Self as Archive>::Archived,
                                    deserializer: &mut __D
                                ) -> Result<#from_ty, <__D as Fallible>::Error> {
                                    Ok(#from_ty {
                                        #( #deserialize_fields, )*
                                    })
                                }
                            }
                        }
                    })
                    .collect()
            }
            Fields::Unnamed(ref fields) => {
                let mut deserialize_where = where_clause.clone();

                for field in fields.unnamed.iter() {
                    let (ty, _) = with_ty(field)?;

                    deserialize_where
                        .predicates
                        .push(parse_quote! { #ty: Archive });

                    deserialize_where
                        .predicates
                        .push(parse_quote! { Archived<#ty>: Deserialize<#ty, __D> });
                }

                from_tys
                    .iter()
                    .map(|from_ty| {
                        let deserialize_fields =
                            fields.unnamed.iter().enumerate().map(|(i, field)| {
                                let index = Index::from(i);
                                let (ty, attrs) = with_ty(field).unwrap();

                                let value = with_inner(
                                    field,
                                    &attrs,
                                    parse_quote! {
                                        Deserialize::<#ty, __D>::deserialize(
                                            &field.#index,
                                            deserializer,
                                        )?
                                    },
                                )
                                .unwrap();

                                quote! { #value }
                            });

                        quote! {
                            impl #impl_generics
                            DeserializeWith<<Self as Archive>::Archived, #from_ty, __D>
                            for #name #ty_generics #deserialize_where {
                                #[inline]
                                fn deserialize_with(
                                    field: &<Self as Archive>::Archived,
                                    deserializer: &mut __D
                                ) -> Result<#from_ty, <__D as Fallible>::Error> {
                                    Ok(#from_ty(
                                        #( #deserialize_fields, )*
                                    ))
                                }
                            }
                        }
                    })
                    .collect()
            }
            Fields::Unit => from_tys
                .iter()
                .map(|from_ty| {
                    quote! {
                        impl #impl_generics
                        DeserializeWith<<Self as Archive>::Archived, #from_ty, __D>
                        for #name #ty_generics #where_clause {
                            #[inline]
                            fn deserialize_with(
                                _: &<Self as Archive>::Archived,
                                _: &mut __D
                            ) -> Result<#from_ty, <__D as Fallible>::Error> {
                                Ok(#from_ty)
                            }
                        }
                    }
                })
                .collect(),
        },
        Data::Enum(ref data) => {
            let mut deserialize_where = where_clause.clone();

            for variant in data.variants.iter() {
                match variant.fields {
                    Fields::Named(ref fields) => {
                        for field in fields.named.iter() {
                            let (ty, _) = with_ty(field)?;

                            deserialize_where
                                .predicates
                                .push(parse_quote! { #ty: Archive });

                            deserialize_where
                                .predicates
                                .push(parse_quote! { Archived<#ty>: Deserialize<#ty, __D> });
                        }
                    }
                    Fields::Unnamed(ref fields) => {
                        for field in fields.unnamed.iter() {
                            let (ty, _) = with_ty(field)?;

                            deserialize_where
                                .predicates
                                .push(parse_quote! { #ty: Archive });

                            deserialize_where
                                .predicates
                                .push(parse_quote! { Archived<#ty>: Deserialize<#ty, __D> });
                        }
                    }
                    Fields::Unit => {}
                }
            }

            from_tys
                .iter()
                .map(|from_ty| {
                    let deserialize_variants = data.variants.iter().map(|v| {
                        let variant = &v.ident;

                        match v.fields {
                            Fields::Named(ref fields) => {
                                let bindings = fields.named.iter().map(|field| {
                                    let name = &field.ident;

                                    quote!(#name)
                                });

                                let fields = fields.named.iter().map(|field| {
                                    let name = &field.ident;
                                    let (ty, attrs) = with_ty(field).unwrap();
                                    let value = with_inner(
                                        field,
                                        &attrs,
                                        parse_quote! {
                                            Deserialize::<#ty, __D>::deserialize(
                                                #name,
                                                deserializer,
                                            )?
                                        },
                                    )
                                    .unwrap();

                                    quote! { #name: #value }
                                });

                                quote! {
                                    __SelfArchived::#variant { #( #bindings, )* } => {
                                        #from_ty::#variant { #( #fields, )* }
                                    }
                                }
                            }
                            Fields::Unnamed(ref fields) => {
                                let bindings = fields.unnamed.iter().enumerate().map(|(i, f)| {
                                    let name = Ident::new(&format!("_{}", i), f.span());

                                    quote!(#name)
                                });

                                let fields = fields.unnamed.iter().enumerate().map(|(i, field)| {
                                    let binding = Ident::new(&format!("_{}", i), field.span());
                                    let (ty, attrs) = with_ty(field).unwrap();

                                    let value = with_inner(
                                        field,
                                        &attrs,
                                        parse_quote! {
                                            Deserialize::<#ty, __D>::deserialize(
                                                #binding,
                                                deserializer,
                                            )?
                                        },
                                    )
                                    .unwrap();

                                    quote! { #value }
                                });

                                quote! {
                                    __SelfArchived::#variant( #( #bindings, )* ) =>
                                        #from_ty::#variant(#( #fields, )*)
                                }
                            }
                            Fields::Unit => {
                                quote! { __SelfArchived::#variant => #from_ty::#variant }
                            }
                        }
                    });

                    quote! {
                        impl #impl_generics
                        DeserializeWith<<Self as Archive>::Archived, #from_ty, __D>
                        for #name #ty_generics #deserialize_where {
                            #[inline]
                            fn deserialize_with(
                                field: &<Self as Archive>::Archived,
                                deserializer: &mut __D
                            ) -> ::core::result::Result<#from_ty, __D::Error> {
                                type __SelfArchived #ty_generics = <#name #ty_generics as Archive>::Archived;

                                Ok(match field {
                                    #( #deserialize_variants, )*
                                })
                            }
                        }
                    }
                })
                .collect()
        }
        Data::Union(_) => {
            let msg = "DeserializeWith cannot be derived for unions";

            return Err(Error::new_spanned(input, msg));
        }
    };

    let tokens = quote! {
        #[automatically_derived]
        const _: () = {
            use ::rkyv::{Archive, Archived, Deserialize, Fallible};

            #deserialize_impl
        };
    };

    Ok(tokens)
}
