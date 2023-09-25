use std::iter;

use quote::quote;
use syn::{
    parenthesized,
    parse::{Parse, ParseStream},
    parse_quote,
    token::Token as TokenTrait,
    Attribute, Error, Expr, Field, Ident, LitStr, Path, Result, Token, Type,
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
                let mut types = Vec::parse_terminated::<Token![,]>(&content)?;
                from.append(&mut types);

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
    pub via: Option<Vec<Type>>,
    pub getter: Option<Getter>,
}

pub struct Getter {
    pub path: Path,
    pub owned_self: bool,
}

impl Getter {
    pub fn make_expr(&self, from_ty: &Type) -> Expr {
        let Self { path, owned_self } = self;

        if *owned_self {
            parse_quote! { #path (<#from_ty as Clone>::clone(field)) }
        } else {
            parse_quote! { #path (field) }
        }
    }
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
                        parsed.via = Some(Vec::parse_separated_nonempty::<Token![,]>(&content)?);
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
                Some(attr.parse_args_with(Vec::parse_separated_nonempty::<Token![,]>))
            } else {
                None
            }
        })
        .collect::<Result<Vec<_>>>()?;

    Ok(fields.iter().flatten().rev().fold(init, f))
}

pub fn with_ty(field: &Field) -> Result<(Type, ParsedAttributes)> {
    let ty = &field.ty;
    let parsed_attrs = ParsedAttributes::new(&field.attrs)?;

    let ty = match (&parsed_attrs.from, &parsed_attrs.via) {
        (Some(from_ty), Some(via_tys)) => via_tys.iter().rev().fold(
            from_ty.clone(),
            |ty, wrapper| parse_quote! { ::rkyv::with::With<#ty, #wrapper> },
        ),
        (Some(from_ty), None) => parse_quote! { ::rkyv::with::With<#from_ty, #ty> },
        (None, Some(via_tys)) => via_tys.iter().rev().fold(
            ty.clone(),
            |ty, wrapper| parse_quote! { ::rkyv::with::With<#ty, #wrapper> },
        ),
        (None, None) => with(
            field,
            ty.clone(),
            |ty, wrapper| parse_quote! { ::rkyv::with::With<#ty, #wrapper> },
        )?,
    };

    Ok((ty, parsed_attrs))
}

pub fn with_cast(field: &Field, expr: Expr) -> Result<Expr> {
    let ty = &field.ty;
    let parsed_attr = ParsedAttributes::new(&field.attrs)?;

    let expr = match (parsed_attr.from, parsed_attr.via) {
        (Some(_), None) => parse_quote! { ::rkyv::with::With::<_, #ty>::cast(#expr) },
        (_, Some(via_tys)) => via_tys.iter().rev().fold(
            expr,
            |expr, wrapper| parse_quote! { ::rkyv::with::With::<_, #wrapper>::cast(#expr) },
        ),
        (None, None) => with(
            field,
            expr,
            |expr, wrapper| parse_quote! { ::rkyv::with::With::<_, #wrapper>::cast(#expr) },
        )?,
    };

    Ok(expr)
}

pub fn with_inner(field: &Field, attrs: &ParsedAttributes, expr: Expr) -> Result<Expr> {
    if attrs.from.is_none() && attrs.via.is_none() {
        with(field, expr, |expr, _| parse_quote! { #expr.into_inner() })
    } else {
        let into_inner_count = attrs.via.as_ref().map_or(1, Vec::len);
        let into_inners = iter::repeat(quote!(.into_inner())).take(into_inner_count);

        Ok(parse_quote! { #expr #( #into_inners )* })
    }
}

pub fn strip_raw(ident: &Ident) -> String {
    let as_string = ident.to_string();

    as_string
        .strip_prefix("r#")
        .map(ToString::to_string)
        .unwrap_or(as_string)
}

/// Revamping utility from [`Punctuated`] for the purpose of storing items
/// more efficiently.
///
/// [`Punctuated`]: syn::punctuated::Punctuated
pub trait PunctuatedExt<T> {
    /// Parses one or more occurrences of `T` separated by punctuation of type
    /// `P`, not accepting trailing punctuation.
    ///
    /// Parsing continues as long as punctuation `P` is present at the head of
    /// the stream. This method returns upon parsing a `T` and observing that it
    /// is not followed by a `P`, even if there are remaining tokens in the
    /// stream.
    fn parse_separated_nonempty<P: Parse + TokenTrait>(input: ParseStream) -> Result<Vec<T>>
    where
        T: Parse,
    {
        Self::parse_separated_nonempty_with::<P>(input, T::parse)
    }

    /// Parses one or more occurrences of `T` using the given parse function,
    /// separated by punctuation of type `P`, not accepting trailing
    /// punctuation.
    ///
    /// Like [`parse_separated_nonempty`], may complete early without parsing
    /// the entire content of this stream.
    fn parse_separated_nonempty_with<P: Parse + TokenTrait>(
        input: ParseStream,
        parser: fn(ParseStream) -> Result<T>,
    ) -> Result<Vec<T>>;

    /// Parses zero or more occurrences of `T` separated by punctuation of type
    /// `P`, with optional trailing punctuation.
    ///
    /// Parsing continues until the end of this parse stream. The entire content
    /// of this parse stream must consist of `T` and `P`.
    fn parse_terminated<P: Parse>(input: ParseStream) -> Result<Vec<T>>
    where
        T: Parse,
    {
        Self::parse_terminated_with::<P>(input, T::parse)
    }

    /// Parses zero or more occurrences of `T` using the given parse function,
    /// separated by punctuation of type `P`, with optional trailing
    /// punctuation.
    ///
    /// Like [`parse_terminated`], the entire content of this stream is expected
    /// to be parsed.
    fn parse_terminated_with<P: Parse>(
        input: ParseStream,
        parser: fn(ParseStream) -> Result<T>,
    ) -> Result<Vec<T>>;
}

impl<T> PunctuatedExt<T> for Vec<T> {
    fn parse_separated_nonempty_with<P: Parse + TokenTrait>(
        input: ParseStream,
        parser: fn(ParseStream) -> Result<T>,
    ) -> Result<Self> {
        let mut vec = Vec::new();

        loop {
            vec.push(parser(input)?);

            if !P::peek(input.cursor()) {
                break;
            }

            input.parse::<P>()?;
        }

        Ok(vec)
    }

    fn parse_terminated_with<P: Parse>(
        input: ParseStream,
        parser: fn(ParseStream) -> Result<T>,
    ) -> Result<Self> {
        let mut vec = Vec::new();

        loop {
            if input.is_empty() {
                break;
            }

            vec.push(parser(input)?);

            if input.is_empty() {
                break;
            }

            input.parse::<P>()?;
        }

        Ok(vec)
    }
}
