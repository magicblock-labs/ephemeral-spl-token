use proc_macro2::Span;
use quote::{format_ident, quote, ToTokens};
use syn::{
    Attribute, Expr, ExprLit, Fields, GenericArgument, Ident, ItemStruct, Lit, LitInt,
    PathArguments, Type, TypePath,
};

pub(crate) fn expand_fixed_layout(
    attr: &str,
    input: &ItemStruct,
) -> syn::Result<proc_macro2::TokenStream> {
    parse_args(attr)?;
    let mut emitted_input = input.clone();
    emitted_input
        .attrs
        .retain(|attr| !attr.path().is_ident("fixed_layout"));
    ensure_allow_dead_code(&mut emitted_input.attrs);

    let struct_name = &emitted_input.ident;
    let view_name = format_ident!("{}View", struct_name);

    let Fields::Named(fields) = &mut emitted_input.fields else {
        return Err(syn::Error::new_spanned(
            &emitted_input.fields,
            "fixed_layout requires named fields",
        ));
    };

    let mut offset = 0usize;
    let mut offsets = vec![0];

    let mut offset_expr = quote!(0);

    let mut where_bounds = Vec::<proc_macro2::TokenStream>::new();
    let mut view_methods = Vec::new();

    let mut validate_steps = Vec::new();
    let mut layout_error: Option<syn::Error> = None;

    for (_index, field) in fields.named.iter_mut().enumerate() {
        let field_ident = field.ident.as_ref().expect("named field");
        reject_max_len(field)?;

        let layout = parse_field_layout(field)?;

        strip_capacity_attr(&mut field.attrs);

        match layout.check_borrow_alignment(offset, field_ident) {
            Ok(Some(issue)) => {
                if let Some(existing) = &mut layout_error {
                    existing.combine(issue.error);
                } else {
                    layout_error = Some(issue.error);
                }
                offset += issue.padding;
            }
            Ok(None) => {}
            Err(err) => {
                if let Some(existing) = &mut layout_error {
                    existing.combine(err);
                } else {
                    layout_error = Some(err);
                }
            }
        }

        let field_offset = offset_expr.clone();
        let slot_len = layout.slot_len_expr();

        validate_steps.push(layout.gen_validate_vec_len(offset, field_ident));
        view_methods.push(layout.gen_view_methods(offset, field_ident)?);

        if let Some(bound) = layout.bound() {
            where_bounds.push(bound);
        }

        offset += layout.slot_len();
        offsets.push(offset);
        offset_expr = quote!(#field_offset + #slot_len);
    }

    let total_len_expr = offset_expr.clone();

    let count_expr = {
        let count = usize_lit(offsets.len());
        quote!(#count)
    };
    let offsets_expr = {
        // I could use offsets directly but that generates the code littered
        // with usize suffixes, e.g:
        //
        //  const OFFSETS: [usize; 6usize] = [0usize, 4usize, 12usize, 45usize, 18usize];
        //
        // I hate this, which is why I'm converting Vec<usize> into Vec<LitInt>.
        //
        let offsets: Vec<_> = offsets.into_iter().map(usize_lit).collect();

        quote! { [#(#offsets),*] }
    };

    if let Some(err) = layout_error {
        return Err(err);
    }

    let where_clause = impl_where_clause(&where_bounds);

    Ok(quote! {
        #emitted_input

        impl #struct_name {
            pub const DATA_LEN: usize = #total_len_expr;

            pub const OFFSETS: [usize; #count_expr] = #offsets_expr;

            fn __validate_bytes(
                bytes: &[u8],
            ) -> ::core::result::Result<(), ::pinocchio::error::ProgramError> {
                if bytes.len() != Self::DATA_LEN {
                    return Err(
                        ::pinocchio::error::ProgramError::InvalidInstructionData,
                    );
                }

                #(#validate_steps)*

                ::core::result::Result::Ok(())
            }

            fn __validate_option(
                bytes: &[u8],
                offset: usize,
                field_name: &'static str,
            ) -> ::core::result::Result<(), ::pinocchio::error::ProgramError> {
                match bytes[offset] {
                    0 | 1 => {}

                    tag => {
                        {
                            let mut logger = pinocchio_log::logger::Logger::<200>::default();
                            logger.append("invalid option tag for ");
                            logger.append(field_name);
                            logger.append(": ");
                            logger.append(tag);
                            logger.log();
                        };
                        return Err(::pinocchio::error::ProgramError::InvalidInstructionData);
                    }
                }
                Ok(())
            }

            fn __validate_vec_len(
                bytes: &[u8],
                offset: usize,
                capacity: usize,
                field_name: &'static str,
                expect_msg: &'static str
            ) -> ::core::result::Result<(), ::pinocchio::error::ProgramError> {
                let len = {
                    let raw: [u8; 8] = bytes[offset..offset + 8] .try_into() .expect(expect_msg);
                    u64::from_le_bytes(raw) as usize
                };
                if len > capacity {
                    let mut logger = pinocchio_log::logger::Logger::<200>::default();
                    logger.append("invalid vec length for ");
                    logger.append(field_name);
                    logger.append(": ");
                    logger.append(len);
                    logger.append(" > ");
                    logger.append(capacity);
                    logger.log();
                    return Err(::pinocchio::error::ProgramError::InvalidInstructionData);
                }
                Ok(())
            }

            pub fn try_view_from(
                bytes: &[u8],
            ) -> ::core::result::Result<#view_name<'_>, ::pinocchio::error::ProgramError> {
                Self::__validate_bytes(bytes)?;
                Ok(#view_name { bytes })
            }
        }

        #[allow(dead_code)]
        #[derive(Debug)]
        pub struct #view_name<'a> {
            bytes: &'a [u8],
        }

        impl<'a> #view_name<'a> #where_clause {
            pub fn bytes(&self) -> &'a [u8] {
                self.bytes
            }

            #(#view_methods)*
        }
    })
}

enum AccessMode {
    Copy,
    Ref,
}

enum FixedValueKind {
    Integer { ty: Type, size: usize, align: usize },
    Array { ty: Type, size: usize, align: usize },
}

impl FixedValueKind {
    fn ty(&self) -> &Type {
        match self {
            Self::Integer { ty, .. } | Self::Array { ty, .. } => ty,
        }
    }

    fn size(&self) -> usize {
        match self {
            Self::Integer { size, .. } | Self::Array { size, .. } => *size,
        }
    }

    fn align(&self) -> usize {
        match self {
            Self::Integer { align, .. } | Self::Array { align, .. } => *align,
        }
    }

    fn access_mode(&self) -> AccessMode {
        if self.size() > 8 {
            AccessMode::Ref
        } else {
            AccessMode::Copy
        }
    }

    fn needs_pod_bound(&self) -> bool {
        matches!(self, Self::Array { .. })
    }
}

enum FixedFieldKind {
    Value {
        value: FixedValueKind,
        optional: bool,
    },
    Vec {
        elem: FixedValueKind,
        capacity: usize,
    },
}

struct PaddingIssue {
    padding: usize,
    error: syn::Error,
}

impl FixedFieldKind {
    fn slot_len(&self) -> usize {
        match self {
            Self::Value { value, optional } => value.size() + usize::from(*optional),
            Self::Vec { elem, capacity } => 8 + elem.size() * capacity,
        }
    }

    fn bound(&self) -> Option<proc_macro2::TokenStream> {
        match self {
            Self::Value { value, .. } => {
                if value.needs_pod_bound() {
                    let ty = value.ty();
                    Some(quote!(#ty: ::bytemuck::Pod))
                } else {
                    None
                }
            }
            Self::Vec { elem, .. } => {
                if elem.needs_pod_bound() {
                    let ty = elem.ty();
                    Some(quote!(#ty: ::bytemuck::Pod))
                } else {
                    None
                }
            }
        }
    }

    fn check_borrow_alignment(
        &self,
        offset: usize,
        field_ident: &Ident,
    ) -> syn::Result<Option<PaddingIssue>> {
        match self {
            Self::Value { value, optional } => {
                if matches!(value.access_mode(), AccessMode::Copy) {
                    return Ok(None);
                }

                let align = value.align();
                if align > 8 {
                    return Err(syn::Error::new(
                        field_ident.span(),
                        format!(
                            "field `{}` cannot be borrowed by fixed_layout: size is {} byte(s) but alignment is {} byte(s), and fixed_layout only assumes the input buffer is 8-byte aligned",
                            field_ident,
                            value.size(),
                            align,
                        ),
                    ));
                }

                let payload_offset = offset + usize::from(*optional);
                let misalignment = payload_offset % align;
                if misalignment == 0 {
                    return Ok(None);
                }

                let padding = align - misalignment;
                let message = if *optional {
                    format!(
                        "field `{}` needs {} byte(s) of padding before it: its Option payload would start at offset {}, but borrowed values of this field must be {}-byte aligned. Insert `_pad: [u8; {}]` before `{}` so the payload starts at offset {}",
                        field_ident,
                        padding,
                        payload_offset,
                        align,
                        padding,
                        field_ident,
                        payload_offset + padding,
                    )
                } else {
                    format!(
                        "field `{}` needs {} byte(s) of padding before it: it would start at offset {}, but borrowed values of this field must be {}-byte aligned. Insert `_pad: [u8; {}]` before `{}` so it starts at offset {}",
                        field_ident,
                        padding,
                        offset,
                        align,
                        padding,
                        field_ident,
                        offset + padding,
                    )
                };
                Ok(Some(PaddingIssue {
                    padding,
                    error: syn::Error::new(field_ident.span(), message),
                }))
            }
            Self::Vec { elem, .. } => {
                let align = elem.align();
                if align > 8 {
                    return Err(syn::Error::new(
                        field_ident.span(),
                        format!(
                            "field `{}` cannot expose a slice view in fixed_layout: each Vec element is {} byte(s) but alignment is {} byte(s), and fixed_layout only assumes the input buffer is 8-byte aligned",
                            field_ident,
                            elem.size(),
                            align,
                        ),
                    ));
                }

                let first_elem_offset = offset + 8;
                let misalignment = first_elem_offset % align;
                if misalignment == 0 {
                    return Ok(None);
                }

                let padding = align - misalignment;
                Ok(Some(PaddingIssue {
                    padding,
                    error: syn::Error::new(
                        field_ident.span(),
                        format!(
                        "field `{}` needs {} byte(s) of padding before it: its Vec elements start after a 8-byte length prefix, so element 0 would start at offset {}, but slice views require {}-byte alignment. Insert `_pad: [u8; {}]` before `{}` so element 0 starts at offset {}",
                        field_ident,
                        padding,
                        first_elem_offset,
                        align,
                        padding,
                        field_ident,
                        first_elem_offset + padding,
                        ),
                    ),
                }))
            }
        }
    }

    fn slot_len_expr(&self) -> proc_macro2::TokenStream {
        match self {
            Self::Value { value, optional } => {
                let ty = value.ty();
                if *optional {
                    quote!((1 + core::mem::size_of::<#ty>()))
                } else {
                    quote!(core::mem::size_of::<#ty>())
                }
            }
            Self::Vec { elem, capacity } => {
                let elem_ty = elem.ty();
                let capacity_lit = usize_lit(*capacity);
                let len_width_lit = usize_lit(8);
                quote!((#len_width_lit + core::mem::size_of::<#elem_ty>() * #capacity_lit))
            }
        }
    }

    fn gen_validate_vec_len(&self, offset: usize, field_ident: &Ident) -> proc_macro2::TokenStream {
        let field_name = field_ident.to_string();
        let offset_lit = usize_lit(offset);
        let offset_expr = quote!(#offset_lit);
        match self {
            Self::Value { optional, .. } => {
                if *optional {
                    quote! {
                        Self::__validate_option(bytes, #offset_expr, #field_name)?;
                    }
                } else {
                    quote! {}
                }
            }
            Self::Vec { capacity, .. } => {
                let capacity_lit = usize_lit(*capacity);
                let expect_msg = format!("validate encoded-len for field '{}'", field_name);
                quote! {
                    Self::__validate_vec_len(bytes, #offset_expr, #capacity_lit, #field_name, #expect_msg)?;
                }
            }
        }
    }

    fn gen_view_methods(
        &self,
        offset: usize,
        field_ident: &Ident,
    ) -> syn::Result<proc_macro2::TokenStream> {
        match self {
            Self::Value { value, optional } => {
                let ty = value.ty();
                let access_mode = value.access_mode();
                let getter_body = getter_tokens(value, offset)?;

                match (optional, access_mode) {
                    (false, AccessMode::Copy) => Ok(quote! {
                        pub fn #field_ident(&self) -> #ty {
                            #getter_body
                        }
                    }),
                    (false, AccessMode::Ref) => Ok(quote! {
                        pub fn #field_ident(&self) -> &#ty {
                            #getter_body
                        }
                    }),
                    (true, AccessMode::Copy) => {
                        let value_body = getter_tokens(value, offset + 1)?;
                        Ok(quote! {
                            pub fn #field_ident(&self) -> ::core::option::Option<#ty> {
                                if self.bytes[(#offset)] == 0 {
                                    ::core::option::Option::None
                                } else {
                                    ::core::option::Option::Some(#value_body)
                                }
                            }
                        })
                    }
                    (true, AccessMode::Ref) => {
                        let value_body = getter_tokens(value, offset + 1)?;
                        Ok(quote! {
                            pub fn #field_ident(&self) -> ::core::option::Option<&#ty> {
                                if self.bytes[(#offset)] == 0 {
                                    ::core::option::Option::None
                                } else {
                                    ::core::option::Option::Some(#value_body)
                                }
                            }
                        })
                    }
                }
            }
            Self::Vec { elem, capacity } => {
                let elem_ty = elem.ty();
                let len_expr = read_len_expr(offset);
                let offset = usize_lit(offset);
                let len_width_lit = usize_lit(8);
                let capacity_name = format_ident!("{}_capacity", accessor_ident(field_ident));
                let capacity_lit = usize_lit(*capacity);
                Ok(quote! {
                    pub fn #field_ident(&self) -> &[#elem_ty] {
                        let len = #len_expr;
                        let start = #offset + #len_width_lit;
                        let end = start + (len * ::core::mem::size_of::<#elem_ty>());
                        ::bytemuck::cast_slice::<u8, #elem_ty>(&self.bytes[start..end])
                    }

                    pub const fn #capacity_name(&self) -> usize {
                        #capacity_lit
                    }
                })
            }
        }
    }
}

fn parse_field_layout(field: &syn::Field) -> syn::Result<FixedFieldKind> {
    let ty = &field.ty;
    let capacity = capacity_attr(field)?;
    if let Some(elem_ty) = vec_inner(ty)? {
        let capacity = capacity.ok_or_else(|| {
            syn::Error::new_spanned(
                field,
                "Vec fields in fixed_layout require `#[capacity = N]`",
            )
        })?;
        return Ok(FixedFieldKind::Vec {
            elem: parse_value_kind(elem_ty)?,
            capacity,
        });
    }

    let (inner_ty, optional) = if let Some(inner) = option_inner(ty) {
        if vec_inner(inner)?.is_some() {
            return Err(syn::Error::new_spanned(
                field,
                "Option<Vec<T>> is not supported in fixed_layout",
            ));
        }
        (inner, true)
    } else {
        (ty, false)
    };

    if capacity.is_some() {
        return Err(syn::Error::new_spanned(
            field,
            "#[capacity = N] is only supported on Vec<T> fields in fixed_layout",
        ));
    }

    if is_string(&inner_ty) {
        return Err(syn::Error::new_spanned(
            field,
            "String is not supported by fixed_layout",
        ));
    }

    Ok(FixedFieldKind::Value {
        value: parse_value_kind(inner_ty)?,
        optional,
    })
}

fn parse_value_kind(ty: &Type) -> syn::Result<FixedValueKind> {
    if let Some((size, align)) = integer_size_and_align(ty) {
        return Ok(FixedValueKind::Integer {
            ty: ty.clone(),
            size,
            align,
        });
    }

    let (size, align) = fixed_array_size_and_align(ty)?;
    Ok(FixedValueKind::Array {
        ty: ty.clone(),
        size,
        align,
    })
}

fn ensure_allow_dead_code(attrs: &mut Vec<Attribute>) {
    if attrs.iter().any(is_allow_dead_code) {
        return;
    }

    attrs.push(syn::parse_quote!(#[allow(dead_code)]));
}

fn is_allow_dead_code(attr: &Attribute) -> bool {
    if !attr.path().is_ident("allow") {
        return false;
    }

    let mut found = false;
    let _ = attr.parse_nested_meta(|meta| {
        if meta.path.is_ident("dead_code") {
            found = true;
        }
        Ok(())
    });
    found
}

fn parse_args(attr: &str) -> syn::Result<()> {
    match attr.trim() {
        "" => Ok(()),
        _ => Err(syn::Error::new(
            Span::call_site(),
            "fixed_layout does not support parameters",
        )),
    }
}

fn impl_where_clause(bounds: &[proc_macro2::TokenStream]) -> proc_macro2::TokenStream {
    if bounds.is_empty() {
        quote!()
    } else {
        quote!(where #(#bounds,)*)
    }
}

fn reject_max_len(field: &syn::Field) -> syn::Result<()> {
    for attr in &field.attrs {
        if attr.path().is_ident("max_len") {
            return Err(syn::Error::new_spanned(
                attr,
                "#[max_len] is not used by fixed_layout",
            ));
        }
    }
    Ok(())
}

fn strip_capacity_attr(attrs: &mut Vec<Attribute>) {
    attrs.retain(|attr| !attr.path().is_ident("capacity"));
}

fn capacity_attr(field: &syn::Field) -> syn::Result<Option<usize>> {
    let mut capacity = None;

    for attr in &field.attrs {
        if !attr.path().is_ident("capacity") {
            continue;
        }

        if capacity.is_some() {
            return Err(syn::Error::new_spanned(
                attr,
                "duplicate `#[capacity = N]` on fixed_layout field",
            ));
        }

        let syn::Meta::NameValue(meta) = &attr.meta else {
            return Err(syn::Error::new_spanned(
                attr,
                "capacity must use the form `#[capacity = N]`",
            ));
        };

        let Expr::Lit(ExprLit {
            lit: Lit::Int(lit_int),
            ..
        }) = &meta.value
        else {
            return Err(syn::Error::new_spanned(
                &meta.value,
                "capacity must be an integer literal",
            ));
        };

        capacity = Some(lit_int.base10_parse()?);
    }

    Ok(capacity)
}

fn option_inner(ty: &Type) -> Option<&Type> {
    let Type::Path(TypePath { path, .. }) = ty else {
        return None;
    };
    let segment = path.segments.last()?;
    if segment.ident != "Option" {
        return None;
    }
    let PathArguments::AngleBracketed(args) = &segment.arguments else {
        return None;
    };
    let GenericArgument::Type(inner) = args.args.first()? else {
        return None;
    };
    Some(inner)
}

fn vec_inner(ty: &Type) -> syn::Result<Option<&Type>> {
    let Type::Path(TypePath { path, .. }) = ty else {
        return Ok(None);
    };
    let Some(segment) = path.segments.last() else {
        return Ok(None);
    };
    if segment.ident != "Vec" {
        return Ok(None);
    }
    let PathArguments::AngleBracketed(args) = &segment.arguments else {
        return Err(syn::Error::new_spanned(
            &segment.arguments,
            "Vec requires exactly one type argument in fixed_layout: Vec<T>",
        ));
    };
    if args.args.len() != 1 {
        return Err(syn::Error::new_spanned(
            args,
            "Vec requires exactly one type argument in fixed_layout: Vec<T>",
        ));
    }

    let Some(GenericArgument::Type(elem_ty)) = args.args.first() else {
        return Err(syn::Error::new_spanned(args, "Vec element must be a type"));
    };

    Ok(Some(elem_ty))
}

fn integer_primitive_name(ty: &Type) -> Option<String> {
    let Type::Path(type_path) = ty else {
        return None;
    };
    if type_path.qself.is_some() || type_path.path.segments.len() != 1 {
        return None;
    }
    let ident = type_path.path.segments[0].ident.to_string();
    match ident.as_str() {
        "i8" | "u8" | "i16" | "u16" | "i32" | "u32" | "i64" | "u64" | "i128" | "u128" | "isize"
        | "usize" => Some(ident),
        _ => None,
    }
}

fn is_string(ty: &Type) -> bool {
    let Type::Path(type_path) = ty else {
        return false;
    };
    type_path.qself.is_none()
        && type_path.path.segments.len() == 1
        && type_path.path.segments[0].ident == "String"
}

fn usize_lit(value: usize) -> LitInt {
    LitInt::new(&value.to_string(), Span::call_site())
}

fn accessor_ident(field_ident: &Ident) -> Ident {
    let field_name = field_ident.to_string();
    let trimmed = field_name.trim_start_matches('_');
    if trimmed.is_empty() {
        format_ident!("{}", field_name)
    } else {
        format_ident!("{}", trimmed)
    }
}

fn bytes_slice_expr(offset: usize, len: usize) -> proc_macro2::TokenStream {
    let offset = usize_lit(offset);
    let len = usize_lit(len);
    quote!(&self.bytes[#offset..#offset + #len])
}

fn parse_integer_expr(
    ty: &Type,
    bytes_expr: proc_macro2::TokenStream,
) -> syn::Result<proc_macro2::TokenStream> {
    let Some(name) = integer_primitive_name(ty) else {
        return Err(syn::Error::new_spanned(
            ty,
            "field must be an integer primitive",
        ));
    };
    let ty_tokens = ty.to_token_stream();
    Ok(match name.as_str() {
        "i8" => quote!(i8::from_le_bytes([(#bytes_expr)[0]])),
        "u8" => quote!(u8::from_le_bytes([(#bytes_expr)[0]])),
        _ => quote!({
            let raw: [u8; ::core::mem::size_of::<#ty_tokens>()] = (#bytes_expr)
                .try_into()
                .expect("validated slice length");
            <#ty_tokens>::from_le_bytes(raw)
        }),
    })
}

fn integer_size_and_align(ty: &Type) -> Option<(usize, usize)> {
    match integer_primitive_name(ty).as_deref() {
        Some("i8" | "u8") => Some((1, 1)),
        Some("i16" | "u16") => Some((2, 2)),
        Some("i32" | "u32") => Some((4, 4)),
        Some("i64" | "u64") => Some((8, 8)),
        Some("i128" | "u128") => Some((16, 16)),
        Some("isize" | "usize") => {
            let size = ::core::mem::size_of::<usize>();
            Some((size, size))
        }
        _ => None,
    }
}

fn fixed_array_size_and_align(ty: &Type) -> syn::Result<(usize, usize)> {
    let Type::Array(array) = ty else {
        return Err(syn::Error::new_spanned(
            ty,
            "fixed_layout fields must be integer primitives or fixed-size arrays",
        ));
    };

    let Expr::Lit(ExprLit {
        lit: Lit::Int(len_lit),
        ..
    }) = &array.len
    else {
        return Err(syn::Error::new_spanned(
            &array.len,
            "array length must be an integer literal",
        ));
    };

    let len = len_lit.base10_parse::<usize>()?;
    let (elem_size, elem_align) = if let Some(size_align) = integer_size_and_align(&array.elem) {
        size_align
    } else {
        fixed_array_size_and_align(&array.elem)?
    };

    Ok((len * elem_size, elem_align))
}

fn read_copy_expr(
    value: &FixedValueKind,
    bytes_expr: proc_macro2::TokenStream,
) -> syn::Result<proc_macro2::TokenStream> {
    match value {
        FixedValueKind::Integer { ty, .. } => parse_integer_expr(ty, bytes_expr),
        FixedValueKind::Array { ty, .. } => Ok(quote! {
            unsafe { ::core::ptr::read_unaligned((#bytes_expr).as_ptr().cast::<#ty>()) }
        }),
    }
}

fn getter_tokens(value: &FixedValueKind, offset: usize) -> syn::Result<proc_macro2::TokenStream> {
    let ty = value.ty();
    let slice_expr = bytes_slice_expr(offset, value.size());
    match value.access_mode() {
        AccessMode::Copy => read_copy_expr(value, slice_expr),
        AccessMode::Ref => Ok(borrow_ref_expr(ty, slice_expr)),
    }
}

fn borrow_ref_expr(ty: &Type, bytes_expr: proc_macro2::TokenStream) -> proc_macro2::TokenStream {
    quote!(::bytemuck::from_bytes::<#ty>(#bytes_expr))
}

fn read_len_expr(offset: usize) -> proc_macro2::TokenStream {
    quote!({
        let raw: [u8; 8] = self.bytes[#offset..#offset + 8]
            .try_into()
            .expect("validated len");
        u64::from_le_bytes(raw) as usize
    })
}

#[cfg(test)]
mod tests {
    use super::expand_fixed_layout;
    use syn::parse_quote;

    #[test]
    fn fixed_layout_reports_padding_for_large_field() {
        let item: syn::ItemStruct = parse_quote! {
            struct Args {
                flag: u8,
                payload: [u64; 2],
            }
        };

        let error = expand_fixed_layout("", &item).unwrap_err().to_string();
        assert!(error.contains("field `payload` needs 7 byte(s) of padding before it"));
        assert!(error.contains("starts at offset 8"));
    }

    #[test]
    fn fixed_layout_reports_padding_for_optional_payload() {
        let item: syn::ItemStruct = parse_quote! {
            struct Args {
                flag: u8,
                payload: Option<[u64; 2]>,
            }
        };

        let error = expand_fixed_layout("", &item).unwrap_err().to_string();
        assert!(error.contains("field `payload` needs 6 byte(s) of padding before it"));
        assert!(error.contains("Option payload would start at offset 2"));
        assert!(error.contains("payload starts at offset 8"));
    }

    #[test]
    fn fixed_layout_reports_padding_for_vec_elements() {
        let item: syn::ItemStruct = parse_quote! {
            struct Args {
                flag: u8,
                #[capacity = 2]
                values: Vec<[u64; 2]>,
            }
        };

        let error = expand_fixed_layout("", &item).unwrap_err().to_string();
        assert!(error.contains("field `values` needs 7 byte(s) of padding before it"));
        assert!(error.contains("element 0 would start at offset 9"));
        assert!(error.contains("element 0 starts at offset 16"));
    }

    #[test]
    fn fixed_layout_assumes_earlier_padding_errors_are_fixed() {
        let item: syn::ItemStruct = parse_quote! {
            struct Args {
                flag: u8,
                payload: [u64; 2],
                _pad1: [u8; 7],
                #[capacity = 2]
                values: Vec<[u64; 2]>,
            }
        };

        let error = expand_fixed_layout("", &item).unwrap_err().to_string();
        assert!(error.contains("field `payload` needs 7 byte(s) of padding before it"));
        assert!(!error.contains("field `values`"));
    }

    #[test]
    fn fixed_layout_rejects_alignment_above_eight() {
        let item: syn::ItemStruct = parse_quote! {
            struct Args {
                big: u128,
            }
        };

        let error = expand_fixed_layout("", &item).unwrap_err().to_string();
        assert!(error.contains("field `big` cannot be borrowed by fixed_layout"));
        assert!(error.contains("alignment is 16 byte(s)"));
        assert!(error.contains("8-byte aligned"));
    }

    #[test]
    fn fixed_layout_rejects_vec_without_capacity() {
        let item: syn::ItemStruct = parse_quote! {
            struct Args {
                values: Vec<u16>,
            }
        };

        let error = expand_fixed_layout("", &item).unwrap_err().to_string();
        assert!(error.contains("Vec fields in fixed_layout require `#[capacity = N]`"));
    }

    #[test]
    fn fixed_layout_rejects_capacity_on_non_vec() {
        let item: syn::ItemStruct = parse_quote! {
            struct Args {
                #[capacity = 2]
                value: u16,
            }
        };

        let error = expand_fixed_layout("", &item).unwrap_err().to_string();
        assert!(error.contains("#[capacity = N] is only supported on Vec<T> fields"));
    }

    #[test]
    fn fixed_layout_rejects_parameters() {
        let item: syn::ItemStruct = parse_quote! {
            struct Args {
                value: u16,
            }
        };

        let error = expand_fixed_layout("mut", &item).unwrap_err().to_string();
        assert!(error.contains("fixed_layout does not support parameters"));
    }
}
