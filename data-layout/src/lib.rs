use proc_macro::TokenStream;
use syn::{parse_macro_input, ItemStruct};

mod fixed_layout;
mod variable_layout;

#[proc_macro_attribute]
pub fn variable_layout(attr: TokenStream, item: TokenStream) -> TokenStream {
    let attr_string = attr.to_string();
    let input = parse_macro_input!(item as ItemStruct);

    match variable_layout::expand_variable_layout(&attr_string, &input) {
        Ok(tokens) => tokens.into(),
        Err(err) => err.to_compile_error().into(),
    }
}

#[proc_macro_attribute]
pub fn fixed_layout(attr: TokenStream, item: TokenStream) -> TokenStream {
    let attr_string = attr.to_string();
    let input = parse_macro_input!(item as ItemStruct);

    match fixed_layout::expand_fixed_layout(&attr_string, &input) {
        Ok(tokens) => tokens.into(),
        Err(err) => err.to_compile_error().into(),
    }
}
