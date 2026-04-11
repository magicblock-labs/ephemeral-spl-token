use proc_macro2::Span;
use quote::{format_ident, quote, ToTokens};
use syn::{
    spanned::Spanned, Attribute, Expr, ExprLit, Fields, GenericArgument, Ident, ItemStruct, Lit,
    LitInt, Meta, PathArguments, Type, TypePath,
};

pub(crate) fn expand_variable_layout(
    attr: &str,
    input: &ItemStruct,
) -> syn::Result<proc_macro2::TokenStream> {
    let mutable = parse_args(attr)?;
    let mut emitted_input = input.clone();
    emitted_input
        .attrs
        .retain(|attr| !attr.path().is_ident("variable_layout"));
    ensure_allow_dead_code(&mut emitted_input.attrs);

    let struct_name = &emitted_input.ident;
    let view_name = format_ident!("{}View", struct_name);
    let view_mut_name = format_ident!("{}ViewMut", struct_name);

    let Fields::Named(fields) = &mut emitted_input.fields else {
        return Err(syn::Error::new_spanned(
            &emitted_input.fields,
            "variable_layout requires named fields",
        ));
    };

    let mut total_len = 0usize;
    let mut where_bounds = Vec::<proc_macro2::TokenStream>::new();
    let mut view_methods = Vec::new();
    let mut view_mut_methods = Vec::new();
    let mut validate_steps = Vec::new();

    for field in &mut fields.named {
        let field_ident = field.ident.as_ref().expect("named field");
        let field_ty = &field.ty;

        let layout = field_layout(field)?;
        field.attrs.retain(|attr| !attr.path().is_ident("max_len"));
        let offset = total_len;
        total_len += layout.slot_len();

        validate_steps.push(layout.validate(offset, field_ident));
        view_methods.push(layout.view_method(offset, field_ident, field_ty)?);

        if mutable {
            view_mut_methods.push(layout.view_mut_methods(offset, field_ident, field_ty)?);
        }

        if layout.needs_pod_bound() {
            let pod_ty = layout.bound_type(field_ty);
            where_bounds.push(quote!(#pod_ty: ::bytemuck::Pod));
        }
    }

    let data_len_lit = syn::LitInt::new(&total_len.to_string(), Span::call_site());
    let where_clause = impl_where_clause(&where_bounds);

    let mut_struct = if mutable {
        quote! {
            #[allow(dead_code)]
            #[derive(Debug)]
            pub struct #view_mut_name<'a> {
                bytes: &'a mut [u8],
            }

            impl<'a> #view_mut_name<'a> #where_clause {
                pub fn bytes(&self) -> &[u8] {
                    self.bytes
                }

                #(#view_mut_methods)*
            }
        }
    } else {
        quote!()
    };

    let try_view_from_mut = if mutable {
        quote! {
            pub fn try_view_from_mut(
                bytes: &mut [u8],
            ) -> ::core::result::Result<#view_mut_name<'_>, ::pinocchio::error::ProgramError> {
                Self::__validate_bytes(bytes)?;
                ::core::result::Result::Ok(#view_mut_name { bytes })
            }
        }
    } else {
        quote!()
    };

    Ok(quote! {
        #emitted_input

        impl #struct_name {
            pub const DATA_LEN: usize = #data_len_lit;

            fn __validate_bytes(
                bytes: &[u8],
            ) -> ::core::result::Result<(), ::pinocchio::error::ProgramError> {
                if bytes.len() != Self::DATA_LEN {
                    return ::core::result::Result::Err(
                        ::pinocchio::error::ProgramError::InvalidInstructionData,
                    );
                }

                #(#validate_steps)*

                ::core::result::Result::Ok(())
            }

            pub fn try_view_from(
                bytes: &[u8],
            ) -> ::core::result::Result<#view_name<'_>, ::pinocchio::error::ProgramError> {
                Self::__validate_bytes(bytes)?;
                ::core::result::Result::Ok(#view_name { bytes })
            }

            #try_view_from_mut
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

        #mut_struct
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

fn parse_args(attr: &str) -> syn::Result<bool> {
    match attr.trim() {
        "" => Ok(false),
        "mut" => Ok(true),
        _ => Err(syn::Error::new(
            Span::call_site(),
            "variable_layout only supports the `mut` parameter",
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

fn parse_max_len(attrs: &[Attribute]) -> syn::Result<Option<usize>> {
    for attr in attrs {
        if !attr.path().is_ident("max_len") {
            continue;
        }

        match &attr.meta {
            Meta::NameValue(name_value) => {
                let Expr::Lit(ExprLit {
                    lit: Lit::Int(lit_int),
                    ..
                }) = &name_value.value
                else {
                    return Err(syn::Error::new_spanned(
                        &name_value.value,
                        "max_len must be an integer literal",
                    ));
                };
                return Ok(Some(lit_int.base10_parse()?));
            }
            Meta::List(list) => {
                let lit_int: LitInt = list.parse_args()?;
                return Ok(Some(lit_int.base10_parse()?));
            }
            Meta::Path(path) => {
                return Err(syn::Error::new_spanned(
                    path,
                    "max_len requires an integer value",
                ));
            }
        }
    }

    Ok(None)
}

fn len_width(max_len: usize, span: proc_macro2::Span) -> syn::Result<usize> {
    if max_len <= 0xFF {
        Ok(1)
    } else if max_len <= 0xFFFF {
        Ok(2)
    } else if max_len <= 0xFFFFFF {
        Ok(3)
    } else if max_len <= 0xFFFFFFFF {
        Ok(4)
    } else {
        Err(syn::Error::new(
            span,
            "max_len above 0xFFFF_FFFF is not supported",
        ))
    }
}

enum BaseKind {
    Int,
    Pod,
    String { max_len: usize, len_width: usize },
}

struct FieldLayout {
    optional: bool,
    base: BaseKind,
    fixed_size: Option<usize>,
}

impl FieldLayout {
    fn slot_len(&self) -> usize {
        let base_len = match self.base {
            BaseKind::Int | BaseKind::Pod => 0, // computed externally by bound type
            BaseKind::String { max_len, len_width } => len_width + max_len,
        };

        match &self.base {
            BaseKind::Int | BaseKind::Pod => {
                let fixed = self.fixed_size.expect("fixed");
                if self.optional {
                    1 + fixed
                } else {
                    fixed
                }
            }
            BaseKind::String { .. } => {
                if self.optional {
                    1 + base_len
                } else {
                    base_len
                }
            }
        }
    }

    fn needs_pod_bound(&self) -> bool {
        matches!(self.base, BaseKind::Pod)
    }

    fn bound_type<'a>(&self, field_ty: &'a Type) -> &'a Type {
        if self.optional {
            option_inner(field_ty).expect("optional inner type")
        } else {
            field_ty
        }
    }

    fn validate(&self, offset: usize, field_ident: &Ident) -> proc_macro2::TokenStream {
        let field_name = field_ident.to_string();
        match (&self.base, self.optional) {
            (BaseKind::Int, false) | (BaseKind::Pod, false) => quote! {},
            (BaseKind::Int, true) | (BaseKind::Pod, true) => {
                let tag = syn::LitInt::new(&offset.to_string(), Span::call_site());
                quote! {
                    match bytes[#tag] {
                        0 | 1 => {}
                        tag => {
                            ::pinocchio_log::log!(
                                "invalid option tag for {}: {}",
                                #field_name,
                                tag
                            );
                            return ::core::result::Result::Err(
                                ::pinocchio::error::ProgramError::InvalidInstructionData,
                            );
                        }
                    }
                }
            }
            (BaseKind::String { max_len, len_width }, false) => {
                validate_string_field(offset, *max_len, *len_width)
            }
            (BaseKind::String { max_len, len_width }, true) => {
                let tag = syn::LitInt::new(&offset.to_string(), Span::call_site());
                let inner = validate_string_field(offset + 1, *max_len, *len_width);
                quote! {
                    match bytes[#tag] {
                        0 => {}
                        1 => {
                            #inner
                        }
                        tag => {
                            ::pinocchio_log::log!(
                                "invalid option tag for {}: {}",
                                #field_name,
                                tag
                            );
                            return ::core::result::Result::Err(
                                ::pinocchio::error::ProgramError::InvalidInstructionData,
                            );
                        }
                    }
                }
            }
        }
    }

    fn view_method(
        &self,
        offset: usize,
        field_ident: &Ident,
        field_ty: &Type,
    ) -> syn::Result<proc_macro2::TokenStream> {
        let base_ty = self.bound_type(field_ty);
        match (&self.base, self.optional) {
            (BaseKind::Int, false) => {
                let expr =
                    parse_integer_expr(base_ty, bytes_range(offset, self.slot_len()), false)?;
                Ok(quote! {
                    pub fn #field_ident(&self) -> #base_ty {
                        #expr
                    }
                })
            }
            (BaseKind::Pod, false) => {
                let start = syn::LitInt::new(&offset.to_string(), Span::call_site());
                let end = syn::LitInt::new(
                    &(offset + fixed_pod_size(base_ty)?).to_string(),
                    Span::call_site(),
                );
                Ok(quote! {
                    pub fn #field_ident(&self) -> &#base_ty {
                        ::bytemuck::from_bytes::<#base_ty>(&self.bytes[#start..#end])
                    }
                })
            }
            (BaseKind::String { max_len, len_width }, false) => Ok(view_string_method(
                field_ident,
                offset,
                *max_len,
                *len_width,
            )),
            (BaseKind::Int, true) => {
                let tag = syn::LitInt::new(&offset.to_string(), Span::call_site());
                let start = offset + 1;
                let size = integer_size(base_ty)?;
                let expr = parse_integer_expr(base_ty, bytes_range(start, size), false)?;
                Ok(quote! {
                    pub fn #field_ident(&self) -> ::core::option::Option<#base_ty> {
                        if self.bytes[#tag] == 0 {
                            ::core::option::Option::None
                        } else {
                            ::core::option::Option::Some(#expr)
                        }
                    }
                })
            }
            (BaseKind::Pod, true) => {
                let tag = syn::LitInt::new(&offset.to_string(), Span::call_site());
                let start = syn::LitInt::new(&(offset + 1).to_string(), Span::call_site());
                let end = syn::LitInt::new(
                    &(offset + 1 + fixed_pod_size(base_ty)?).to_string(),
                    Span::call_site(),
                );
                Ok(quote! {
                    pub fn #field_ident(&self) -> ::core::option::Option<&#base_ty> {
                        if self.bytes[#tag] == 0 {
                            ::core::option::Option::None
                        } else {
                            ::core::option::Option::Some(
                                ::bytemuck::from_bytes::<#base_ty>(&self.bytes[#start..#end]),
                            )
                        }
                    }
                })
            }
            (BaseKind::String { max_len, len_width }, true) => Ok(view_optional_string_method(
                field_ident,
                offset,
                *max_len,
                *len_width,
            )),
        }
    }

    fn view_mut_methods(
        &self,
        offset: usize,
        field_ident: &Ident,
        field_ty: &Type,
    ) -> syn::Result<proc_macro2::TokenStream> {
        let base_ty = self.bound_type(field_ty);
        let setter_name = format_ident!("set_{}", field_ident);
        let getter_mut_name = format_ident!("{}_mut", field_ident);

        match (&self.base, self.optional) {
            (BaseKind::Int, false) => {
                let expr = parse_integer_expr(base_ty, bytes_range(offset, self.slot_len()), true)?;
                let write = write_integer_expr(
                    base_ty,
                    quote!(value),
                    bytes_range_mut(offset, self.slot_len()),
                    true,
                )?;
                Ok(quote! {
                    pub fn #field_ident(&self) -> #base_ty {
                        #expr
                    }

                    pub fn #setter_name(
                        &mut self,
                        value: #base_ty,
                    ) -> ::core::result::Result<(), ::pinocchio::error::ProgramError> {
                        #write
                        ::core::result::Result::Ok(())
                    }
                })
            }
            (BaseKind::Pod, false) => {
                let start = syn::LitInt::new(&offset.to_string(), Span::call_site());
                let end = syn::LitInt::new(
                    &(offset + fixed_pod_size(base_ty)?).to_string(),
                    Span::call_site(),
                );
                Ok(quote! {
                    pub fn #field_ident(&self) -> &#base_ty {
                        ::bytemuck::from_bytes::<#base_ty>(&self.bytes[#start..#end])
                    }

                    pub fn #getter_mut_name(&mut self) -> &mut #base_ty {
                        ::bytemuck::from_bytes_mut::<#base_ty>(&mut self.bytes[#start..#end])
                    }

                    pub fn #setter_name(
                        &mut self,
                        value: #base_ty,
                    ) -> ::core::result::Result<(), ::pinocchio::error::ProgramError> {
                        *::bytemuck::from_bytes_mut::<#base_ty>(&mut self.bytes[#start..#end]) = value;
                        ::core::result::Result::Ok(())
                    }
                })
            }
            (BaseKind::String { max_len, len_width }, false) => Ok(view_mut_string_method(
                field_ident,
                &setter_name,
                &getter_mut_name,
                offset,
                *max_len,
                *len_width,
            )),
            (BaseKind::Int, true) => {
                let tag = syn::LitInt::new(&offset.to_string(), Span::call_site());
                let start = offset + 1;
                let size = integer_size(base_ty)?;
                let expr = parse_integer_expr(base_ty, bytes_range(start, size), true)?;
                let write =
                    write_integer_expr(base_ty, quote!(value), bytes_range_mut(start, size), true)?;
                Ok(quote! {
                    pub fn #field_ident(&self) -> ::core::option::Option<#base_ty> {
                        if self.bytes[#tag] == 0 {
                            ::core::option::Option::None
                        } else {
                            ::core::option::Option::Some(#expr)
                        }
                    }

                    pub fn #setter_name(
                        &mut self,
                        value: #base_ty,
                    ) -> ::core::result::Result<(), ::pinocchio::error::ProgramError> {
                        if self.bytes[#tag] == 0 {
                            return ::core::result::Result::Err(
                                ::pinocchio::error::ProgramError::InvalidInstructionData,
                            );
                        }
                        #write
                        ::core::result::Result::Ok(())
                    }
                })
            }
            (BaseKind::Pod, true) => {
                let tag = syn::LitInt::new(&offset.to_string(), Span::call_site());
                let start = syn::LitInt::new(&(offset + 1).to_string(), Span::call_site());
                let end = syn::LitInt::new(
                    &(offset + 1 + fixed_pod_size(base_ty)?).to_string(),
                    Span::call_site(),
                );
                Ok(quote! {
                    pub fn #field_ident(&self) -> ::core::option::Option<&#base_ty> {
                        if self.bytes[#tag] == 0 {
                            ::core::option::Option::None
                        } else {
                            ::core::option::Option::Some(
                                ::bytemuck::from_bytes::<#base_ty>(&self.bytes[#start..#end]),
                            )
                        }
                    }

                    pub fn #getter_mut_name(&mut self) -> ::core::option::Option<&mut #base_ty> {
                        if self.bytes[#tag] == 0 {
                            ::core::option::Option::None
                        } else {
                            ::core::option::Option::Some(
                                ::bytemuck::from_bytes_mut::<#base_ty>(&mut self.bytes[#start..#end]),
                            )
                        }
                    }

                    pub fn #setter_name(
                        &mut self,
                        value: #base_ty,
                    ) -> ::core::result::Result<(), ::pinocchio::error::ProgramError> {
                        if self.bytes[#tag] == 0 {
                            return ::core::result::Result::Err(
                                ::pinocchio::error::ProgramError::InvalidInstructionData,
                            );
                        }
                        *::bytemuck::from_bytes_mut::<#base_ty>(&mut self.bytes[#start..#end]) = value;
                        ::core::result::Result::Ok(())
                    }
                })
            }
            (BaseKind::String { max_len, len_width }, true) => Ok(view_mut_optional_string_method(
                field_ident,
                &setter_name,
                &getter_mut_name,
                offset,
                *max_len,
                *len_width,
            )),
        }
    }
}

fn field_layout(field: &syn::Field) -> syn::Result<FieldLayout> {
    let optional = option_inner(&field.ty).is_some();
    let base_ty = if optional {
        option_inner(&field.ty).unwrap()
    } else {
        &field.ty
    };

    let (base, fixed_size) = if integer_primitive_name(base_ty).is_some() {
        (BaseKind::Int, Some(integer_size(base_ty)?))
    } else if is_string(base_ty) {
        let max_len = parse_max_len(&field.attrs)?.ok_or_else(|| {
            syn::Error::new_spanned(field, "variable-size fields require #[max_len = N]")
        })?;
        (
            BaseKind::String {
                max_len,
                len_width: len_width(max_len, field.span())?,
            },
            None,
        )
    } else {
        (BaseKind::Pod, Some(fixed_array_size(base_ty)?))
    };

    Ok(FieldLayout {
        optional,
        base,
        fixed_size,
    })
}

fn fixed_pod_size(ty: &Type) -> syn::Result<usize> {
    if let Some(_) = integer_primitive_name(ty) {
        return integer_size(ty);
    }
    fixed_array_size(ty)
}

fn integer_size(ty: &Type) -> syn::Result<usize> {
    match integer_primitive_name(ty).as_deref() {
        Some("i8" | "u8") => Ok(1),
        Some("i16" | "u16") => Ok(2),
        Some("i32" | "u32") => Ok(4),
        Some("i64" | "u64") => Ok(8),
        Some("i128" | "u128") => Ok(16),
        Some("isize" | "usize") => Ok(::core::mem::size_of::<usize>()),
        _ => Err(syn::Error::new_spanned(
            ty,
            "field must be an integer primitive",
        )),
    }
}

fn fixed_array_size(ty: &Type) -> syn::Result<usize> {
    let Type::Array(array) = ty else {
        return Err(syn::Error::new_spanned(
            ty,
            "fixed-size non-integer fields must be arrays",
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
    let elem_size = if integer_primitive_name(&array.elem).is_some() {
        integer_size(&array.elem)?
    } else {
        fixed_array_size(&array.elem)?
    };

    Ok(len * elem_size)
}

fn bytes_range(offset: usize, len: usize) -> proc_macro2::TokenStream {
    let start = syn::LitInt::new(&offset.to_string(), Span::call_site());
    let end = syn::LitInt::new(&(offset + len).to_string(), Span::call_site());
    quote!(&self.bytes[#start..#end])
}

fn bytes_range_mut(offset: usize, len: usize) -> proc_macro2::TokenStream {
    let start = syn::LitInt::new(&offset.to_string(), Span::call_site());
    let end = syn::LitInt::new(&(offset + len).to_string(), Span::call_site());
    quote!(&mut self.bytes[#start..#end])
}

fn parse_integer_expr(
    ty: &Type,
    bytes_expr: proc_macro2::TokenStream,
    _validated: bool,
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

fn write_integer_expr(
    ty: &Type,
    value_expr: proc_macro2::TokenStream,
    bytes_expr: proc_macro2::TokenStream,
    _validated: bool,
) -> syn::Result<proc_macro2::TokenStream> {
    let Some(name) = integer_primitive_name(ty) else {
        return Err(syn::Error::new_spanned(
            ty,
            "field must be an integer primitive",
        ));
    };
    Ok(match name.as_str() {
        "i8" | "u8" => quote! { (#bytes_expr)[0] = #value_expr as u8; },
        _ => quote! { (#bytes_expr).copy_from_slice(&#value_expr.to_le_bytes()); },
    })
}

fn read_len_expr(
    offset: usize,
    len_width: usize,
    bytes_expr: proc_macro2::TokenStream,
) -> proc_macro2::TokenStream {
    let start = syn::LitInt::new(&offset.to_string(), Span::call_site());
    match len_width {
        1 => {
            quote!((#bytes_expr)[#start] as usize)
        }
        2 | 4 => {
            let end = syn::LitInt::new(&(offset + len_width).to_string(), Span::call_site());
            let ty = if len_width == 2 {
                quote!(u16)
            } else {
                quote!(u32)
            };
            quote!({
                let raw: [u8; #len_width] = (#bytes_expr)[#start..#end].try_into().expect("validated len");
                #ty::from_le_bytes(raw) as usize
            })
        }
        _ => unreachable!(),
    }
}

fn validate_string_field(
    offset: usize,
    max_len: usize,
    len_width: usize,
) -> proc_macro2::TokenStream {
    let len_expr = read_len_expr(offset, len_width, quote!(bytes));
    let data_start = syn::LitInt::new(&(offset + len_width).to_string(), Span::call_site());
    let max_len_lit = syn::LitInt::new(&max_len.to_string(), Span::call_site());
    quote! {
        let len = #len_expr;
        if len > #max_len_lit {
            return ::core::result::Result::Err(
                ::pinocchio::error::ProgramError::InvalidInstructionData,
            );
        }
        ::core::str::from_utf8(&bytes[#data_start..#data_start + len])
            .map_err(|_| ::pinocchio::error::ProgramError::InvalidInstructionData)?;
    }
}

fn view_string_method(
    field_ident: &Ident,
    offset: usize,
    _max_len: usize,
    len_width: usize,
) -> proc_macro2::TokenStream {
    let len_expr = read_len_expr(offset, len_width, quote!(self.bytes));
    let data_start = syn::LitInt::new(&(offset + len_width).to_string(), Span::call_site());
    quote! {
        pub fn #field_ident(&self) -> &str {
            let len = #len_expr;
            ::core::str::from_utf8(&self.bytes[#data_start..#data_start + len])
                .expect("validated utf8")
        }
    }
}

fn view_optional_string_method(
    field_ident: &Ident,
    offset: usize,
    _max_len: usize,
    len_width: usize,
) -> proc_macro2::TokenStream {
    let tag = syn::LitInt::new(&offset.to_string(), Span::call_site());
    let len_expr = read_len_expr(offset + 1, len_width, quote!(self.bytes));
    let data_start = syn::LitInt::new(&(offset + 1 + len_width).to_string(), Span::call_site());
    quote! {
        pub fn #field_ident(&self) -> ::core::option::Option<&str> {
            if self.bytes[#tag] == 0 {
                ::core::option::Option::None
            } else {
                let len = #len_expr;
                ::core::option::Option::Some(
                    ::core::str::from_utf8(&self.bytes[#data_start..#data_start + len])
                        .expect("validated utf8"),
                )
            }
        }
    }
}

fn view_mut_string_method(
    field_ident: &Ident,
    setter_name: &Ident,
    getter_mut_name: &Ident,
    offset: usize,
    _max_len: usize,
    len_width: usize,
) -> proc_macro2::TokenStream {
    let len_expr = read_len_expr(offset, len_width, quote!(self.bytes));
    let data_start = syn::LitInt::new(&(offset + len_width).to_string(), Span::call_site());
    quote! {
        pub fn #field_ident(&self) -> &str {
            let len = #len_expr;
            ::core::str::from_utf8(&self.bytes[#data_start..#data_start + len])
                .expect("validated utf8")
        }

        pub fn #getter_mut_name(&mut self) -> &mut str {
            let len = #len_expr;
            ::core::str::from_utf8_mut(&mut self.bytes[#data_start..#data_start + len])
                .expect("validated utf8")
        }

        pub fn #setter_name(
            &mut self,
            value: &str,
        ) -> ::core::result::Result<(), ::pinocchio::error::ProgramError> {
            let len = #len_expr;
            if value.len() != len {
                return ::core::result::Result::Err(
                    ::pinocchio::error::ProgramError::InvalidInstructionData,
                );
            }
            self.bytes[#data_start..#data_start + len].copy_from_slice(value.as_bytes());
            ::core::result::Result::Ok(())
        }
    }
}

fn view_mut_optional_string_method(
    field_ident: &Ident,
    setter_name: &Ident,
    getter_mut_name: &Ident,
    offset: usize,
    _max_len: usize,
    len_width: usize,
) -> proc_macro2::TokenStream {
    let tag = syn::LitInt::new(&offset.to_string(), Span::call_site());
    let len_expr = read_len_expr(offset + 1, len_width, quote!(self.bytes));
    let data_start = syn::LitInt::new(&(offset + 1 + len_width).to_string(), Span::call_site());
    quote! {
        pub fn #field_ident(&self) -> ::core::option::Option<&str> {
            if self.bytes[#tag] == 0 {
                ::core::option::Option::None
            } else {
                let len = #len_expr;
                ::core::option::Option::Some(
                    ::core::str::from_utf8(&self.bytes[#data_start..#data_start + len])
                        .expect("validated utf8"),
                )
            }
        }

        pub fn #getter_mut_name(&mut self) -> ::core::option::Option<&mut str> {
            if self.bytes[#tag] == 0 {
                ::core::option::Option::None
            } else {
                let len = #len_expr;
                ::core::option::Option::Some(
                    ::core::str::from_utf8_mut(&mut self.bytes[#data_start..#data_start + len])
                        .expect("validated utf8"),
                )
            }
        }

        pub fn #setter_name(
            &mut self,
            value: &str,
        ) -> ::core::result::Result<(), ::pinocchio::error::ProgramError> {
            if self.bytes[#tag] == 0 {
                return ::core::result::Result::Err(
                    ::pinocchio::error::ProgramError::InvalidInstructionData,
                );
            }
            let len = #len_expr;
            if value.len() != len {
                return ::core::result::Result::Err(
                    ::pinocchio::error::ProgramError::InvalidInstructionData,
                );
            }
            self.bytes[#data_start..#data_start + len].copy_from_slice(value.as_bytes());
            ::core::result::Result::Ok(())
        }
    }
}
