use proc_macro2::TokenStream;
use quote::quote;

/// Wrap expression tokens in syn::File and output formatted string
pub fn format_expr(tokens: TokenStream) -> syn::Result<String> {
    let file = quote! {
        static EXPR: ExprType = #tokens;
    };
    let parsed = syn::parse_file(&file.to_string())?;
    Ok(prettyplease::unparse(&parsed))
}

#[cfg(test)]
pub fn assert_tokens_eq(left: TokenStream, right: TokenStream) {
    let left = format_expr(left).unwrap();
    let right = format_expr(right).unwrap();
    similar_asserts::assert_eq!(left, right);
}
