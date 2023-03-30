use syn::{
    parenthesized, parse_quote, punctuated::Punctuated, token::Comma, Attribute, Error, Expr,
    Field, Ident, LitStr, Path, Result, Type,
};

use crate::ATTR;

pub fn parse_top_attrs(attrs: &[Attribute]) -> Result<Vec<Type>> {
    let mut from = Vec::new();

    for attr in attrs {
        if !attr.path().is_ident(ATTR) {
            continue;
        }

        attr.parse_nested_meta(|meta| {
            if meta.path.is_ident("from") {
                let content;
                parenthesized!(content in meta.input);
                let types = Punctuated::<_, Comma>::parse_terminated(&content)?;
                from.extend(types);

                Ok(())
            } else {
                Err(Error::new_spanned(meta.path, "expected `from`"))
            }
        })?;
    }

    Ok(from)
}

#[derive(Default)]
pub struct ParsedAttributes {
    pub from: Option<Type>,
    pub via: Option<Type>,
    pub getter: Option<Getter>,
}

pub struct Getter {
    pub path: Path,
    pub owned_self: bool,
}

impl ParsedAttributes {
    pub fn new(attrs: &[Attribute]) -> Result<Self> {
        let mut parsed = ParsedAttributes::default();
        let mut getter_path = None;
        let mut getter_owned = false;

        for attr in attrs {
            if attr.path().is_ident(ATTR) {
                attr.parse_nested_meta(|meta| {
                    if meta.path.is_ident("from") {
                        let content;
                        parenthesized!(content in meta.input);
                        parsed.from = Some(content.parse()?);
                    } else if meta.path.is_ident("via") {
                        let content;
                        parenthesized!(content in meta.input);
                        parsed.via = Some(content.parse()?);
                    } else if meta.path.is_ident("getter") {
                        getter_path = Some(meta.value()?.parse::<LitStr>()?.parse()?);
                    } else if meta.path.is_ident("getter_owned") {
                        getter_owned = true;
                    }

                    Ok(())
                })?;
            }
        }

        if let Some(path) = getter_path {
            parsed.getter = Some(Getter {
                path,
                owned_self: getter_owned,
            });
        }

        Ok(parsed)
    }
}

pub fn with<B, F: FnMut(B, &Type) -> B>(field: &Field, init: B, f: F) -> Result<B> {
    let fields = field
        .attrs
        .iter()
        .filter_map(|attr| {
            if attr.path().is_ident("with") {
                Some(attr.parse_args_with(Punctuated::<Type, Comma>::parse_separated_nonempty))
            } else {
                None
            }
        })
        .collect::<Result<Vec<_>>>()?;
    Ok(fields.iter().flatten().rev().fold(init, f))
}

pub fn with_ty(field: &Field) -> Result<Type> {
    let ty = &field.ty;
    let parsed_attr = ParsedAttributes::new(&field.attrs)?;

    match (parsed_attr.from, parsed_attr.via) {
        (Some(from_ty), None) => Ok(parse_quote! { ::rkyv::with::With<#from_ty, #ty> }),
        (Some(from_ty), Some(via_ty)) => Ok(parse_quote! { ::rkyv::with::With<#from_ty, #via_ty> }),
        (None, Some(via_ty)) => Ok(parse_quote! { ::rkyv::with::With<#ty, #via_ty> }),
        (None, None) => with(
            field,
            ty.clone(),
            |ty, wrapper| parse_quote! { ::rkyv::with::With<#ty, #wrapper> },
        ),
    }
}

pub fn with_cast(field: &Field, expr: Expr) -> Result<Expr> {
    let ty = &field.ty;
    let parsed_attr = ParsedAttributes::new(&field.attrs)?;

    match (parsed_attr.from, parsed_attr.via) {
        (Some(_), None) => Ok(parse_quote! { ::rkyv::with::With::<_, #ty>::cast(#expr) }),
        (_, Some(via)) => Ok(parse_quote! { ::rkyv::with::With::<_, #via>::cast(#expr) }),
        (None, None) => with(
            field,
            expr,
            |expr, wrapper| parse_quote! { ::rkyv::with::With::<_, #wrapper>::cast(#expr) },
        ),
    }
}

pub fn with_inner(field: &Field, expr: Expr) -> Result<Expr> {
    with(field, expr, |expr, _| parse_quote! { #expr.into_inner() })
}

pub fn strip_raw(ident: &Ident) -> String {
    let as_string = ident.to_string();

    as_string
        .strip_prefix("r#")
        .map(ToString::to_string)
        .unwrap_or(as_string)
}
