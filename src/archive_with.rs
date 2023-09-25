use proc_macro2::{Ident, Span, TokenStream};
use quote::{format_ident, quote};
use syn::{
    parse_quote, punctuated::Punctuated, spanned::Spanned, Data, DeriveInput, Error, Fields,
    Generics, Index, Result,
};

use crate::util::{parse_top_attrs, strip_raw, with_cast, with_ty, ParsedAttributes};

pub fn derive(mut input: DeriveInput) -> Result<TokenStream> {
    let _ = input.generics.make_where_clause();
    let (impl_generics, ty_generics, where_clause) = input.generics.split_for_impl();
    let where_clause = where_clause.unwrap();

    let mut impl_input_params = Punctuated::default();
    impl_input_params.push(parse_quote! { __S: Fallible + ?Sized });

    for param in input.generics.params.iter() {
        impl_input_params.push(param.clone());
    }

    let serialize_impl_input_generics = Generics {
        lt_token: Some(Default::default()),
        params: impl_input_params,
        gt_token: Some(Default::default()),
        where_clause: input.generics.where_clause.clone(),
    };

    let (serialize_impl_generics, _, _) = serialize_impl_input_generics.split_for_impl();

    let from_tys = parse_top_attrs(&input.attrs)?;

    if from_tys.is_empty() {
        let msg = "requires top level attribute `#[archive_with(from(TypeName))]`";

        return Err(Error::new(Span::call_site(), msg));
    }

    let name = &input.ident;
    let generics = &input.generics;

    let (archive_impl, serialize_impl): (TokenStream, TokenStream) = match input.data {
        Data::Struct(ref data) => {
            match data.fields {
                Fields::Named(ref fields) => {
                    let mut archive_where = where_clause.clone();
                    let mut serialize_where = where_clause.clone();

                    for field in fields.named.iter() {
                        let (ty, _) = with_ty(field)?;

                        archive_where.predicates.push(parse_quote! { #ty: Archive });

                        serialize_where
                            .predicates
                            .push(parse_quote! { #ty: Serialize<__S> });
                    }

                    let archive_impls = from_tys
                        .iter()
                        .map(|from_ty| {
                            let resolve_fields = fields.named.iter().map(|field| {
                                let name = &field.ident;
                                let attrs = ParsedAttributes::new(&field.attrs).unwrap();
                                let ty = attrs.from.as_ref().unwrap_or(&field.ty);

                                let expr = attrs.getter.as_ref().map_or_else(
                                    || parse_quote! { (field.#name) },
                                    |getter| getter.make_expr(from_ty),
                                );
                                let field = with_cast(field, parse_quote!(__field)).unwrap();

                                quote! {
                                    let (fp, fo) = out_field!(out.#name);
                                    let __field: &#ty = &#expr;
                                    ::rkyv::Archive::resolve(#field, pos + fp, resolver.#name, fo);
                                }
                            });

                            quote! {
                                impl #impl_generics ArchiveWith<#from_ty>
                                for #name #ty_generics #archive_where {
                                    type Archived = <Self as Archive>::Archived;
                                    type Resolver = <Self as Archive>::Resolver;

                                    #[allow(clippy::unit_arg)]
                                    #[inline]
                                    unsafe fn resolve_with(
                                        field: &#from_ty,
                                        pos: usize,
                                        resolver: Self::Resolver,
                                        out: *mut Self::Archived,
                                    ) {
                                        #( #resolve_fields )*
                                    }
                                }
                            }
                        })
                        .collect();

                    let serialize_impls = from_tys
                        .iter()
                        .map(|from_ty| {
                            let field_vars = fields.named.iter().map(|field| {
                                let name = &field.ident;
                                let ident = format_ident!("__{}", name.as_ref().unwrap());
                                let attrs = ParsedAttributes::new(&field.attrs).unwrap();
                                let ty = attrs.from.as_ref().unwrap_or(&field.ty);

                                let expr = attrs.getter.as_ref().map_or_else(
                                    || parse_quote! { (field.#name) },
                                    |getter| getter.make_expr(from_ty),
                                );

                                quote! { let #ident: &#ty = &#expr; }
                            });

                            let resolver_values = fields.named.iter().map(|field| {
                                let name = &field.ident;
                                let ident = format_ident!("__{}", name.as_ref().unwrap());
                                let expr = parse_quote!( #ident );
                                let field = with_cast(field, expr).unwrap();

                                quote! { #name: Serialize::<__S>::serialize(#field, serializer)? }
                            });

                            quote! {
                                impl #serialize_impl_generics SerializeWith<#from_ty, __S>
                                for #name #ty_generics #serialize_where {
                                    #[inline]
                                    fn serialize_with(
                                        field: &#from_ty,
                                        serializer: &mut __S,
                                    ) -> Result<Self::Resolver, <__S as Fallible>::Error> {
                                        #( #field_vars )*
                                        Ok(Self::Resolver {
                                            #( #resolver_values, )*
                                        })
                                    }
                                }
                            }
                        })
                        .collect();

                    (archive_impls, serialize_impls)
                }
                Fields::Unnamed(ref fields) => {
                    let mut archive_where = where_clause.clone();
                    let mut serialize_where = where_clause.clone();

                    for field in fields.unnamed.iter() {
                        let (ty, _) = with_ty(field)?;

                        archive_where
                            .predicates
                            .push(parse_quote! { #ty: ::rkyv::Archive });

                        serialize_where
                            .predicates
                            .push(parse_quote! { #ty: Serialize<__S> });
                    }

                    let archive_impls = from_tys
                        .iter()
                        .map(|from_ty| {
                            let resolve_fields =
                                fields.unnamed.iter().enumerate().map(|(i, field)| {
                                    let index = Index::from(i);
                                    let attrs = ParsedAttributes::new(&field.attrs).unwrap();
                                    let ty = attrs.from.as_ref().unwrap_or(&field.ty);

                                    let expr = attrs.getter.as_ref().map_or_else(
                                        || parse_quote! { (field.#index) },
                                        |getter| getter.make_expr(from_ty),
                                    );
                                    let field = with_cast(field, parse_quote!(__field)).unwrap();

                                    quote! {
                                        let (fp, fo) = out_field!(out.#index);
                                        let __field: &#ty = &#expr;
                                        ::rkyv::Archive::resolve(
                                            #field,
                                            pos + fp,
                                            resolver.#index,
                                            fo
                                        );
                                    }
                                });

                            quote! {
                                impl #impl_generics ArchiveWith<#from_ty>
                                for #name #ty_generics #archive_where {
                                    type Archived = <Self as Archive>::Archived;
                                    type Resolver = <Self as Archive>::Resolver;

                                    #[allow(clippy::unit_arg)]
                                    #[inline]
                                    unsafe fn resolve_with(
                                        field: &#from_ty,
                                        pos: usize,
                                        resolver: Self::Resolver,
                                        out: *mut Self::Archived,
                                    ) {
                                        #( #resolve_fields )*
                                    }
                                }
                            }
                        })
                        .collect();

                    let serialize_impls = from_tys
                        .iter()
                        .map(|from_ty| {
                            let field_vars = fields.unnamed.iter().enumerate().map(|(i, field)| {
                                let index = Index::from(i);
                                let ident = format_ident!("__{i}", span = index.span());
                                let attrs = ParsedAttributes::new(&field.attrs).unwrap();
                                let ty = attrs.from.as_ref().unwrap_or(&field.ty);

                                let expr = attrs.getter.as_ref().map_or_else(
                                    || parse_quote! { (field.#index) },
                                    |getter| getter.make_expr(from_ty),
                                );

                                quote! { let #ident: &#ty = &#expr; }
                            });

                            let resolver_values =
                                fields.unnamed.iter().enumerate().map(|(i, field)| {
                                    let index = Index::from(i);
                                    let ident = format_ident!("__{i}", span = index.span());
                                    let expr = parse_quote!( #ident );
                                    let field = with_cast(field, expr).unwrap();

                                    quote! { Serialize::<__S>::serialize(#field, serializer)? }
                                });

                            // FIXME: rust currently requires the actual name instead of
                            //        something like `<Self as Archive>::Resolver(...)`
                            let resolver_name = Ident::new(&format!("{name}Resolver"), name.span());

                            quote! {
                                impl #serialize_impl_generics SerializeWith<#from_ty, __S>
                                for #name #ty_generics #serialize_where {
                                    #[inline]
                                    fn serialize_with(
                                        field: &#from_ty,
                                        serializer: &mut __S,
                                    ) -> Result<Self::Resolver, <__S as Fallible>::Error> {
                                        #( #field_vars )*
                                        Ok(#resolver_name(
                                            #( #resolver_values, )*
                                        ))
                                    }
                                }
                            }
                        })
                        .collect();

                    (archive_impls, serialize_impls)
                }
                Fields::Unit => {
                    let archive_impls = from_tys
                        .iter()
                        .map(|from_ty| {
                            quote! {
                                impl #impl_generics ::rkyv::with::ArchiveWith<#from_ty>
                                for #name #ty_generics #where_clause {
                                    type Archived = <Self as ::rkyv::Archive>::Archived;
                                    type Resolver = <Self as ::rkyv::Archive>::Resolver;

                                    #[allow(clippy::unit_arg)]
                                    #[inline]
                                    unsafe fn resolve_with(
                                        field: &#from_ty,
                                        pos: usize,
                                        resolver: Self::Resolver,
                                        out: *mut Self::Archived,
                                    ) {
                                    }
                                }
                            }
                        })
                        .collect();

                    let serialize_impls = from_tys
                        .iter()
                        .map(|from_ty| {
                            let resolver_name = Ident::new(&format!("{name}Resolver"), name.span());

                            quote! {
                                impl #serialize_impl_generics
                                ::rkyv::with::SerializeWith<#from_ty, __S>
                                for #name #ty_generics #where_clause {
                                    #[inline]
                                    fn serialize_with(
                                        field: &#from_ty,
                                        serializer: &mut __S,
                                    ) -> Result<Self::Resolver, <__S as Fallible>::Error> {
                                        Ok(#resolver_name)
                                    }
                                }
                            }
                        })
                        .collect();

                    (archive_impls, serialize_impls)
                }
            }
        }
        Data::Enum(ref data) => {
            let mut archive_where = where_clause.clone();
            let mut serialize_where = where_clause.clone();

            for variant in data.variants.iter() {
                match variant.fields {
                    Fields::Named(ref fields) => {
                        for field in fields.named.iter() {
                            let (ty, _) = with_ty(field)?;

                            archive_where
                                .predicates
                                .push(parse_quote!( #ty: ::rkyv::Archive ));

                            serialize_where
                                .predicates
                                .push(parse_quote!( #ty: Serialize<__S> ));
                        }
                    }
                    Fields::Unnamed(ref fields) => {
                        for field in fields.unnamed.iter() {
                            let (ty, _) = with_ty(field)?;

                            archive_where
                                .predicates
                                .push(parse_quote!( #ty: ::rkyv::Archive ));

                            serialize_where
                                .predicates
                                .push(parse_quote!( #ty: Serialize<__S> ));
                        }
                    }
                    Fields::Unit => {}
                }
            }

            let archive_impls = from_tys
                .iter()
                .map(|from_ty| {
                    let archived_variant_tags = data.variants.iter().map(|v| {
                        let variant = &v.ident;

                        quote! { #variant }
                    });

                    let archived_variant_structs = data.variants.iter().map(|v| {
                        let variant = &v.ident;
                        let archived_variant_name =
                            Ident::new(&format!("ArchivedVariant{}", strip_raw(variant)), v.span());

                        match v.fields {
                            Fields::Named(ref fields) => {
                                let fields = fields.named.iter().map(|field| {
                                    let name = &field.ident;
                                    let (ty, _) = with_ty(field).unwrap();

                                    quote! { #name: Archived<#ty> }
                                });

                                quote! {
                                    #[repr(C)]
                                    struct #archived_variant_name #generics #archive_where {
                                        __tag: ArchivedTag,
                                        #( #fields, )*
                                        __phantom: PhantomData<#name #ty_generics>
                                    }
                                }
                            }
                            Fields::Unnamed(ref fields) => {
                                let fields = fields.unnamed.iter().map(|field| {
                                    let (ty, _) = with_ty(field).unwrap();

                                    quote! { Archived<#ty> }
                                });

                                quote! {
                                    #[repr(C)]
                                    struct #archived_variant_name #generics (
                                        ArchivedTag,
                                        #( #fields, )*
                                        PhantomData<#name #ty_generics>
                                    ) #archive_where;
                                }
                            }
                            Fields::Unit => quote! {},
                        }
                    });

                    let resolve_arms = data.variants.iter().map(|v| {
                        let variant = &v.ident;
                        let archived_variant_name =
                            Ident::new(&format!("ArchivedVariant{}", strip_raw(variant)), v.span());

                        match v.fields {
                            Fields::Named(ref fields) => {
                                let self_bindings = fields.named.iter().map(|f| {
                                    let name = &f.ident;
                                    let binding = Ident::new(
                                        &format!("self_{}", strip_raw(name.as_ref().unwrap())),
                                        name.span(),
                                    );

                                    quote! { #name: #binding }
                                });

                                let resolver_bindings = fields.named.iter().map(|f| {
                                    let name = &f.ident;
                                    let binding = Ident::new(
                                        &format!("resolver_{}", strip_raw(name.as_ref().unwrap())),
                                        name.span(),
                                    );

                                    quote! { #name: #binding }
                                });

                                let resolves = fields.named.iter().map(|f| {
                                    let name = &f.ident;
                                    let self_binding = Ident::new(
                                        &format!("self_{}", strip_raw(name.as_ref().unwrap())),
                                        name.span(),
                                    );
                                    let resolver_binding = Ident::new(
                                        &format!("resolver_{}", strip_raw(name.as_ref().unwrap())),
                                        name.span(),
                                    );
                                    let value =
                                        with_cast(f, parse_quote! { #self_binding }).unwrap();

                                    quote! {
                                        let (fp, fo) = out_field!(out.#name);
                                        ::rkyv::Archive::resolve(
                                            #value,
                                            pos + fp,
                                            #resolver_binding,
                                            fo
                                        );
                                    }
                                });
                                quote! {
                                    __SelfResolver::#variant {
                                        #( #resolver_bindings, )*
                                    } => {
                                        match field {
                                            #from_ty::#variant { #(#self_bindings,)* } => {
                                                let out = out
                                                    .cast::<#archived_variant_name #ty_generics>();
                                                ::core::ptr::addr_of_mut!((*out).__tag)
                                                    .write(ArchivedTag::#variant);
                                                #( #resolves )*
                                            },
                                            #[allow(unreachable_patterns)]
                                            _ => ::core::hint::unreachable_unchecked(),
                                        }
                                    }
                                }
                            }
                            Fields::Unnamed(ref fields) => {
                                let self_bindings =
                                    fields.unnamed.iter().enumerate().map(|(i, f)| {
                                        let name = Ident::new(&format!("self_{}", i), f.span());

                                        quote! { #name }
                                    });

                                let resolver_bindings =
                                    fields.unnamed.iter().enumerate().map(|(i, f)| {
                                        let name = Ident::new(&format!("resolver_{}", i), f.span());

                                        quote! { #name }
                                    });

                                let resolves = fields.unnamed.iter().enumerate().map(|(i, f)| {
                                    let index = Index::from(i + 1);
                                    let self_binding = Ident::new(&format!("self_{}", i), f.span());
                                    let resolver_binding =
                                        Ident::new(&format!("resolver_{}", i), f.span());
                                    let value =
                                        with_cast(f, parse_quote! { #self_binding }).unwrap();

                                    quote! {
                                        let (fp, fo) = out_field!(out.#index);
                                        ::rkyv::Archive::resolve(
                                            #value,
                                            pos + fp,
                                            #resolver_binding,
                                            fo
                                        );
                                    }
                                });

                                quote! {
                                    __SelfResolver::#variant(
                                        #( #resolver_bindings, )*
                                    ) => {
                                        match field {
                                            #from_ty::#variant(#(#self_bindings,)*) => {
                                                let out = out
                                                    .cast::<#archived_variant_name #ty_generics>();
                                                ::core::ptr::addr_of_mut!((*out).0)
                                                    .write(ArchivedTag::#variant);
                                                #( #resolves )*
                                            },
                                            #[allow(unreachable_patterns)]
                                            _ => ::core::hint::unreachable_unchecked(),
                                        }
                                    }
                                }
                            }
                            Fields::Unit => quote! {
                                <Self as Archive>::Resolver::#variant => {
                                    out.cast::<ArchivedTag>().write(ArchivedTag::#variant);
                                }
                            },
                        }
                    });

                    quote! {
                        #[repr(u8)]
                        enum ArchivedTag {
                            #( #archived_variant_tags, )*
                        }

                        #( #archived_variant_structs )*

                        impl #impl_generics ArchiveWith<#from_ty>
                        for #name #ty_generics #archive_where {
                            type Archived = <Self as Archive>::Archived;
                            type Resolver = <Self as Archive>::Resolver;

                            #[allow(clippy::unit_arg)]
                            #[inline]
                            unsafe fn resolve_with(
                                field: &#from_ty,
                                pos: usize,
                                resolver: <Self as Archive>::Resolver,
                                out: *mut <Self as Archive>::Archived
                            ) {
                                type __SelfResolver #ty_generics = <#name #ty_generics as Archive>::Resolver;

                                match resolver {
                                    #( #resolve_arms, )*
                                }
                            }
                        }
                    }
                })
                .collect();

            let serialize_impls = from_tys
                .iter()
                .map(|from_ty| {
                    let serialize_arms = data.variants.iter().map(|v| {
                        let variant = &v.ident;

                        match v.fields {
                            Fields::Named(ref fields) => {
                                let bindings = fields.named.iter().map(|field| {
                                    let name = &field.ident;

                                    quote!(#name)
                                });

                                let fields = fields.named.iter().map(|field| {
                                    let name = &field.ident;
                                    let field = with_cast(field, parse_quote! { #name }).unwrap();

                                    quote! {
                                        #name: Serialize::<__S>::serialize(#field, serializer)?
                                    }
                                });

                                quote! {
                                    #from_ty::#variant { #( #bindings, )* } =>
                                    __SelfResolver::#variant {
                                        #( #fields, )*
                                    }
                                }
                            }
                            Fields::Unnamed(ref fields) => {
                                let bindings = fields.unnamed.iter().enumerate().map(|(i, f)| {
                                    let name = Ident::new(&format!("_{}", i), f.span());

                                    quote! { #name }
                                });

                                let fields = fields.unnamed.iter().enumerate().map(|(i, f)| {
                                    let binding = Ident::new(&format!("_{}", i), f.span());
                                    let field = with_cast(f, parse_quote! { #binding }).unwrap();

                                    quote! {
                                        Serialize::<__S>::serialize(#field, serializer)?
                                    }
                                });

                                quote! {
                                    #from_ty::#variant( #(#bindings,)* ) =>
                                    __SelfResolver::#variant(#(#fields,)*)
                                }
                            }
                            Fields::Unit => {
                                quote! { #from_ty::#variant => <Self as Archive>::Resolver::#variant }
                            }
                        }
                    });

                    quote! {
                        impl #serialize_impl_generics SerializeWith<#from_ty, __S>
                        for #name #ty_generics #serialize_where {
                            #[inline]
                            fn serialize_with(
                                field: &#from_ty,
                                serializer: &mut __S
                            ) -> ::core::result::Result<<#name #ty_generics as Archive>::Resolver, __S::Error> {
                                type __SelfResolver #ty_generics = <#name #ty_generics as Archive>::Resolver;
                                Ok(match field {
                                    #( #serialize_arms, )*
                                })
                            }
                        }
                    }
                })
                .collect();

            (archive_impls, serialize_impls)
        }
        Data::Union(_) => {
            let msg = "ArchiveWith cannot be derived for unions";

            return Err(Error::new_spanned(input, msg));
        }
    };

    let tokens = quote! {
        #[automatically_derived]
        const _: () = {
            use ::core::marker::PhantomData;
            use ::rkyv::{out_field, Archive, Archived};

            #archive_impl
        };

        #[automatically_derived]
        const _: () = {
            use ::rkyv::{out_field, Archive, Fallible, Serialize, with::SerializeWith};

            #serialize_impl
        };
    };

    Ok(tokens)
}
