use std::fmt::Debug;

use proc_macro2::Span;
use quote::{format_ident, quote, ToTokens};
use syn::{
    spanned::Spanned, Attribute, Expr, ExprLit, Fields, GenericArgument, Ident, ItemStruct, Lit,
    LitInt, PathArguments, Type, TypePath,
};

const FIELD_ATTRIBUTES: &[&str] = &["capacity", "flexible"];

// [capacity = flexible] array-len is always encoded as 2-bytes
const MAX_LEN_WIDTH: usize = 2;

const MAX_CAPACITY: usize = 0xffff;

pub(crate) fn expand_variable_offset_layout(
    attr: &str,
    input: &ItemStruct,
) -> syn::Result<proc_macro2::TokenStream> {
    parse_args(attr)?;
    let mut emitted_input = input.clone();
    emitted_input
        .attrs
        .retain(|attr| !attr.path().is_ident("variable_offset_layout"));

    ensure_allow_dead_code(&mut emitted_input.attrs);

    let struct_name = &emitted_input.ident;
    let view_name = format_ident!("{}View", struct_name);

    let Fields::Named(fields) = &mut emitted_input.fields else {
        return Err(syn::Error::new_spanned(
            &emitted_input.fields,
            "variable_offset_layout requires named fields",
        ));
    };

    let mut offset = 0usize;
    let mut offsets = vec![0];

    let total_len_expr = quote!();
    let fields_encode_expr = quote!();

    let mut where_bounds = Vec::<proc_macro2::TokenStream>::new();
    let mut view_methods = Vec::new();
    let mut validate_steps = Vec::new();

    let layout_error: Option<syn::Error> = None;
    let mut min_datalen = 0;
    let mut max_datalen = 0;
    let mut min_datalen_expr = quote!();
    let mut max_datalen_expr = quote!();

    let mut field_offsets = vec![];
    let mut seen_variable_sized_field = false;

    let field_count = fields.named.len();
    for (index, field) in fields.named.iter_mut().enumerate() {
        let field_ident = field.ident.as_ref().expect("named field");

        let is_last_field = index + 1 == field_count;

        let field_layout = parse_field_layout(field, is_last_field)?;
        let has_dynamic_offset = seen_variable_sized_field;

        strip_field_attr(&mut field.attrs);

        // match layout.check_ref_alignment(offset, field_ident) {
        //     Ok(Some(issue)) => {
        //         if let Some(existing) = &mut layout_error {
        //             existing.combine(issue.error);
        //         } else {
        //             layout_error = Some(issue.error);
        //         }
        //         offset += issue.padding;
        //     }
        //     Ok(None) => {}
        //     Err(err) => {
        //         if let Some(existing) = &mut layout_error {
        //             existing.combine(err);
        //         } else {
        //             layout_error = Some(err);
        //         }
        //     }
        // }

        if has_dynamic_offset {
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
                min_datalen += slot_len;
                max_datalen += slot_len;
                offset += slot_len;

                if min_datalen_expr.is_empty() {
                    min_datalen_expr = slot_len_expr.clone();
                    max_datalen_expr = slot_len_expr;
                } else {
                    min_datalen_expr = quote!(#min_datalen_expr + #slot_len_expr);
                    max_datalen_expr = quote!(#max_datalen_expr + #slot_len_expr);
                }
            }
            Err(((slot_min_len_expr, slot_min_len), (slot_max_len_expr, slot_max_len))) => {
                min_datalen += slot_min_len;
                max_datalen += slot_max_len;
                seen_variable_sized_field = true;

                if min_datalen_expr.is_empty() {
                    min_datalen_expr = slot_min_len_expr.clone();
                    max_datalen_expr = slot_max_len_expr;
                } else {
                    min_datalen_expr = quote!(#min_datalen_expr + #slot_min_len_expr);
                    max_datalen_expr = quote!(#max_datalen_expr + #slot_max_len_expr);
                }
            }
        }

        validate_steps.push(field_layout.gen_validate_step(field_ident));
        view_methods.push(field_layout.gen_view_methods(
            offset,
            field_ident,
            &field_offsets[..=index],
        )?);

        // fields_encode_expr = layout.gen_field_encode(fields_encode_expr, offset, field_ident);

        if let Some(bound) = field_layout.bound() {
            where_bounds.push(bound);
        }
    }

    // remove the last entry, as it is supposed to be for the NEXT field
    // to the last one and next-to-last does not exist.
    // field_offsets.pop();

    // println!("field_layouts: {:#?}", field_offsets);

    let field_count_expr = {
        let field_count = usize_lit(field_count);
        quote!(#field_count)
    };
    let (offsets_expr, datalen) = {
        // I could use offsets directly but that generates the code littered
        // with usize suffixes, e.g:
        //
        //  const OFFSETS: [usize; 6usize] = [0usize, 4usize, 12usize, 45usize, 18usize];
        //
        // I hate this, which is why I'm converting Vec<usize> into Vec<LitInt>.
        //
        let datalen = offsets.pop().unwrap();
        let offsets: Vec<_> = offsets.into_iter().map(usize_lit).collect();

        // the last one isn't offset to any member, but
        // actually represents the size of the struct

        (quote! { [#(#offsets),*] }, datalen)
    };

    if let Some(err) = layout_error {
        return Err(err);
    }

    let where_clause = impl_where_clause(&where_bounds);

    let min_msg = format!("Minimum size of encoded data = {}", min_datalen);
    let max_msg = format!("Maximum size of encoded data = {}", max_datalen);

    let min_datalen_lit = usize_lit(min_datalen);
    let max_datalen_lit = usize_lit(max_datalen);
    let msgfmt_1 = if max_datalen != min_datalen {
        format!(
        "bytes [len={{}}] cannot be deserialized to {} which needs at least {} and at most {} bytes"
       , struct_name, min_datalen, max_datalen)
    } else {
        format!(
            "bytes [len={{}}] cannot be deserialized to {} which needs exactly {} bytes",
            struct_name, min_datalen
        )
    };

    Ok(quote! {
        #emitted_input

        impl #struct_name {
            #[doc = #min_msg]
            pub const MIN_DATA_LEN: usize = #min_datalen_expr;
            #[doc = #max_msg]
            pub const MAX_DATA_LEN: usize = #max_datalen_expr;

            //#[doc = "Byte offsets marking the start of each field"]
            //pub const OFFSETS: [usize; #field_count_expr] = #offsets_expr;

            pub fn try_view_from(
                bytes: &[u8],
            ) -> core::result::Result<#view_name<'_>, pinocchio::error::ProgramError> {
                Self::__validate_bytes(bytes)?;
                Ok(#view_name { bytes })
            }

            // pub fn encode_to(&self, bytes: &mut [u8]) -> core::result::Result<(), pinocchio::error::ProgramError> {
            //     #fields_encode_expr;
            //     Ok(())
            // }

            // pub fn encode(&self) -> core::result::Result<#encoding_ret_ty, pinocchio::error::ProgramError> {
            //     let mut bytes = #encoding_buf_var;
            //     self.encode_to(&mut bytes)?;
            //     Ok(bytes)
            // }

            fn __validate_bytes(
                bytes: &[u8],
            ) -> core::result::Result<(), pinocchio::error::ProgramError> {
                if bytes.len() < Self::MIN_DATA_LEN || bytes.len() > Self::MAX_DATA_LEN {
                    pinocchio_log::log!(#msgfmt_1, bytes.len());
                    return Err(
                        pinocchio::error::ProgramError::InvalidInstructionData,
                    );
                } else if bytes.as_ptr().align_offset(8) != 0 {
                    pinocchio_log::log!("bytes [align_offset={}] cannot be deserialized to {} which requires 8-byte alignment", bytes.as_ptr().align_offset(8), stringify!(#struct_name));
                    return Err(
                        pinocchio::error::ProgramError::InvalidInstructionData,
                    );
                }

                let mut offset = 0usize;
                #(#validate_steps)*

                Ok(())
            }

            // fn __validate_option(
            //     bytes: &[u8],
            //     offset: usize,
            //     field_name: &'static str,
            // ) -> core::result::Result<(), pinocchio::error::ProgramError> {
            //     match bytes[offset] {
            //         0 | 1 => {}

            //         tag => {
            //             pinocchio_log::log!("Invalid Option tag for field {}::{} : tag = {} (which should be either 0 or 1)", stringify!(#struct_name), field_name, tag);
            //             return Err(pinocchio::error::ProgramError::InvalidInstructionData);
            //         }
            //     }
            //     Ok(())
            // }

            // fn __validate_vec_len(
            //     bytes: &[u8],
            //     offset: usize,
            //     capacity: usize,
            //     len_width: usize,
            //     field_name: &'static str,
            //     expect_msg: &'static str
            // ) -> core::result::Result<(), pinocchio::error::ProgramError> {
            //     let len = match len_width {
            //         1 =>  bytes[offset] as usize,
            //         2 => {
            //             let raw: [u8; 2] =bytes[offset..offset + 2].try_into().expect("validated len");
            //             u16::from_le_bytes(raw) as usize
            //         },
            //         _ => {
            //             unreachable!()
            //         }
            //     };
            //     if len > capacity {
            //         pinocchio_log::log!("Invalid Vec length for field {}::{} : capacity = {}, len = {}", stringify!(#struct_name), field_name, capacity, len);
            //         return Err(pinocchio::error::ProgramError::InvalidInstructionData);
            //     }
            //     Ok(())
            // }
            //

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
        optional: bool,
    },
    Vec {
        elem: FixedValueKind,
        flexible: Flexible,
    },
}

struct PaddingIssue {
    padding: usize,
    error: syn::Error,
}

impl FieldLayout {
    fn is_fixed(&self) -> bool {
        match self {
            FieldLayout::Value { optional, .. } => *optional == false,
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
                if *optional {
                    Err((
                        (quote!(1), 1),
                        (quote!((1 + core::mem::size_of::<#ty>())), 1 + value.size()),
                    ))
                } else {
                    Ok((quote!(core::mem::size_of::<#ty>()), value.size()))
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

    fn gen_validate_step(&self, field_ident: &Ident) -> proc_macro2::TokenStream {
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

                if *optional {
                    quote! {
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
                    }
                } else {
                    quote! {
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
        offset: usize,
        field_ident: &Ident,
        field_offsets: &[FieldOffset],
    ) -> syn::Result<proc_macro2::TokenStream> {
        match self {
            Self::Value { value, optional } => {
                let ty = value.ty();
                let value_size = usize_lit(value.size());
                let access_mode = value.access_mode();
                let offset_expr = find_data_offset(value, field_offsets);
                let data_offset_expr = if *optional {
                    // note that it refers to the varible 'offset' declared inside the generated code,
                    // not to offset_expr which is an expression.
                    quote!(offset + 1)
                } else {
                    quote!(offset)
                };
                let slice_expr =
                    quote!(&self.bytes[#data_offset_expr..#data_offset_expr+#value_size]);

                let return_expr = match access_mode {
                    AccessMode::Copy => read_copy_expr(value, slice_expr)?,
                    AccessMode::Ref => quote!(bytemuck::from_bytes::<#ty>(#slice_expr)),
                };

                match (*optional, access_mode) {
                    (false, AccessMode::Copy) => Ok(quote! {
                        pub fn #field_ident(&self) -> #ty {
                            let offset = #offset_expr;
                            #return_expr
                        }
                    }),
                    (false, AccessMode::Ref) => Ok(quote! {
                        pub fn #field_ident(&self) -> &#ty {
                            let offset = #offset_expr;
                            #return_expr
                        }
                    }),
                    (true, AccessMode::Copy) => {
                        //let value_body = getter_body(value, offset + 1)?;
                        Ok(quote! {
                            pub fn #field_ident(&self) -> core::option::Option<#ty> {
                                // (self.bytes[(#offset)] != 0).then(||#getter_body)
                                let offset = #offset_expr;
                                (self.bytes[offset] != 0).then(|| #return_expr)
                            }
                        })
                    }
                    (true, AccessMode::Ref) => {
                        //let value_body = getter_body(value, offset + 1)?;
                        Ok(quote! {
                            pub fn #field_ident(&self) -> core::option::Option<&#ty> {
                                //(self.bytes[(#offset)] != 0).then(||#getter_body)
                                let offset = #offset_expr;
                                (self.bytes[offset] != 0).then(|| #return_expr)
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

                //let access_mode = elem.access_mode();
                let offset_expr = find_data_offset(elem, field_offsets);
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
                // Ok(quote!())
                // let elem_ty = elem.ty();
                // let len_expr = read_len_expr(offset, capacity.len_width());
                // let offset = usize_lit(offset);
                // let len_width_lit = capacity.len_width_lit();
                // if let Some(cap) = capacity.comptime_capacity_lit() {
                //     let capacity_name = format_ident!("{}_capacity", accessor_ident(field_ident));
                //     Ok(quote! {
                //         pub fn #field_ident(&self) -> &[#elem_ty] {
                //             let len = #len_expr;
                //             let start = #offset + #len_width_lit;
                //             let end = start + (len * core::mem::size_of::<#elem_ty>());
                //             bytemuck::cast_slice::<u8, #elem_ty>(&self.bytes[start..end])
                //         }

                //         pub const fn #capacity_name(&self) -> usize {
                //             #cap
                //         }
                //     })
                // } else {
                //     Ok(quote! {
                //         pub fn #field_ident(&self) -> &[#elem_ty] {
                //             let len = #len_expr;
                //             let start = #offset + #len_width_lit;
                //             let end = start + (len * core::mem::size_of::<#elem_ty>());
                //             bytemuck::cast_slice::<u8, #elem_ty>(&self.bytes[start..end])
                //         }
                //     })
                // }
            }
        }
    }
}

fn parse_field_layout(field: &syn::Field, _is_last_field: bool) -> syn::Result<FieldLayout> {
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
            optional: true,
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
        optional: false,
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
            "variable_offset_layout does not support parameters",
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
                            "flexible must use the form `#[flexible = 1|2]` (on Vec field) or #[flexible] (on Option field)",
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
                        "flexible must use the form `#[flexible = 1|2]` (on Vec field) or #[flexible] (on Option field)",
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

fn size_and_align_of(ty: &Type) -> (usize, usize) {
    integer_size_and_align(ty)
        .or_else(|| fixed_array_size_and_align(ty).ok())
        .expect("type must be a supported by now")
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

fn borrow_ref_expr(ty: &Type, bytes_expr: proc_macro2::TokenStream) -> proc_macro2::TokenStream {
    quote!(bytemuck::from_bytes::<#ty>(#bytes_expr))
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
    value: &FixedValueKind,
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
                                if *optional {
                                    let fixed_lit = value.size();
                                    quote! {
                                        #expr + if self.bytes[#expr] == 0 { 1 } else { 1 + #fixed_lit }
                                    }
                                } else {
                                    let fixed_lit = value.size();
                                    quote! {#expr + #fixed_lit}
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
                            if *optional {
                                let fixed_lit = value.size();
                                quote! {
                                    #expr + if self.bytes[#expr] == 0 { 1 } else { 1 + #fixed_lit }
                                }
                            } else {
                                let fixed_lit = value.size();
                                quote! {#expr + #fixed_lit}
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

fn find_data_offset__(
    value: &FixedValueKind,
    field_offsets: &[FieldOffset],
) -> syn::Result<proc_macro2::TokenStream> {
    let ty = value.ty();
    match field_offsets.last().unwrap() {
        FieldOffset::FixedOffset { offset, .. } => {
            let slice_expr = bytes_slice_expr(*offset, value.size());
            match value.access_mode() {
                AccessMode::Copy => read_copy_expr(value, slice_expr),
                AccessMode::Ref => Ok(borrow_ref_expr(ty, slice_expr)),
            }
        }
        FieldOffset::VariableOffset { .. } => {
            if field_offsets.len() == 0 {
                return Ok(quote! {959});
            }

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

            let current = field_offsets.last().unwrap().layout();

            println!("field_offsets: {:#?}", field_offsets);
            println!("index: {:?}", index);
            println!("offset: {:?}", offset);

            // | F | F | V | V | V | V |
            //
            let current_offset_expr = field_offsets[index..field_offsets.len() - 1]
                .iter()
                .fold(quote!(#offset), |expr, field_offset| match field_offset {
                    FieldOffset::FixedOffset { layout, .. } => {
                        // unreachable!("getter_body: FixedOffset is impossible as this point only VariableOffset is expected"),
                        // acutally only the first entry has FixedOffset with layout.is_fixed() == false
                        match layout {
                            FieldLayout::Value { value, optional } => {
                                if *optional {
                                    let fixed_lit = value.size();
                                    quote! {
                                        if self.bytes[0] == 0 {
                                            #expr + 1
                                        } else {
                                            #expr + (1 + #fixed_lit)
                                        }
                                    }
                                } else {
                                    let fixed_lit = value.size();
                                    quote! {#expr + #fixed_lit}
                                }
                            }
                            FieldLayout::Vec { elem, flexible } => {
                                let elem_size = usize_lit(elem.size());
                                match flexible.len_width {
                                    1 => {
                                        quote! {
                                            #expr + (1 + (self.bytes[0] as usize) * #elem_size)
                                        }
                                    }
                                    2 => {
                                        quote! {
                                            #expr + (2 + (core::ptr::read_unaligned(self.bytes.as_ptr() as *const u16)  as usize) * #elem_size)
                                        }
                                    }
                                    _ => unreachable!("getter_body: len_width cannot be greater than 2"),
                                }
                            },
                        }
                    },
                    FieldOffset::VariableOffset { layout, .. } => match layout {
                        FieldLayout::Value { value, optional } => {
                            if *optional {
                                let fixed_lit = value.size();
                                quote! {
                                    if self.bytes[0] == 0 {
                                        #expr + 1
                                    } else {
                                        #expr + (1 + #fixed_lit)
                                    }
                                }
                            } else {
                                let fixed_lit = value.size();
                                quote! {#expr + #fixed_lit}
                            }
                        }
                        FieldLayout::Vec { elem, flexible } => {
                            let elem_size = usize_lit(elem.size());
                            match flexible.len_width {
                                1 => {
                                    quote! {
                                        #expr + (1 + (self.bytes[0] as usize) * #elem_size)
                                    }
                                }
                                2 => {
                                    quote! {
                                        #expr + (2 + (core::ptr::read_unaligned(self.bytes.as_ptr() as *const u16)  as usize) * #elem_size)
                                    }
                                }
                                _ => unreachable!("getter_body: len_width cannot be greater than 2"),
                            }
                        },
                    },
                });

            Ok(quote! {
                let offset = #current_offset_expr;
            })
        }
    }
}

fn read_len_expr(offset: usize, len_width: usize) -> proc_macro2::TokenStream {
    match len_width {
        1 => quote!(self.bytes[#offset] as usize),
        2 => quote!({
            let raw: [u8; 2] = self.bytes[#offset..#offset + 2].try_into().expect("validated len");
            u16::from_le_bytes(raw) as usize
        }),
        _ => {
            unreachable!("read_len_expr: len_width more than 2 isn't supported")
        }
    }
}

// #[cfg(test)]
// mod tests {
//     use super::expand_variable_offset_layout;
//     use syn::parse_quote;
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
//     #[test]
//     fn variable_offset_layout_rejects_parameters() {
//         let item: syn::ItemStruct = parse_quote! {
//             struct Args {
//                 value: u16,
//             }
//         };
//
//         let error = expand_variable_offset_layout("mut", &item)
//             .unwrap_err()
//             .to_string();
//         assert!(error.contains("variable_offset_layout does not support parameters"));
//     }
// }
