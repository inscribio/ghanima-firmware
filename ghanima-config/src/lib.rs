pub mod custom;
pub mod format;
pub mod layers;
pub mod leds;
pub mod mouse;

use std::{path::Path, fs::File, io::{Write, BufReader}};

use anyhow::Context;
use proc_macro2::TokenStream;
use quote::{quote, ToTokens, TokenStreamExt};
use serde::{Serialize, Deserialize};
use schemars::{JsonSchema, schema_for, schema::RootSchema};

#[derive(Serialize, Deserialize, JsonSchema, Debug, PartialEq)]
pub struct KeyboardConfig {
    layers: layers::Layers<custom::Action>,
    mouse: mouse::MouseConfig,
    leds: leds::LedConfigurations,
    timeout: u32,
    bootload_strict: bool,
}

impl ToTokens for KeyboardConfig {
    fn to_tokens(&self, tokens: &mut proc_macro2::TokenStream) {
        let layers = layers::to_tokens(&self.layers);
        let leds = leds::to_tokens(&self.leds);
        let mouse = &self.mouse;
        let timeout = &self.timeout;
        let bootload_strict = &self.bootload_strict;
        tokens.append_all(quote! {
            crate::keyboard::KeyboardConfig {
                layers: &#layers,
                mouse: &#mouse,
                leds: #leds,
                timeout: #timeout,
                bootload_strict: #bootload_strict,
            }
        })
    }
}

impl KeyboardConfig {
    fn n_layers(&self) -> usize {
        self.layers.len()
    }

    fn n_rows(&self) -> usize {
        let n_rows = self.layers.get(0)
            .map(|l| l.len())
            .unwrap_or(0);
        self.layers.iter()
            .enumerate()
            .for_each(|(i, l)| {
                assert_eq!(l.len(), n_rows,
                    "Wrong number of rows in layer {}: {} vs {}", i, l.len(), n_rows)
            });
        n_rows
    }

    fn n_cols(&self) -> usize {
        let n_cols = self.layers.get(0)
            .and_then(|l| l.get(0).map(|r| r.len()))
            .unwrap_or(0);
        self.layers.iter()
            .enumerate()
            .for_each(|(li, l)| l.iter()
                .enumerate()
                .for_each(|(ri, r)| {
                    assert_eq!(r.len(), n_cols,
                        "Wrong number of columns in layer {} row {}: {} vs {}", li, ri, r.len(), n_cols)
                })
            );
        n_cols
    }

    fn file_tokens(&self) -> TokenStream {
        let n_layers = self.n_layers();
        let n_cols = self.n_cols();
        let n_rows = self.n_rows();
        quote! {
            pub const CONFIG: crate::keyboard::KeyboardConfig<N_LAYERS> = #self;
            pub const N_LAYERS: usize = #n_layers;
            #[allow(dead_code)]
            pub const N_COLS: usize = #n_cols;
            #[allow(dead_code)]
            pub const N_ROWS: usize = #n_rows;
        }
    }

    fn to_string_pretty(&self) -> anyhow::Result<String> {
        let file = self.file_tokens().to_string();
        let parsed = syn::parse_file(&file)
            .context(format!("Failed to parse:\n{}", file))?;
        Ok(prettyplease::unparse(&parsed))
    }

    pub fn to_file(&self, path: &Path) -> anyhow::Result<()> {
        let mut file = File::create(path)?;
        let code = self.to_string_pretty()?;
        file.write_all(code.as_bytes())?;
        Ok(())
    }

    pub fn from_file(path: &Path) -> anyhow::Result<Self> {
        let file = File::open(path)?;
        let mut reader = BufReader::new(file);
        let config = serde_json::from_reader(&mut reader)?;
        Ok(config)
    }

    pub fn schema() -> RootSchema {
        schema_for!(Self)
    }

    pub fn schema_to_file(path: &Path) -> anyhow::Result<()> {
        let mut file = File::create(path)?;
        let schema = Self::schema();
        let string = serde_json::to_string_pretty(&schema)?;
        file.write_all(string.as_bytes())?;
        Ok(())
    }
}


/// Implement ToTokens for a simple enum with variants without data.
#[macro_export]
macro_rules! impl_enum_to_tokens {
    ( $( enum $enum:ident: $path:path ),* $(,)? ) => {
        $(
            impl ToTokens for $enum {
                fn to_tokens(&self, tokens: &mut TokenStream) {
                    let v = serde_json::to_value(self).unwrap();
                    let s = v.as_str().unwrap();
                    let i = Ident::new(s, Span::call_site());
                    tokens.append_all(quote! { $path::#i });
                }
            }
        )*
    };
}

/// Implement ToTokens for a simple enum with tuple-like variants.
#[macro_export]
macro_rules! impl_enum_tuple_to_tokens {
    ( $( enum $enum:ident: $path:path { $( $variant:ident( $( $field:ident ),* ) ),* } )* ) => {
        $(
            impl ToTokens for $enum {
                fn to_tokens(&self, tokens: &mut TokenStream) {
                    tokens.append_all(match self {
                        $(
                            Self::$variant( $( $field ),* ) => quote! { $path::$variant( $( #$field ),* ) }
                        ),*
                    });
                }
            }
        )*
    };
}

/// Implement ToTokens for a regular struct
///
/// Generates implementations of ToTokens for a list of structs. Will use $path
/// as the name of struct in generated tokens. Current limitation is that each
/// field in struct def has to end with a comma (even the last one).
#[macro_export]
macro_rules! impl_struct_to_tokens {
    // Main entry point, accept a list of struct definitions
    ( $( struct $struct:ident: $path:path { $($field_defs:tt)* } )* ) => {
        $(
            impl_struct_to_tokens! { @struct $struct: $path { $($field_defs)* } }
        )*
    };

    // Generate ToTokens for a single struct
    ( @struct $struct:ident: $path:path { $($field_defs:tt)* } ) => {
        impl ToTokens for $struct {
            fn to_tokens(&self, tokens: &mut proc_macro2::TokenStream) {
                impl_struct_to_tokens! { @vars self $($field_defs)* }
                tokens.append_all( quote! { $path } );
                let mut fields = proc_macro2::TokenStream::new();
                impl_struct_to_tokens! { @tokens fields $($field_defs)* }
                tokens.append(proc_macro2::Group::new(proc_macro2::Delimiter::Brace, fields));
            }
        }
    };

    // Extract struct field to local variable so that we can use it in quote! later
    // e.g. `let field_a = &self.field_a;`
    ( @vars $self:ident $field:ident, $($field_defs:tt)* ) => {
        let $field = &$self.$field;
        impl_struct_to_tokens! { @vars $self $($field_defs)* }
    };
    // Same for &field
    ( @vars $self:ident & $field:ident, $($field_defs:tt)* ) => {
        impl_struct_to_tokens! { @vars $self $field, $($field_defs)* }
    };
    // For ?field (Option + reference) extract the Option here and add reference to it.
    ( @vars $self:ident &? $field:ident, $($field_defs:tt)* ) => {
        let $field = match &$self.$field {
            Some(inner) => quote! { Some(& #inner) },
            None => quote! { None },
        };
        impl_struct_to_tokens! { @vars $self $($field_defs)* }
    };
    // Same for &[field]
    ( @vars $self:ident &[ $field:ident ], $($field_defs:tt)* ) => {
        impl_struct_to_tokens! { @vars $self $field, $($field_defs)* }
    };
    ( @vars $self:ident ) => {};

    // Add tokens for field assignment inside struct initializer
    // e.g. `Struct { field_a: field_a }`
    ( @tokens $tokens:ident $field:ident, $($field_defs:tt)* ) => {
        $tokens.append_all(quote! {
            $field: #$field,
        });
        impl_struct_to_tokens! { @tokens $tokens $($field_defs)* }
    };
    // Take by reference
    // e.g. `Struct { field_a: &field_a }`
    ( @tokens $tokens:ident & $field:ident, $($field_defs:tt)* ) => {
        $tokens.append_all(quote! {
            $field: & #$field,
        });
        impl_struct_to_tokens! { @tokens $tokens $($field_defs)* }
    };
    // Take as an Option -> same as normal, extracting of Option done during @vars.
    // e.g. `Struct { field_a: Some(&field_a) }`
    ( @tokens $tokens:ident &? $field:ident, $($field_defs:tt)* ) => {
        impl_struct_to_tokens! { @tokens $tokens $field, $($field_defs)* }
    };
    // Take an array by reference
    // e.g. `Struct { field_a: &[ field_a, ... ] }`
    ( @tokens $tokens:ident &[ $field:ident ], $($field_defs:tt)* ) => {
        $tokens.append_all(quote! {
            $field: &[ #( #$field ),* ],
        });
        impl_struct_to_tokens! { @tokens $tokens $($field_defs)* }
    };
    ( @tokens $tokens:ident ) => {};
}

#[cfg(test)]
mod tests {
    use crate::format::assert_tokens_eq;

    use super::*;

    pub fn example_json() -> serde_json::Value {
        serde_json::json!({
            "layers": layers::tests::example_json(),
            "leds": leds::tests::example_json(),
            "mouse": mouse::tests::example_json(),
            "timeout": 1000u32,
            "bootload_strict": true,
        })
    }

    pub fn example_config() -> KeyboardConfig {
        KeyboardConfig {
            layers: layers::tests::example_config(),
            leds: leds::tests::example_config(),
            mouse: mouse::tests::example_config(),
            timeout: 1000,
            bootload_strict: true,
        }
    }

    pub fn example_code() -> TokenStream {
        let layers = layers::tests::example_code();
        let leds = leds::tests::example_code();
        let mouse = mouse::tests::example_code();
        quote! {
            crate::keyboard::KeyboardConfig {
                layers: &#layers,
                mouse: &#mouse,
                leds: #leds,
                timeout: 1000u32,
                bootload_strict: true,
            }
        }
    }

    #[test]
    fn deserialize() -> anyhow::Result<()> {
        let config: KeyboardConfig = serde_json::from_value(example_json())?;
        assert_eq!(config, example_config());
        Ok(())
    }

    #[test]
    fn tokenize() {
        let config = example_config();
        assert_tokens_eq(quote! { #config }, example_code())
    }

    // #[test]
    // fn example() -> anyhow::Result<()> {
    //     let config = KeyboardConfig::from_file(Path::new("./config.json"))?;
    //     config.to_file(Path::new("/tmp/config.rs"))?;
    //     Ok(())
    // }
    //
    // #[test]
    // fn schema() -> anyhow::Result<()> {
    //     KeyboardConfig::schema_to_file(Path::new("/tmp/schema.json"))?;
    //     Ok(())
    // }
}
