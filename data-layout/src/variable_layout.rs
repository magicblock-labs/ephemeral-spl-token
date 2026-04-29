use std::{
    collections::{BTreeMap, BTreeSet},
    fmt::Debug,
};

use proc_macro2::Span;
use quote::{format_ident, quote, ToTokens};
use syn::{
    parse::Parser, punctuated::Punctuated, spanned::Spanned, Attribute, Expr, ExprLit, Fields,
    GenericArgument, Ident, ItemStruct, Lit, LitInt, Meta, PathArguments, Token, Type, TypePath,
};

const FIELD_ATTRIBUTES: &[&str] = &["capacity", "flexible"];

// [capacity = flexible] array-len is always encoded as 2-bytes
const MAX_LEN_WIDTH: usize = 2;

pub(crate) fn expand_variable_offset_layout(
    attr: &str,
    input: &ItemStruct,
) -> syn::Result<proc_macro2::TokenStream> {
    let args = parse_args(attr)?;
    let mut emitted_input = input.clone();
    emitted_input
        .attrs
        .retain(|attr| !attr.path().is_ident("variable_offset_layout"));

    ensure_allow_dead_code(&mut emitted_input.attrs);

    let struct_name = &emitted_input.ident;
    let view_name = format_ident!("{}View", struct_name);
    let buffer_offset_lit = usize_lit(args.buffer_offset);

    let Fields::Named(fields) = &mut emitted_input.fields else {
        return Err(syn::Error::new_spanned(
            &emitted_input.fields,
            "variable_offset_layout requires named fields",
        ));
    };

    let mut offset = 0usize;

    let mut where_bounds = Vec::<proc_macro2::TokenStream>::new();
    let mut view_methods = Vec::new();
    let mut validate_steps = Vec::new();
    let mut encoded_len_steps = Vec::new();
    let mut encode_steps = Vec::new();

    let mut min_datalen = 0;
    let mut max_datalen = 0;
    let mut min_datalen_expr = quote!();
    let mut max_datalen_expr = quote!();
    let mut update_maxmin_datalen =
        |(slot_min_len_expr, slot_min_len), (slot_max_len_expr, slot_max_len)| {
            min_datalen += slot_min_len;
            max_datalen += slot_max_len;
            if min_datalen_expr.is_empty() {
                min_datalen_expr = slot_min_len_expr;
                max_datalen_expr = slot_max_len_expr;
            } else {
                min_datalen_expr = quote!(#min_datalen_expr + #slot_min_len_expr);
                max_datalen_expr = quote!(#max_datalen_expr + #slot_max_len_expr);
            }
        };

    let mut field_offsets = vec![];
    let mut seen_variable_sized_field = false;

    let mut implicit_option_index = 0usize;
    let field_layouts = fields
        .named
        .iter()
        .map(|field| parse_field_layout(field, args.option_encoding, &mut implicit_option_index))
        .collect::<syn::Result<Vec<_>>>()?;

    validate_struct_options(struct_name, &fields.named, &field_layouts, &args)?;
    validate_borrowed_field_alignment(struct_name, &fields.named, &field_layouts, &args)?;

    for (index, (field, field_layout)) in fields
        .named
        .iter_mut()
        .zip(field_layouts.iter())
        .enumerate()
    {
        let field_ident = field.ident.as_ref().expect("named field");

        strip_field_attr(&mut field.attrs);

        if seen_variable_sized_field {
            field_offsets.push(FieldOffset::VariableOffset {
                layout: field_layout.clone(),
            });
        } else {
            field_offsets.push(FieldOffset::FixedOffset {
                offset,
                layout: field_layout.clone(),
            });
        }

        match field_layout.slot_minmax_len() {
            Ok((slot_len_expr, slot_len)) => {
                offset += slot_len;
                update_maxmin_datalen((slot_len_expr.clone(), slot_len), (slot_len_expr, slot_len));
            }
            Err(((slot_min_len_expr, slot_min_len), (slot_max_len_expr, slot_max_len))) => {
                seen_variable_sized_field = true;
                update_maxmin_datalen(
                    (slot_min_len_expr, slot_min_len),
                    (slot_max_len_expr, slot_max_len),
                );
            }
        }

        validate_steps.push(field_layout.gen_validate_step(struct_name, field_ident));
        encoded_len_steps.push(field_layout.gen_encoded_len_step(field_ident));
        encode_steps.push(field_layout.gen_field_encode_step(field_ident));
        view_methods.push(field_layout.gen_view_methods(
            struct_name,
            field_ident,
            &field_offsets[..=index],
        )?);

        if let Some(bound) = field_layout.bound() {
            where_bounds.push(bound);
        }
    }

    let where_clause = impl_where_clause(&where_bounds);

    let implicit_len_helpers = implicit_len_helpers(min_datalen, &fields.named, &field_layouts)?;
    let implicit_len_validation =
        implicit_len_validation(struct_name, min_datalen, &fields.named, &field_layouts)?;

    let public_len_const = public_len_const(
        min_datalen_expr.clone(),
        min_datalen,
        max_datalen_expr.clone(),
        max_datalen,
        &field_layouts,
    )?;
    let data_len_validation =
        data_len_validation(struct_name, min_datalen, max_datalen, &field_layouts)?;

    Ok(quote! {
        #emitted_input

        impl #struct_name {
            const __MIN_DATA_LEN: usize = #min_datalen_expr;
            const __MAX_DATA_LEN: usize = #max_datalen_expr;

            #public_len_const

            pub fn decode(
                bytes: &[u8],
            ) -> core::result::Result<#view_name<'_>, pinocchio::error::ProgramError> {
                Self::__validate_bytes(bytes)?;
                Ok(#view_name { bytes })
            }

            pub fn encode_to(
                &self,
                bytes: &mut [u8],
            ) -> core::result::Result<(), pinocchio::error::ProgramError> {
                let encoded_len = self.__encoded_len()?;
                if bytes.len() < encoded_len {
                    pinocchio_log::log!(
                        "bytes [len={}] are too small to encode {} which needs {} bytes",
                        bytes.len(),
                        stringify!(#struct_name),
                        encoded_len,
                    );
                    return Err(pinocchio::error::ProgramError::AccountDataTooSmall);
                }

                let mut offset = 0usize;
                #(#encode_steps)*
                Ok(())
            }

            pub fn encode(&self) -> core::result::Result<Vec<u8>, pinocchio::error::ProgramError> {
                let mut bytes = vec![0; self.__encoded_len()?];
                self.encode_to(&mut bytes)?;
                Ok(bytes)
            }

            fn __validate_bytes(
                bytes: &[u8],
            ) -> core::result::Result<(), pinocchio::error::ProgramError> {
                if (bytes.as_ptr() as usize) % 8 != #buffer_offset_lit {
                    pinocchio_log::log!(
                        "bytes [ptr_mod_8={}] cannot be deserialized to {} which requires buffer_offset = {} from an 8-byte aligned base",
                        (bytes.as_ptr() as usize) % 8,
                        stringify!(#struct_name),
                        #buffer_offset_lit,
                    );
                    return Err(
                        pinocchio::error::ProgramError::InvalidInstructionData,
                    );
                }

                #data_len_validation
                #implicit_len_validation

                let mut offset = 0usize;
                #(#validate_steps)*

                Ok(())
            }

            fn __encoded_len(
                &self,
            ) -> core::result::Result<usize, pinocchio::error::ProgramError> {
                let mut len = 0usize;
                #(#encoded_len_steps)*
                Ok(len)
            }

            #implicit_len_helpers
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

#[derive(Clone)]
enum FieldOffset {
    // A field gets this value only if there is no variable-len field before it.
    // Once a variable-len field is seen, all other fields will be VariableOffset.
    FixedOffset { offset: usize, layout: FieldLayout },

    // Note that it depends on the order of fields, e.g
    // if a u64 appears after Vec<u8> (or Option<u16>) field, then it will
    // be treated as VariableOffset as the offset for this field is fixed anymore.
    VariableOffset { layout: FieldLayout },
}

impl FieldOffset {
    fn fixed_offset(&self) -> Option<usize> {
        let FieldOffset::FixedOffset { offset, .. } = self else {
            return None;
        };
        return Some(*offset);
    }

    fn layout(&self) -> &FieldLayout {
        match self {
            FieldOffset::FixedOffset { layout, .. } => layout,
            FieldOffset::VariableOffset { layout } => layout,
        }
    }
}

impl Debug for FieldOffset {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            FieldOffset::FixedOffset { offset, .. } => {
                write!(f, "FieldOffset {{offset: {} }}", offset)
            }
            FieldOffset::VariableOffset { .. } => write!(f, "VariableOffset"),
        }
    }
}

enum AccessMode {
    Copy,
    Ref,
}

#[derive(Clone)]
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

#[derive(Clone)]
enum FieldLayout {
    Value {
        value: FixedValueKind,
        optional: OptionalKind,
    },
    Vec {
        elem: FixedValueKind,
        flexible: Flexible,
    },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum OptionalKind {
    No,
    Tagged,
    Implicit(usize),
}

impl FieldLayout {
    fn is_fixed(&self) -> bool {
        match self {
            FieldLayout::Value { optional, .. } => *optional == OptionalKind::No,
            FieldLayout::Vec { .. } => false,
        }
    }

    fn slot_minmax_len(
        &self,
    ) -> Result<
        // fixed size
        (proc_macro2::TokenStream, usize),
        // variable size (min and max)
        (
            (proc_macro2::TokenStream, usize), // min
            (proc_macro2::TokenStream, usize), // max
        ),
    > {
        match self {
            Self::Value { value, optional } => {
                let ty = value.ty();
                match optional {
                    OptionalKind::No => Ok((quote!(core::mem::size_of::<#ty>()), value.size())),
                    OptionalKind::Tagged => Err((
                        (quote!(1), 1),
                        (quote!((1 + core::mem::size_of::<#ty>())), 1 + value.size()),
                    )),
                    OptionalKind::Implicit(_) => Err((
                        (quote!(0), 0),
                        (quote!(core::mem::size_of::<#ty>()), value.size()),
                    )),
                }
            }
            Self::Vec { elem, flexible } => {
                let elem_ty = elem.ty();
                let len_width_lit = usize_lit(flexible.len_width);
                let capacity_lit = usize_lit(flexible.capacity());
                Err((
                    (quote!(#len_width_lit), flexible.len_width),
                    (
                        quote!((#len_width_lit + core::mem::size_of::<#elem_ty>() * #capacity_lit)),
                        flexible.len_width + elem.size() * flexible.capacity(),
                    ),
                ))
            }
        }
    }

    fn gen_encoded_len_step(&self, field_ident: &Ident) -> proc_macro2::TokenStream {
        match self {
            Self::Value { value, optional } => {
                let value_size = usize_lit(value.size());
                match optional {
                    OptionalKind::No => quote! {
                        len += #value_size;
                    },
                    OptionalKind::Tagged => quote! {
                        len += if self.#field_ident.is_some() { 1 + #value_size } else { 1 };
                    },
                    OptionalKind::Implicit(_) => quote! {
                        len += if self.#field_ident.is_some() { #value_size } else { 0 };
                    },
                }
            }
            Self::Vec { elem, flexible } => {
                let elem_size = usize_lit(elem.size());
                let len_width = usize_lit(flexible.len_width);
                let capacity = usize_lit(flexible.capacity());
                quote! {
                    let field_len = self.#field_ident.len();
                    if field_len > #capacity {
                        pinocchio_log::log!(
                            "Cannot encode field {}: len {} exceeds max {}",
                            stringify!(#field_ident),
                            field_len,
                            #capacity,
                        );
                        return Err(pinocchio::error::ProgramError::InvalidRealloc);
                    }
                    len += #len_width + field_len * #elem_size;
                }
            }
        }
    }

    fn gen_validate_step(
        &self,
        struct_name: &Ident,
        field_ident: &Ident,
    ) -> proc_macro2::TokenStream {
        let field_name = field_ident.to_string();

        match self {
            Self::Value { value, optional } => {
                let value_size = usize_lit(value.size());
                let value_align = usize_lit(value.align());
                let alignment_check = if matches!(value.access_mode(), AccessMode::Ref) {
                    quote! {
                        if data_offset % #value_align != 0 {
                            pinocchio_log::log!(
                                "Invalid alignment for field {}: payload starts at offset {}, expected {}-byte alignment",
                                #field_name,
                                data_offset,
                                #value_align,
                            );
                            return Err(pinocchio::error::ProgramError::InvalidInstructionData);
                        }
                    }
                } else {
                    quote!()
                };

                match optional {
                    OptionalKind::No => quote! {
                        let data_offset = offset;
                        #alignment_check
                        let end = data_offset + #value_size;
                        if end > bytes.len() {
                            pinocchio_log::log!(
                                "Truncated payload for field {}: need {} bytes, have {}",
                                #field_name,
                                end,
                                bytes.len(),
                            );
                            return Err(pinocchio::error::ProgramError::InvalidInstructionData);
                        }
                        offset = end;
                    },
                    OptionalKind::Tagged => quote! {
                        if offset >= bytes.len() {
                            pinocchio_log::log!(
                                "Missing Option tag for field {} at offset {}",
                                #field_name,
                                offset,
                            );
                            return Err(pinocchio::error::ProgramError::InvalidInstructionData);
                        }

                        match bytes[offset] {
                            0 => {
                                offset += 1;
                            }
                            1 => {
                                let data_offset = offset + 1;
                                #alignment_check
                                let end = data_offset + #value_size;
                                if end > bytes.len() {
                                    pinocchio_log::log!(
                                        "Truncated payload for field {}: need {} bytes, have {}",
                                        #field_name,
                                        end,
                                        bytes.len(),
                                    );
                                    return Err(pinocchio::error::ProgramError::InvalidInstructionData);
                                }
                                offset = end;
                            }
                            tag => {
                                pinocchio_log::log!(
                                    "Invalid Option tag for field {}: tag = {}",
                                    #field_name,
                                    tag,
                                );
                                return Err(pinocchio::error::ProgramError::InvalidInstructionData);
                            }
                        }
                    },
                    OptionalKind::Implicit(bit_index) => {
                        let bit_index = usize_lit(*bit_index);
                        quote! {
                            if #struct_name::__implicit_option_present_for_len(bytes.len(), #bit_index) {
                                let data_offset = offset;
                                #alignment_check
                                let end = data_offset + #value_size;
                                if end > bytes.len() {
                                    pinocchio_log::log!(
                                        "Truncated payload for field {}: need {} bytes, have {}",
                                        #field_name,
                                        end,
                                        bytes.len(),
                                    );
                                    return Err(pinocchio::error::ProgramError::InvalidInstructionData);
                                }
                                offset = end;
                            }
                        }
                    }
                }
            }
            Self::Vec { elem, flexible } => {
                let elem_size = usize_lit(elem.size());
                let elem_align = usize_lit(elem.align());
                let len_width_lit = usize_lit(flexible.len_width);
                let len_expr = match flexible.len_width {
                    1 => quote!(bytes[offset] as usize),
                    2 => quote!({
                        let raw: [u8; 2] = bytes[offset..offset + 2]
                            .try_into()
                            .expect("validated len header");
                        u16::from_le_bytes(raw) as usize
                    }),
                    _ => unreachable!("read_len_expr: len_width more than 2 isn't supported"),
                };
                let alignment_check = if elem.align() > 1 {
                    quote! {
                        if len != 0 && data_offset % #elem_align != 0 {
                            pinocchio_log::log!(
                                "Invalid alignment for field {}: element data starts at offset {}, expected {}-byte alignment",
                                #field_name,
                                data_offset,
                                #elem_align,
                            );
                            return Err(pinocchio::error::ProgramError::InvalidInstructionData);
                        }
                    }
                } else {
                    quote!()
                };

                quote! {
                    let data_offset = offset + #len_width_lit;
                    if data_offset > bytes.len() {
                        pinocchio_log::log!(
                            "Missing length header for field {} at offset {}",
                            #field_name,
                            offset,
                        );
                        return Err(pinocchio::error::ProgramError::InvalidInstructionData);
                    }

                    let len = #len_expr;
                    #alignment_check

                    let end = data_offset + len * #elem_size;
                    if end > bytes.len() {
                        pinocchio_log::log!(
                            "Truncated Vec payload for field {}: need {} bytes, have {}",
                            #field_name,
                            end,
                            bytes.len(),
                        );
                        return Err(pinocchio::error::ProgramError::InvalidInstructionData);
                    }

                    offset = end;
                }
            }
        }
    }

    fn gen_field_encode_step(&self, field_ident: &Ident) -> proc_macro2::TokenStream {
        match self {
            Self::Value { value, optional } => {
                let value_size = usize_lit(value.size());
                match optional {
                    OptionalKind::No => quote! {
                        bytes[offset..offset + #value_size]
                            .copy_from_slice(bytemuck::bytes_of(&self.#field_ident));
                        offset += #value_size;
                    },
                    OptionalKind::Tagged => quote! {
                        if let core::option::Option::Some(value) = &self.#field_ident {
                            bytes[offset] = 1;
                            bytes[offset + 1..offset + 1 + #value_size]
                                .copy_from_slice(bytemuck::bytes_of(value));
                            offset += 1 + #value_size;
                        } else {
                            bytes[offset] = 0;
                            offset += 1;
                        }
                    },
                    OptionalKind::Implicit(_) => quote! {
                        if let core::option::Option::Some(value) = &self.#field_ident {
                            bytes[offset..offset + #value_size]
                                .copy_from_slice(bytemuck::bytes_of(value));
                            offset += #value_size;
                        }
                    },
                }
            }
            Self::Vec { elem, flexible } => {
                let elem_size = usize_lit(elem.size());
                let len_width = usize_lit(flexible.len_width);
                let len_width_ty = flexible.len_width_ty();
                quote! {
                    let field_len = self.#field_ident.len();
                    bytes[offset..offset + #len_width]
                        .copy_from_slice(bytemuck::bytes_of(&(field_len as #len_width_ty)));
                    let start = offset + #len_width;
                    let end = start + field_len * #elem_size;
                    if field_len != 0 {
                        bytes[start..end]
                            .copy_from_slice(bytemuck::cast_slice(self.#field_ident.as_slice()));
                    }
                    offset = end;
                }
            }
        }
    }

    fn bound(&self) -> Option<proc_macro2::TokenStream> {
        match self {
            Self::Value { value, .. } => {
                if value.needs_pod_bound() {
                    let ty = value.ty();
                    Some(quote!(#ty: bytemuck::Pod))
                } else {
                    None
                }
            }
            Self::Vec { elem, .. } => {
                if elem.needs_pod_bound() {
                    let ty = elem.ty();
                    Some(quote!(#ty: bytemuck::Pod))
                } else {
                    None
                }
            }
        }
    }

    // fn check_ref_alignment(
    //     &self,
    //     offset: usize,
    //     field_ident: &Ident,
    // ) -> syn::Result<Option<PaddingIssue>> {
    //     match self {
    //         Self::Value { value, optional } => {
    //             if matches!(value.access_mode(), AccessMode::Copy) {
    //                 return Ok(None);
    //             }

    //             let align = value.align();
    //             if align > 8 {
    //                 return Err(syn::Error::new(
    //                     field_ident.span(),
    //                     format!(
    //                         "field `{}` cannot be borrowed by variable_offset_layout: size is {} byte(s) but alignment is {} byte(s), and fixed_layout only assumes the input buffer is 8-byte aligned",
    //                         field_ident,
    //                         value.size(),
    //                         align,
    //                     ),
    //                 ));
    //             }

    //             let payload_offset = offset + usize::from(optional.is_some());
    //             let misalignment = payload_offset % align;
    //             if misalignment == 0 {
    //                 return Ok(None);
    //             }

    //             let padding = align - misalignment;
    //             let message = if optional.is_some() {
    //                 format!(
    //                     "field `{}` needs {} byte(s) of padding before it: its Option payload would start at offset {}, but borrowed values of this field must be {}-byte aligned. Insert `_pad: [u8; {}]` before `{}` so the payload starts at offset {}",
    //                     field_ident,
    //                     padding,
    //                     payload_offset,
    //                     align,
    //                     padding,
    //                     field_ident,
    //                     payload_offset + padding,
    //                 )
    //             } else {
    //                 format!(
    //                     "field `{}` needs {} byte(s) of padding before it: it would start at offset {}, but borrowed values of this field must be {}-byte aligned. Insert `_pad: [u8; {}]` before `{}` so it starts at offset {}",
    //                     field_ident,
    //                     padding,
    //                     offset,
    //                     align,
    //                     padding,
    //                     field_ident,
    //                     offset + padding,
    //                 )
    //             };
    //             Ok(Some(PaddingIssue {
    //                 padding,
    //                 error: syn::Error::new(field_ident.span(), message),
    //             }))
    //         }
    //         Self::Vec { elem, capacity } => {
    //             let align = elem.align();
    //             if align > 8 {
    //                 return Err(syn::Error::new(
    //                     field_ident.span(),
    //                     format!(
    //                         "field `{}` cannot expose a slice view in variable_offset_layout: each Vec element is {} byte(s) but alignment is {} byte(s), and fixed_layout only assumes the input buffer is 8-byte aligned, so it cannot support type which requires alignment greater than 8",
    //                         field_ident,
    //                         elem.size(),
    //                         align,
    //                     ),
    //                 ));
    //             }

    //             let len_width = capacity.len_width();
    //             let first_elem_offset = offset + len_width;
    //             let misalignment = first_elem_offset % align;
    //             if misalignment == 0 {
    //                 return Ok(None);
    //             }

    //             let padding = align - misalignment;
    //             Ok(Some(PaddingIssue {
    //                 padding,
    //                 error: syn::Error::new(
    //                     field_ident.span(),
    //                     format!(
    //                     "field `{}` needs {} byte(s) of padding before it: its Vec elements start after a {}-byte length prefix, so element 0 would start at offset {}, but slice views require {}-byte alignment. Insert `_pad: [u8; {}]` before `{}` so element 0 starts at offset {}",
    //                     field_ident,
    //                     padding,
    //                     len_width,
    //                     first_elem_offset,
    //                     align,
    //                     padding,
    //                     field_ident,
    //                     first_elem_offset + padding,
    //                     ),
    //                 ),
    //             }))
    //         }
    //     }
    // }

    // fn gen_validate_vec_len(&self, offset: usize, field_ident: &Ident) -> proc_macro2::TokenStream {
    //     let field_name = field_ident.to_string();
    //     let offset_lit = usize_lit(offset);
    //     let offset_expr = quote!(#offset_lit);
    //     match self {
    //         Self::Value { optional, .. } => {
    //             if optional.is_some() {
    //                 quote! {
    //                     Self::__validate_option(bytes, #offset_expr, #field_name)?;
    //                 }
    //             } else {
    //                 quote! {}
    //             }
    //         }
    //         Self::Vec { capacity, .. } => {
    //             let capacity_lit = capacity
    //                 .comptime_capacity_lit()
    //                 .unwrap_or(usize_lit(MAX_CAPACITY));
    //             let len_width_lit = capacity.len_width_lit();
    //             let expect_msg = format!(
    //                 "validate encoded-len [len_width={}] for field '{}'",
    //                 capacity.len_width(),
    //                 field_name
    //             );
    //             quote! {
    //                 Self::__validate_vec_len(bytes, #offset_expr, #capacity_lit, #len_width_lit, #field_name, #expect_msg)?;
    //             }
    //         }
    //     }
    // }

    // fn gen_field_encode(
    //     &self,
    //     fields_encode_expr: proc_macro2::TokenStream,
    //     offset: usize,
    //     field_ident: &Ident,
    // ) -> proc_macro2::TokenStream {
    //     let offset = usize_lit(offset);
    //     match self {
    //         Self::Value { value, optional } => {
    //             let len = usize_lit(value.size());
    //             match optional {
    //                 Some(Optional::Fixed) => {
    //                     quote! {
    //                         #fields_encode_expr

    //                         if let Some(value) = &self.#field_ident {
    //                             bytes[#offset] = 1;
    //                             bytes[#offset + 1 .. #offset + 1 + #len].copy_from_slice(bytemuck::bytes_of(value));
    //                         } else {
    //                             bytes[#offset] = 0;
    //                             bytes[#offset + 1 .. #offset + 1 + #len].fill(0);
    //                         }
    //                     }
    //                 }
    //                 Some(Optional::Flexible) => {
    //                     quote! {
    //                         #fields_encode_expr

    //                         if let Some(value) = &self.#field_ident {
    //                             bytes[#offset] = 1;
    //                             bytes[#offset + 1 .. #offset + 1 + #len].copy_from_slice(bytemuck::bytes_of(value));
    //                         }
    //                     }
    //                 }
    //                 None => {
    //                     quote! {
    //                         #fields_encode_expr

    //                         bytes[#offset..#offset + #len].copy_from_slice(bytemuck::bytes_of(&self.#field_ident));
    //                     }
    //                 }
    //             }
    //         }
    //         Self::Vec { elem, capacity } => {
    //             let elem_size = usize_lit(elem.size());
    //             let len_width_ty = capacity.len_width_ty();
    //             let len_width = capacity.len_width_lit();
    //             if let Some(cap) = capacity.comptime_capacity_lit() {
    //                 quote! {
    //                     #fields_encode_expr

    //                     if self.#field_ident.len() > #cap {
    //                         return Err(pinocchio::error::ProgramError::InvalidRealloc);
    //                     }

    //                     bytes[#offset..#offset + #len_width].copy_from_slice(bytemuck::bytes_of(&(self.#field_ident.len() as #len_width_ty)));
    //                     bytes[#offset + #len_width..#offset + #len_width + self.#field_ident.len() * #elem_size].copy_from_slice(bytemuck::cast_slice(&self.#field_ident.as_slice()));
    //                     if self.#field_ident.len() < #cap {
    //                          bytes[#offset + #len_width + self.#field_ident.len() * #elem_size..#offset + #len_width + #cap * #elem_size].fill(0);
    //                     }
    //                 }
    //             } else {
    //                 // it must be the last field of Vec type with #[capacity = flexible]
    //                 let max_capacity = usize_lit(MAX_CAPACITY);
    //                 quote! {
    //                     #fields_encode_expr

    //                     if self.#field_ident.len() > #max_capacity {
    //                         return Err(pinocchio::error::ProgramError::InvalidRealloc);
    //                     } else if !self.#field_ident.is_empty() {
    //                         bytes[#offset..#offset + #len_width].copy_from_slice(bytemuck::bytes_of(&(self.#field_ident.len() as #len_width_ty)));
    //                         bytes[#offset + #len_width..#offset + #len_width + self.#field_ident.len() * #elem_size].copy_from_slice(bytemuck::cast_slice(&self.#field_ident.as_slice()));
    //                     } else {
    //                         // Note that it is an empty-vector scenario in which case we do not have any 'buffer' to write anything (even zeroes) to
    //                     }
    //                 }
    //             }
    //         }
    //     }
    // }

    fn gen_view_methods(
        &self,
        struct_name: &Ident,
        field_ident: &Ident,
        field_offsets: &[FieldOffset],
    ) -> syn::Result<proc_macro2::TokenStream> {
        match self {
            Self::Value { value, optional } => {
                let ty = value.ty();
                let value_size = usize_lit(value.size());
                let access_mode = value.access_mode();
                let offset_expr = find_data_offset(struct_name, field_offsets);
                let data_offset_expr = match optional {
                    OptionalKind::No | OptionalKind::Implicit(_) => quote!(offset),
                    OptionalKind::Tagged => quote!(offset + 1),
                };
                let slice_expr =
                    quote!(&self.bytes[#data_offset_expr..#data_offset_expr+#value_size]);

                let return_expr = match access_mode {
                    AccessMode::Copy => read_copy_expr(value, slice_expr)?,
                    AccessMode::Ref => quote!(bytemuck::from_bytes::<#ty>(#slice_expr)),
                };

                match (*optional, access_mode) {
                    (OptionalKind::No, AccessMode::Copy) => Ok(quote! {
                        pub fn #field_ident(&self) -> #ty {
                            let offset = #offset_expr;
                            #return_expr
                        }
                    }),
                    (OptionalKind::No, AccessMode::Ref) => Ok(quote! {
                        pub fn #field_ident(&self) -> &#ty {
                            let offset = #offset_expr;
                            #return_expr
                        }
                    }),
                    (OptionalKind::Tagged, AccessMode::Copy) => Ok(quote! {
                        pub fn #field_ident(&self) -> core::option::Option<#ty> {
                            let offset = #offset_expr;
                            (self.bytes[offset] != 0).then(|| #return_expr)
                        }
                    }),
                    (OptionalKind::Tagged, AccessMode::Ref) => Ok(quote! {
                        pub fn #field_ident(&self) -> core::option::Option<&#ty> {
                            let offset = #offset_expr;
                            (self.bytes[offset] != 0).then(|| #return_expr)
                        }
                    }),
                    (OptionalKind::Implicit(bit_index), AccessMode::Copy) => {
                        let bit_index = usize_lit(bit_index);
                        Ok(quote! {
                            pub fn #field_ident(&self) -> core::option::Option<#ty> {
                                let offset = #offset_expr;
                                #struct_name::__implicit_option_present_for_len(self.bytes.len(), #bit_index)
                                    .then(|| #return_expr)
                            }
                        })
                    }
                    (OptionalKind::Implicit(bit_index), AccessMode::Ref) => {
                        let bit_index = usize_lit(bit_index);
                        Ok(quote! {
                            pub fn #field_ident(&self) -> core::option::Option<&#ty> {
                                let offset = #offset_expr;
                                #struct_name::__implicit_option_present_for_len(self.bytes.len(), #bit_index)
                                    .then(|| #return_expr)
                            }
                        })
                    }
                }
            }
            Self::Vec { elem, flexible } => {
                let elem_ty = elem.ty();
                let elem_size = usize_lit(elem.size());
                let len_width_lit = usize_lit(flexible.len_width);
                let len_expr = match flexible.len_width {
                    1 => quote!(self.bytes[offset] as usize),
                    2 => quote!({
                        let raw: [u8; 2] = self.bytes[offset..offset + 2]
                            .try_into()
                            .expect("validated len");
                        u16::from_le_bytes(raw) as usize
                    }),
                    _ => {
                        unreachable!("read_len_expr: len_width more than 2 isn't supported")
                    }
                };

                let offset_expr = find_data_offset(struct_name, field_offsets);
                Ok(quote! {
                    pub fn #field_ident(&self) -> &[#elem_ty] {
                        let offset = #offset_expr;
                        let len = #len_expr;
                        if len == 0 {
                            return &[];
                        }
                        let start = offset + #len_width_lit;
                        let end = start + len * #elem_size;
                        bytemuck::cast_slice::<u8, #elem_ty>(&self.bytes[start..end])
                    }
                })
            }
        }
    }
}

fn parse_field_layout(
    field: &syn::Field,
    option_encoding: StructOptionEncoding,
    implicit_option_index: &mut usize,
) -> syn::Result<FieldLayout> {
    let ty = &field.ty;
    let attribute = parse_field_attr(field)?;

    if let Some(elem_ty) = vec_inner(ty)? {
        let attribute = attribute.ok_or_else(|| {
            syn::Error::new_spanned(
                field,
                "Vec fields in variable_offset_layout require `#[flexible = N]`",
            )
        })?;
        let flexible = match attribute {
            FieldAttribute::Flexible(len_width) => Flexible { len_width },
        };
        return Ok(FieldLayout::Vec {
            elem: parse_value_kind(elem_ty)?,
            flexible,
        });
    }

    if attribute.is_some() {
        return Err(syn::Error::new_spanned(
            field,
            "`#[flexible = N]` is only applicable on Vec fields",
        ));
    }

    if let Some(inner) = option_inner(ty) {
        if vec_inner(inner)?.is_some() {
            return Err(syn::Error::new_spanned(
                field,
                "Option<Vec<T>> is not supported in variable_offset_layout",
            ));
        }
        if is_string(&inner) {
            return Err(syn::Error::new_spanned(
                field,
                "String is not supported by variable_offset_layout",
            ));
        }

        return Ok(FieldLayout::Value {
            value: parse_value_kind(inner)?,
            optional: match option_encoding {
                StructOptionEncoding::Tagged => OptionalKind::Tagged,
                StructOptionEncoding::Implicit => {
                    let index = *implicit_option_index;
                    *implicit_option_index += 1;
                    OptionalKind::Implicit(index)
                }
            },
        });
    }

    if attribute.is_some() {
        return Err(syn::Error::new_spanned(
            field,
            "attributes are allowed on Vec or Option field only",
        ));
    }

    if is_string(&ty) {
        return Err(syn::Error::new_spanned(
            field,
            "String is not supported by variable_offset_layout",
        ));
    }

    Ok(FieldLayout::Value {
        value: parse_value_kind(ty)?,
        optional: OptionalKind::No,
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

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct LayoutArgs {
    option_encoding: StructOptionEncoding,
    buffer_offset: usize,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum StructOptionEncoding {
    Tagged,
    Implicit,
}

fn parse_args(attr: &str) -> syn::Result<LayoutArgs> {
    if attr.trim().is_empty() {
        return Err(syn::Error::new(
            Span::call_site(),
            "variable_offset_layout requires `buffer_offset = 0..7`",
        ));
    }

    let metas = Punctuated::<Meta, Token![,]>::parse_terminated.parse_str(attr)?;
    let mut option_encoding = StructOptionEncoding::Tagged;
    let mut buffer_offset = None;

    for meta in metas {
        match meta {
            Meta::NameValue(meta) if meta.path.is_ident("option") => {
                let Expr::Path(value) = meta.value else {
                    return Err(syn::Error::new_spanned(
                        meta,
                        "option must use the form `option = implicit`",
                    ));
                };

                let Some(ident) = value.path.get_ident() else {
                    return Err(syn::Error::new_spanned(
                        value,
                        "option must use the form `option = implicit`",
                    ));
                };

                match ident.to_string().as_str() {
                    "implicit" => option_encoding = StructOptionEncoding::Implicit,
                    _ => {
                        return Err(syn::Error::new_spanned(
                            ident,
                            "option must be `implicit`",
                        ))
                    }
                }
            }
            Meta::NameValue(meta) if meta.path.is_ident("buffer_offset") => {
                let Expr::Lit(ExprLit {
                    lit: Lit::Int(lit_int),
                    ..
                }) = meta.value
                else {
                    return Err(syn::Error::new_spanned(
                        meta,
                        "buffer_offset must use the form `buffer_offset = 0..7`",
                    ));
                };

                let value: usize = lit_int.base10_parse()?;
                if value <= 7 {
                    buffer_offset = Some(value);
                } else {
                    return Err(syn::Error::new_spanned(
                        lit_int,
                        "buffer_offset must be in the range 0..=7",
                    ));
                }
            }
            _ => {
                return Err(syn::Error::new_spanned(
                    meta,
                    "variable_offset_layout only supports `option = implicit` and `buffer_offset = 0..7`",
                ))
            }
        }
    }

    let Some(buffer_offset) = buffer_offset else {
        return Err(syn::Error::new(
            Span::call_site(),
            "variable_offset_layout requires `buffer_offset = 0..7`",
        ));
    };

    Ok(LayoutArgs {
        option_encoding,
        buffer_offset,
    })
}

fn validate_struct_options(
    struct_name: &Ident,
    fields: &syn::punctuated::Punctuated<syn::Field, Token![,]>,
    layouts: &[FieldLayout],
    args: &LayoutArgs,
) -> syn::Result<()> {
    if args.option_encoding != StructOptionEncoding::Implicit {
        return Ok(());
    }

    if let Some(field) = fields
        .iter()
        .zip(layouts.iter())
        .find_map(|(field, layout)| matches!(layout, FieldLayout::Vec { .. }).then_some(field))
    {
        return Err(syn::Error::new_spanned(
            field,
            "variable_offset_layout(option = implicit) does not support Vec fields",
        ));
    }

    let implicit_option_count = layouts
        .iter()
        .filter(|layout| {
            matches!(
                layout,
                FieldLayout::Value {
                    optional: OptionalKind::Implicit(_),
                    ..
                }
            )
        })
        .count();

    if implicit_option_count == 0 {
        return Err(syn::Error::new_spanned(
            struct_name,
            "variable_offset_layout(option = implicit) requires at least one Option<T> field",
        ));
    }

    let _ = implicit_len_map(fields, layouts, 0)?;

    Ok(())
}

fn implicit_len_map(
    fields: &syn::punctuated::Punctuated<syn::Field, Token![,]>,
    layouts: &[FieldLayout],
    base_len: usize,
) -> syn::Result<Vec<(usize, u128)>> {
    let implicit_payloads = fields
        .iter()
        .zip(layouts.iter())
        .filter_map(|(field, layout)| match layout {
            FieldLayout::Value {
                value,
                optional: OptionalKind::Implicit(bit_index),
            } => Some((field, *bit_index, value.size())),
            _ => None,
        })
        .collect::<Vec<_>>();

    if implicit_payloads.is_empty() {
        return Ok(Vec::new());
    }

    if implicit_payloads.len() > 128 {
        return Err(syn::Error::new_spanned(
            fields,
            "variable_offset_layout(option = implicit) supports at most 128 Option<T> fields",
        ));
    }

    let mut sums = BTreeMap::from([(0usize, 0u128)]);

    for (field, bit_index, payload_size) in implicit_payloads {
        let existing = sums.clone();
        for (sum, mask) in existing {
            let next_sum = sum + payload_size;
            let next_mask = mask | (1u128 << bit_index);
            if sums.insert(next_sum, next_mask).is_some() {
                let field_ident = field.ident.as_ref().expect("named field");
                return Err(syn::Error::new(
                    field_ident.span(),
                    format!(
                        "variable_offset_layout(option = implicit) requires Option<T> payload sizes to have unique subset sums: field `{}` creates duplicate extra payload length {}",
                        field_ident,
                        next_sum,
                    ),
                ));
            }
        }
    }

    Ok(sums
        .into_iter()
        .map(|(extra_len, mask)| (base_len + extra_len, mask))
        .collect())
}

fn implicit_len_helpers(
    min_datalen: usize,
    fields: &syn::punctuated::Punctuated<syn::Field, Token![,]>,
    layouts: &[FieldLayout],
) -> syn::Result<proc_macro2::TokenStream> {
    let len_map = implicit_len_map(fields, layouts, min_datalen)?;
    if len_map.is_empty() {
        return Ok(quote!());
    }

    let len_arms = len_map.iter().map(|(len, mask)| {
        let len = usize_lit(*len);
        let mask = syn::LitInt::new(&format!("{}u128", mask), Span::call_site());
        quote!(#len => core::option::Option::Some(#mask),)
    });

    Ok(quote! {
        fn __implicit_option_mask_for_len(len: usize) -> core::option::Option<u128> {
            match len {
                #(#len_arms)*
                _ => core::option::Option::None,
            }
        }

        fn __implicit_option_present_for_len(len: usize, bit_index: usize) -> bool {
            let mask = Self::__implicit_option_mask_for_len(len)
                .expect("validated implicit option length");
            (mask & (1u128 << bit_index)) != 0
        }
    })
}

fn implicit_len_validation(
    struct_name: &Ident,
    min_datalen: usize,
    fields: &syn::punctuated::Punctuated<syn::Field, Token![,]>,
    layouts: &[FieldLayout],
) -> syn::Result<proc_macro2::TokenStream> {
    let len_map = implicit_len_map(fields, layouts, min_datalen)?;
    if len_map.is_empty() {
        return Ok(quote!());
    }

    let valid_lens = format!(
        "{:?}",
        len_map.iter().map(|(len, _)| *len).collect::<Vec<_>>()
    );

    Ok(quote! {
        if #struct_name::__implicit_option_mask_for_len(bytes.len()).is_none() {
            pinocchio_log::log!(
                "Invalid implicit Option encoding for {}: len {} is not one of {}",
                stringify!(#struct_name),
                bytes.len(),
                #valid_lens,
            );
            return Err(pinocchio::error::ProgramError::InvalidInstructionData);
        }
    })
}

fn exact_data_lens(layouts: &[FieldLayout]) -> Option<Vec<usize>> {
    let mut lengths = BTreeSet::from([0usize]);
    for layout in layouts {
        let slot_lengths: BTreeSet<usize> = match layout {
            FieldLayout::Value { value, optional } => match optional {
                OptionalKind::No => BTreeSet::from([value.size()]),
                OptionalKind::Tagged => BTreeSet::from([1, 1 + value.size()]),
                OptionalKind::Implicit(_) => BTreeSet::from([0, value.size()]),
            },
            FieldLayout::Vec { .. } => return None,
        };

        let mut next = BTreeSet::new();
        for total in &lengths {
            for slot_len in &slot_lengths {
                next.insert(total + slot_len);
            }
        }
        lengths = next;
    }

    Some(lengths.into_iter().collect())
}

fn public_len_const(
    min_datalen_expr: proc_macro2::TokenStream,
    min_datalen: usize,
    max_datalen_expr: proc_macro2::TokenStream,
    max_datalen: usize,
    layouts: &[FieldLayout],
) -> syn::Result<proc_macro2::TokenStream> {
    if let Some(exact_lens) = exact_data_lens(layouts) {
        if exact_lens.len() == 1 {
            let data_len = exact_lens[0];
            let doc = format!("Exact size of encoded data = {}", data_len);
            return Ok(quote! {
                #[doc = #doc]
                pub const DATA_LEN: usize = #min_datalen_expr;
            });
        }

        let doc = format!("Exact valid encoded sizes = {:?}", exact_lens);
        let len_count = usize_lit(exact_lens.len());
        let len_lits = exact_lens.iter().map(|len| usize_lit(*len));
        return Ok(quote! {
            #[doc = #doc]
            pub const DATA_LENS: [usize; #len_count] = [#(#len_lits),*];
        });
    }

    let doc = format!(
        "Valid encoded size range = ({}, {})",
        min_datalen, max_datalen
    );
    Ok(quote! {
        #[doc = #doc]
        pub const DATA_LEN_RANGE: (usize, usize) = (#min_datalen_expr, #max_datalen_expr);
    })
}

fn data_len_validation(
    struct_name: &Ident,
    min_datalen: usize,
    max_datalen: usize,
    layouts: &[FieldLayout],
) -> syn::Result<proc_macro2::TokenStream> {
    if let Some(exact_lens) = exact_data_lens(layouts) {
        if exact_lens.len() == 1 {
            let msg = format!(
                "bytes [len={{}}] cannot be deserialized to {} which needs exactly {} bytes",
                struct_name, exact_lens[0]
            );
            return Ok(quote! {
                if bytes.len() != Self::DATA_LEN {
                    pinocchio_log::log!(#msg, bytes.len());
                    return Err(pinocchio::error::ProgramError::InvalidInstructionData);
                }
            });
        }

        let valid_lens = format!("{:?}", exact_lens);
        let len_patterns = exact_lens.iter().map(|len| usize_lit(*len));
        return Ok(quote! {
            if !matches!(bytes.len(), #(#len_patterns)|*) {
                pinocchio_log::log!(
                    "bytes [len={}] cannot be deserialized to {} which needs one of {} bytes",
                    bytes.len(),
                    stringify!(#struct_name),
                    #valid_lens,
                );
                return Err(pinocchio::error::ProgramError::InvalidInstructionData);
            }
        });
    }

    let msg = format!(
        "bytes [len={{}}] cannot be deserialized to {} which needs at least {} and at most {} bytes",
        struct_name, min_datalen, max_datalen
    );
    Ok(quote! {
        if bytes.len() < Self::__MIN_DATA_LEN || bytes.len() > Self::__MAX_DATA_LEN {
            pinocchio_log::log!(#msg, bytes.len());
            return Err(pinocchio::error::ProgramError::InvalidInstructionData);
        }
    })
}

fn validate_borrowed_field_alignment(
    _struct_name: &Ident,
    fields: &syn::punctuated::Punctuated<syn::Field, Token![,]>,
    layouts: &[FieldLayout],
    args: &LayoutArgs,
) -> syn::Result<()> {
    for (index, (field, layout)) in fields.iter().zip(layouts.iter()).enumerate() {
        let Some(requirement) = borrowed_requirement(layout) else {
            continue;
        };

        let field_ident = field.ident.as_ref().expect("named field");
        let align = requirement.align();

        if align > 8 {
            return Err(syn::Error::new(
                field_ident.span(),
                requirement.insufficient_base_alignment_message(
                    field_ident,
                    args.buffer_offset,
                    align,
                ),
            ));
        }

        let start_residues = possible_start_residues(&layouts[..index], align);
        let aligned_residues = shift_residues(
            &start_residues,
            args.buffer_offset + requirement.payload_shift(),
            align,
        );
        if aligned_residues.len() == 1 && aligned_residues.contains(&0) {
            continue;
        }

        if let Some(start_offset) = exact_start_offset(&layouts[..index]) {
            return Err(syn::Error::new(
                field_ident.span(),
                requirement.fixed_offset_alignment_message(
                    field_ident,
                    args.buffer_offset,
                    start_offset,
                ),
            ));
        }

        if aligned_residues.len() == 1 {
            let residue = *aligned_residues.first().expect("single residue");
            return Err(syn::Error::new(
                field_ident.span(),
                requirement.stable_but_misaligned_message(field_ident, args.buffer_offset, residue),
            ));
        }

        return Err(syn::Error::new(
            field_ident.span(),
            requirement.variable_offset_alignment_message(field_ident, args.buffer_offset),
        ));
    }

    Ok(())
}

#[derive(Clone, Copy)]
enum BorrowedRequirement {
    Value {
        align: usize,
        optional_kind: OptionalKind,
    },
    Vec {
        align: usize,
        len_width: usize,
    },
}

impl BorrowedRequirement {
    fn align(self) -> usize {
        match self {
            Self::Value { align, .. } | Self::Vec { align, .. } => align,
        }
    }

    fn payload_shift(self) -> usize {
        match self {
            Self::Value {
                optional_kind: OptionalKind::Tagged,
                ..
            } => 1,
            Self::Value { .. } => 0,
            Self::Vec { len_width, .. } => len_width,
        }
    }

    fn insufficient_base_alignment_message(
        self,
        field_ident: &Ident,
        buffer_offset: usize,
        align: usize,
    ) -> String {
        match self {
            Self::Value { .. } => format!(
                "field `{}` cannot be borrowed with `buffer_offset = {}`: it requires {}-byte alignment, but variable_offset_layout only assumes the original input buffer is 8-byte aligned",
                field_ident, buffer_offset, align
            ),
            Self::Vec { .. } => format!(
                "field `{}` cannot expose a slice view with `buffer_offset = {}`: its elements require {}-byte alignment, but variable_offset_layout only assumes the original input buffer is 8-byte aligned",
                field_ident, buffer_offset, align
            ),
        }
    }

    fn fixed_offset_alignment_message(
        self,
        field_ident: &Ident,
        buffer_offset: usize,
        start_offset: usize,
    ) -> String {
        let payload_offset = start_offset + self.payload_shift();
        let align = self.align();
        match self {
            Self::Value {
                optional_kind: OptionalKind::Tagged,
                ..
            } => format!(
                "field `{}` cannot be borrowed with `buffer_offset = {}`: its Option payload would start at offset {}, so its actual address would be {} mod {}, but borrowed values of this field must be {}-byte aligned",
                field_ident,
                buffer_offset,
                payload_offset,
                (buffer_offset + payload_offset) % align,
                align,
                align
            ),
            Self::Value { .. } => format!(
                "field `{}` cannot be borrowed with `buffer_offset = {}`: it would start at offset {}, so its actual address would be {} mod {}, but borrowed values of this field must be {}-byte aligned",
                field_ident,
                buffer_offset,
                payload_offset,
                (buffer_offset + payload_offset) % align,
                align,
                align
            ),
            Self::Vec { len_width, .. } => format!(
                "field `{}` cannot expose a slice view with `buffer_offset = {}`: its Vec elements start after a {}-byte length prefix, so element 0 would start at offset {}, and its actual address would be {} mod {}, but slice views require {}-byte alignment",
                field_ident,
                buffer_offset,
                len_width,
                payload_offset,
                (buffer_offset + payload_offset) % align,
                align,
                align
            ),
        }
    }

    fn stable_but_misaligned_message(
        self,
        field_ident: &Ident,
        buffer_offset: usize,
        residue: usize,
    ) -> String {
        let align = self.align();
        match self {
            Self::Value { .. } => format!(
                "field `{}` cannot be borrowed with `buffer_offset = {}`: its actual address would always be congruent to {} mod {}, so {}-byte alignment cannot be guaranteed",
                field_ident, buffer_offset, residue, align, align
            ),
            Self::Vec { .. } => format!(
                "field `{}` cannot expose a slice view with `buffer_offset = {}`: element 0 would always be congruent to {} mod {}, so {}-byte alignment cannot be guaranteed",
                field_ident, buffer_offset, residue, align, align
            ),
        }
    }

    fn variable_offset_alignment_message(
        self,
        field_ident: &Ident,
        buffer_offset: usize,
    ) -> String {
        let align = self.align();
        match self {
            Self::Value { .. } => format!(
                "field `{}` cannot be borrowed with `buffer_offset = {}`: earlier variable-sized fields make its actual address vary, so {}-byte alignment cannot be guaranteed",
                field_ident, buffer_offset, align
            ),
            Self::Vec { .. } => format!(
                "field `{}` cannot expose a slice view with `buffer_offset = {}`: earlier variable-sized fields make element 0's actual address vary, so {}-byte alignment cannot be guaranteed",
                field_ident, buffer_offset, align
            ),
        }
    }
}

fn borrowed_requirement(layout: &FieldLayout) -> Option<BorrowedRequirement> {
    match layout {
        FieldLayout::Value { value, optional } => match value.access_mode() {
            AccessMode::Copy => None,
            AccessMode::Ref => Some(BorrowedRequirement::Value {
                align: value.align(),
                optional_kind: *optional,
            }),
        },
        FieldLayout::Vec { elem, flexible } => {
            (elem.align() > 1).then_some(BorrowedRequirement::Vec {
                align: elem.align(),
                len_width: flexible.len_width,
            })
        }
    }
}

fn exact_start_offset(layouts: &[FieldLayout]) -> Option<usize> {
    let mut offset = 0usize;
    for layout in layouts {
        match layout.slot_minmax_len() {
            Ok((_, len)) => offset += len,
            Err(_) => return None,
        }
    }
    Some(offset)
}

fn possible_start_residues(layouts: &[FieldLayout], modulus: usize) -> BTreeSet<usize> {
    let mut residues = BTreeSet::from([0usize]);
    for layout in layouts {
        let len_residues = possible_len_residues(layout, modulus);
        let mut next = BTreeSet::new();
        for start in &residues {
            for len in &len_residues {
                next.insert((start + len) % modulus);
            }
        }
        residues = next;
    }
    residues
}

fn shift_residues(residues: &BTreeSet<usize>, shift: usize, modulus: usize) -> BTreeSet<usize> {
    residues
        .iter()
        .map(|residue| (residue + shift) % modulus)
        .collect()
}

fn possible_len_residues(layout: &FieldLayout, modulus: usize) -> BTreeSet<usize> {
    if modulus == 1 {
        return BTreeSet::from([0]);
    }

    match layout {
        FieldLayout::Value { value, optional } => match optional {
            OptionalKind::No => BTreeSet::from([value.size() % modulus]),
            OptionalKind::Tagged => BTreeSet::from([1 % modulus, (1 + value.size()) % modulus]),
            OptionalKind::Implicit(_) => BTreeSet::from([0, value.size() % modulus]),
        },
        FieldLayout::Vec { elem, flexible } => {
            let capacity = flexible.capacity();
            let period = modulus / gcd(modulus, elem.size());
            let max_k = capacity.min(period.saturating_sub(1));
            let mut residues = BTreeSet::new();
            for k in 0..=max_k {
                residues.insert((flexible.len_width + k * elem.size()) % modulus);
            }
            residues
        }
    }
}

fn gcd(mut a: usize, mut b: usize) -> usize {
    while b != 0 {
        let next = a % b;
        a = b;
        b = next;
    }
    a
}

fn impl_where_clause(bounds: &[proc_macro2::TokenStream]) -> proc_macro2::TokenStream {
    if bounds.is_empty() {
        quote!()
    } else {
        quote!(where #(#bounds,)*)
    }
}

fn strip_field_attr(attrs: &mut Vec<Attribute>) {
    attrs.retain(|attr| !FIELD_ATTRIBUTES.iter().any(|a| attr.path().is_ident(a)));
}

#[derive(Clone, Copy, Debug)]
struct Flexible {
    len_width: usize,
}

impl Flexible {
    fn capacity(self) -> usize {
        match self.len_width {
            1 => 0xFF,
            2 => 0xFFFF,
            _ => unreachable!("Flexible::capacity() - len_width cannot be greater than 2"),
        }
    }

    fn len_width_ty(&self) -> proc_macro2::TokenStream {
        match self.len_width {
            1 => quote!(u8),
            2 => quote!(u16),
            _ => unreachable!("Flexible::len_width_ty() - len_width cannot be greater than 2"),
        }
    }
}

enum FieldAttribute {
    ///
    /// forms:
    ///
    ///   #[flexible = 1|2]
    ///     applicable on Vec only
    ///     Some(usize) represents it's a Vec and its length is encoded as len_width  bytes
    ///
    Flexible(usize),
}

fn parse_field_attr(field: &syn::Field) -> syn::Result<Option<FieldAttribute>> {
    let mut attributes = vec![];
    for attr in &field.attrs {
        if attr.path().is_ident("flexible") {
            match &attr.meta {
                syn::Meta::NameValue(meta) => {
                    let Expr::Lit(ExprLit {
                        lit: Lit::Int(lit_int),
                        ..
                    }) = &meta.value
                    else {
                        return Err(syn::Error::new_spanned(
                            attr,
                            "flexible must use the form `#[flexible = 1|2]` on a Vec field",
                        ));
                    };

                    let len_width: usize = lit_int.base10_parse()?;

                    if len_width > MAX_LEN_WIDTH {
                        return Err(syn::Error::new(
                            field.span(),
                            "flexible must be either 1 or 2",
                        ));
                    }
                    attributes.push(FieldAttribute::Flexible(len_width));
                }
                _meta => {
                    return Err(syn::Error::new_spanned(
                        attr,
                        "flexible must use the form `#[flexible = 1|2]` on a Vec field",
                    ));
                }
            };
        }
    }

    if attributes.len() > 1 {
        Err(syn::Error::new_spanned(
            field,
            "Multiple attributes on a single field not supported",
        ))
    } else {
        Ok(attributes.pop())
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
            "Vec requires exactly one type argument in variable_offset_layout: Vec<T>",
        ));
    };
    if args.args.len() != 1 {
        return Err(syn::Error::new_spanned(
            args,
            "Vec requires exactly one type argument in variable_offset_layout: Vec<T>",
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

fn integer_size_and_align(ty: &Type) -> Option<(usize, usize)> {
    match integer_primitive_name(ty).as_deref() {
        Some("i8" | "u8") => Some((1, 1)),
        Some("i16" | "u16") => Some((2, 2)),
        Some("i32" | "u32") => Some((4, 4)),
        Some("i64" | "u64") => Some((8, 8)),
        Some("i128" | "u128") => Some((16, 16)),
        Some("isize" | "usize") => {
            let size = core::mem::size_of::<usize>();
            Some((size, size))
        }
        _ => None,
    }
}

fn fixed_array_size_and_align(ty: &Type) -> syn::Result<(usize, usize)> {
    let Type::Array(array) = ty else {
        return Err(syn::Error::new_spanned(
            ty,
            "variable_offset_layout fields must be integer primitives or fixed-size arrays",
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
            unsafe { core::ptr::read_unaligned((#bytes_expr).as_ptr().cast::<#ty>()) }
        }),
    }
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
            let raw: [u8; core::mem::size_of::<#ty_tokens>()] = (#bytes_expr)
                .try_into()
                .expect("validated slice length");
            <#ty_tokens>::from_le_bytes(raw)
        }),
    })
}

fn find_data_offset(
    struct_name: &Ident,
    field_offsets: &[FieldOffset],
) -> proc_macro2::TokenStream {
    match field_offsets.last().unwrap() {
        FieldOffset::FixedOffset { offset, .. } => {
            let offset = usize_lit(*offset);
            quote!(#offset)
        }
        FieldOffset::VariableOffset { .. } => {
            let (index, offset, layout) = field_offsets
                .iter()
                .enumerate()
                .rev()
                .find_map(|(index, field_offset)| {
                    field_offset
                        .fixed_offset()
                        .map(|offset| (index, offset, field_offset.layout()))
                })
                .unwrap();

            assert_eq!(layout.is_fixed(), false);

            // | F | F | V | V | V | V |
            //
            let current_offset_expr = field_offsets[index..field_offsets.len() - 1]
                .iter()
                .fold(quote!(#offset), |expr, field_offset| {
                    match field_offset {
                    FieldOffset::FixedOffset { layout, .. } => {
                        // unreachable!("getter_body: FixedOffset is impossible as this point only VariableOffset is expected"),
                        // acutally only the first entry has FixedOffset with layout.is_fixed() == false
                        match layout {
                            FieldLayout::Value { value, optional } => {
                                let fixed_lit = value.size();
                                match optional {
                                    OptionalKind::No => quote! {#expr + #fixed_lit},
                                    OptionalKind::Tagged => quote! {
                                        #expr + if self.bytes[#expr] == 0 { 1 } else { 1 + #fixed_lit }
                                    },
                                    OptionalKind::Implicit(bit_index) => {
                                        let bit_index = usize_lit(*bit_index);
                                        quote! {
                                        #expr + if #struct_name::__implicit_option_present_for_len(self.bytes.len(), #bit_index) { #fixed_lit } else { 0 }
                                    }
                                    },
                                }
                            }
                            FieldLayout::Vec { elem, flexible } => {
                                let elem_size = usize_lit(elem.size());
                                match flexible.len_width {
                                    1 => {
                                        quote! {
                                            #expr + (1 + (self.bytes[#expr] as usize) * #elem_size)
                                        }
                                    }
                                    2 => {
                                        quote! {
                                            #expr + (2 + (core::ptr::read_unaligned(self.bytes[#expr..].as_ptr() as *const u16)  as usize) * #elem_size)
                                        }
                                    }
                                    _ => unreachable!("getter_body: len_width cannot be greater than 2"),
                                }
                            },
                        }
                    },
                    FieldOffset::VariableOffset { layout } => match layout {
                        FieldLayout::Value { value, optional } => {
                            let fixed_lit = value.size();
                            match optional {
                                OptionalKind::No => quote! {#expr + #fixed_lit},
                                OptionalKind::Tagged => quote! {
                                    #expr + if self.bytes[#expr] == 0 { 1 } else { 1 + #fixed_lit }
                                },
                                OptionalKind::Implicit(bit_index) => {
                                    let bit_index = usize_lit(*bit_index);
                                    quote! {
                                    #expr + if #struct_name::__implicit_option_present_for_len(self.bytes.len(), #bit_index) { #fixed_lit } else { 0 }
                                }
                                },
                            }
                        }
                        FieldLayout::Vec { elem, flexible } => {
                            let elem_size = usize_lit(elem.size());
                            match flexible.len_width {
                                1 => {
                                    quote! {
                                        #expr + (1 + (self.bytes[#expr] as usize) * #elem_size)
                                    }
                                }
                                2 => {
                                    quote! {
                                        #expr + (2 + (core::ptr::read_unaligned(self.bytes[#expr..].as_ptr() as *const u16)  as usize) * #elem_size)
                                    }
                                }
                                _ => unreachable!("getter_body: len_width cannot be greater than 2"),
                            }
                        },
                    },
                }
                });

            quote!(#current_offset_expr)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::expand_variable_offset_layout;
    use syn::parse_quote;
    //
    //     #[test]
    //     fn variable_offset_layout_reports_padding_for_large_field() {
    //         let item: syn::ItemStruct = parse_quote! {
    //             struct Args {
    //                 flag: u8,
    //                 payload: [u64; 2],
    //             }
    //         };
    //
    //         let error = expand_variable_offset_layout("", &item)
    //             .unwrap_err()
    //             .to_string();
    //         assert!(error.contains("field `payload` needs 7 byte(s) of padding before it"));
    //         assert!(error.contains("starts at offset 8"));
    //     }
    //
    //     #[test]
    //     fn variable_offset_layout_reports_padding_for_optional_payload() {
    //         let item: syn::ItemStruct = parse_quote! {
    //             struct Args {
    //                 flag: u8,
    //                 payload: Option<[u64; 2]>,
    //             }
    //         };
    //
    //         let error = expand_variable_offset_layout("", &item)
    //             .unwrap_err()
    //             .to_string();
    //         assert!(error.contains("field `payload` needs 6 byte(s) of padding before it"));
    //         assert!(error.contains("Option payload would start at offset 2"));
    //         assert!(error.contains("payload starts at offset 8"));
    //     }
    //
    //     #[test]
    //     fn variable_offset_layout_reports_padding_for_vec_elements() {
    //         let item: syn::ItemStruct = parse_quote! {
    //             struct Args {
    //                 flag: u8,
    //                 padding: [u8; 7],
    //                 #[capacity = 2]
    //                 values: Vec<[u64; 2]>,
    //             }
    //         };
    //
    //         let error = expand_variable_offset_layout("", &item)
    //             .unwrap_err()
    //             .to_string();
    //         assert!(error.contains("field `values` needs 7 byte(s) of padding before it"));
    //         assert!(error.contains("element 0 would start at offset 9"));
    //         assert!(error.contains("element 0 starts at offset 16"));
    //     }
    //
    //     #[test]
    //     fn variable_offset_layout_assumes_earlier_padding_errors_are_fixed() {
    //         let item: syn::ItemStruct = parse_quote! {
    //             struct Args {
    //                 flag: u8,
    //                 payload: [u64; 2],
    //                 _pad1: [u8; 7],
    //                 #[capacity = 2]
    //                 values: Vec<[u64; 2]>,
    //             }
    //         };
    //
    //         let error = expand_variable_offset_layout("", &item)
    //             .unwrap_err()
    //             .to_string();
    //         assert!(error.contains("field `payload` needs 7 byte(s) of padding before it"));
    //         assert!(!error.contains("field `values`"));
    //     }
    //
    //     #[test]
    //     fn variable_offset_layout_rejects_alignment_above_eight() {
    //         let item: syn::ItemStruct = parse_quote! {
    //             struct Args {
    //                 big: u128,
    //             }
    //         };
    //
    //         let error = expand_variable_offset_layout("", &item)
    //             .unwrap_err()
    //             .to_string();
    //         assert!(error.contains("field `big` cannot be borrowed by variable_offset_layout"));
    //         assert!(error.contains("alignment is 16 byte(s)"));
    //         assert!(error.contains("8-byte aligned"));
    //     }
    //
    //     #[test]
    //     fn variable_offset_layout_rejects_vec_without_capacity() {
    //         let item: syn::ItemStruct = parse_quote! {
    //             struct Args {
    //                 values: Vec<u16>,
    //             }
    //         };
    //
    //         let error = expand_variable_offset_layout("", &item)
    //             .unwrap_err()
    //             .to_string();
    //         assert!(error.contains("Vec fields in variable_offset_layout require `#[capacity = N]`"));
    //     }
    //
    //     #[test]
    //     fn variable_offset_layout_rejects_capacity_on_non_vec() {
    //         let item: syn::ItemStruct = parse_quote! {
    //             struct Args {
    //                 #[capacity = 2]
    //                 value: u16,
    //             }
    //         };
    //
    //         let error = expand_variable_offset_layout("", &item)
    //             .unwrap_err()
    //             .to_string();
    //         assert!(
    //             error.contains("attributes are allowed on Vec or Option field only"),
    //             "error: {}",
    //             error
    //         );
    //     }
    //
    #[test]
    fn variable_offset_layout_rejects_implicit_without_option() {
        let item: syn::ItemStruct = parse_quote! {
            struct Args {
                value: u16,
            }
        };

        let error = expand_variable_offset_layout("buffer_offset = 0, option = implicit", &item)
            .unwrap_err()
            .to_string();
        assert!(error.contains(
            "variable_offset_layout(option = implicit) requires at least one Option<T> field"
        ));
    }

    #[test]
    fn variable_offset_layout_rejects_implicit_with_vec() {
        let item: syn::ItemStruct = parse_quote! {
            struct Args {
                value: Option<u16>,
                #[flexible = 1]
                payload: Vec<u8>,
            }
        };

        let error = expand_variable_offset_layout("buffer_offset = 0, option = implicit", &item)
            .unwrap_err()
            .to_string();
        assert!(
            error.contains("variable_offset_layout(option = implicit) does not support Vec fields")
        );
    }

    #[test]
    fn variable_offset_layout_rejects_implicit_with_ambiguous_subset_sums() {
        let item: syn::ItemStruct = parse_quote! {
            struct Args {
                first: Option<u8>,
                second: Option<u16>,
                third: Option<[u8; 3]>,
            }
        };

        let error = expand_variable_offset_layout("buffer_offset = 0, option = implicit", &item)
            .unwrap_err()
            .to_string();
        assert!(error.contains(
            "variable_offset_layout(option = implicit) requires Option<T> payload sizes to have unique subset sums"
        ));
    }

    #[test]
    fn variable_offset_layout_rejects_unknown_parameters() {
        let item: syn::ItemStruct = parse_quote! {
            struct Args {
                value: u16,
            }
        };

        let error = expand_variable_offset_layout("foo = bar", &item)
            .unwrap_err()
            .to_string();
        assert!(error.contains(
            "variable_offset_layout only supports `option = implicit` and `buffer_offset = 0..7`"
        ));
    }

    #[test]
    fn variable_offset_layout_requires_buffer_offset() {
        let item: syn::ItemStruct = parse_quote! {
            struct Args {
                value: u16,
            }
        };

        let error = expand_variable_offset_layout("", &item)
            .unwrap_err()
            .to_string();
        assert!(error.contains("variable_offset_layout requires `buffer_offset = 0..7`"));
    }

    #[test]
    fn variable_offset_layout_rejects_invalid_buffer_offset() {
        let item: syn::ItemStruct = parse_quote! {
            struct Args {
                value: u16,
            }
        };

        let error = expand_variable_offset_layout("buffer_offset = 8", &item)
            .unwrap_err()
            .to_string();
        assert!(error.contains("buffer_offset must be in the range 0..=7"));
    }

    #[test]
    fn variable_offset_layout_rejects_borrowed_field_when_buffer_offset_prevents_alignment() {
        let item: syn::ItemStruct = parse_quote! {
            struct Args {
                values: [u64; 2],
            }
        };

        let error = expand_variable_offset_layout("buffer_offset = 1", &item)
            .unwrap_err()
            .to_string();
        assert!(error.contains("field `values` cannot be borrowed with `buffer_offset = 1`"));
        assert!(error.contains("must be 8-byte aligned"));
    }

    #[test]
    fn variable_offset_layout_rejects_fixed_misaligned_vec_slice() {
        let item: syn::ItemStruct = parse_quote! {
            struct Args {
                flag: u8,
                #[flexible = 1]
                values: Vec<u64>,
            }
        };

        let error = expand_variable_offset_layout("buffer_offset = 0", &item)
            .unwrap_err()
            .to_string();
        assert!(error.contains("field `values` cannot expose a slice view"));
        assert!(error.contains("element 0 would start at offset 2"));
    }

    #[test]
    fn variable_offset_layout_rejects_borrowed_field_after_unstable_variable_field() {
        let item: syn::ItemStruct = parse_quote! {
            struct Args {
                tag: u8,
                #[flexible = 1]
                prefix: Vec<u8>,
                values: [u64; 2],
            }
        };

        let error = expand_variable_offset_layout("buffer_offset = 0", &item)
            .unwrap_err()
            .to_string();
        assert!(error.contains("field `values` cannot be borrowed"));
        assert!(error.contains("earlier variable-sized fields make its actual address vary"));
    }
}
