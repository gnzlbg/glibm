//! A proc macro to analyze the libm APIs We want to exhaustively match all
// fields here to create a compilation error if new fields are added.
#![allow(clippy::unneeded_field_pattern)]

extern crate proc_macro;
use self::proc_macro::TokenStream;
use quote::quote;
use std::collections::HashSet;
use syn::parse_macro_input;

/// `input` contains a single identifier, corresponding to a user-defined macro.
/// This identifier is expanded for each libm public API.
///
/// See tests/analyze or below for the API.
#[proc_macro]
pub fn for_each_api(input: TokenStream) -> TokenStream {
    let input = parse_macro_input!(input as Input);
    let files = get_libm_files();
    let functions = get_functions(&files, &input.ignored);
    let mut tokens = proc_macro2::TokenStream::new();
    let input_macro = input.macro_id;
    for function in functions {
        let id = function.ident;
        let api_kind = function.api_kind;
        let ret_ty = function.ret_ty;
        let arg_tys = function.arg_tys;
        let arg_ids = get_arg_ids(arg_tys.len());
        let t = quote! {
            #input_macro! {
                id: #id;
                api_kind: #api_kind;
                arg_tys: #(#arg_tys),*;
                arg_ids: #(#arg_ids),*;
                ret_ty: #ret_ty;
            }

        };
        tokens.extend(t);
    }
    tokens.into()
}

/// Traverses the libm crate directory, parsing all .rs files
fn get_libm_files() -> Vec<syn::File> {
    // Find the directory of the libm crate:
    let root_dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
    let libm_dir = root_dir
        .parent()
        .expect("couldn't access crates/ dir")
        .join("libm");
    let libm_src_dir = libm_dir.join("src");

    // Traverse all Rust files, parsing them as `syn::File`
    let mut files = Vec::new();
    for entry in walkdir::WalkDir::new(libm_src_dir)
        .into_iter()
        .filter_map(Result::ok)
    {
        use std::io::Read;
        let file_path = entry.path();
        if file_path.is_dir()
            || !file_path
                .to_str()
                .expect("can't format file path")
                .ends_with(".rs")
        {
            // If the path is a directory or not a ".rs" file => skip it.
            continue;
        }

        // Read the file into a string, and parse it into an AST using syn.
        let mut file_string = String::new();
        std::fs::File::open(&file_path)
            .unwrap_or_else(|_| panic!("can't open file at path: {}", file_path.display()))
            .read_to_string(&mut file_string)
            .expect("failed to read file to string");
        let file = syn::parse_file(&file_string).expect("failed to parse");
        files.push(file);
    }
    files
}

/// Function signature that will be expanded for the user macro.
struct FnSig {
    ident: syn::Ident,
    api_kind: syn::Ident,
    c_abi: bool,
    ret_ty: Option<syn::Type>,
    arg_tys: Vec<syn::Type>,
}

impl FnSig {
    fn name(&self) -> String {
        self.ident.to_string()
    }
}

macro_rules! syn_to_str {
    ($e:expr) => {{
        let t = $e;
        let tokens = quote! {
            #t
        };
        format!("{}", tokens)
    }};
}

/// Extracts all public functions from the libm files while
/// doing some sanity checks on the function signatures.
fn get_functions(files: &[syn::File], ignored: &Option<HashSet<String>>) -> Vec<FnSig> {
    let mut error = false;
    let mut functions = Vec::new();
    // Traverse all files matching function items
    for item in files.iter().flat_map(|f| f.items.iter()) {
        let mut e = false;
        if let syn::Item::Fn(syn::ItemFn {
            vis: syn::Visibility::Public(_),
            ident,
            constness,
            asyncness,
            unsafety,
            attrs,
            abi,
            decl,
            block: _,
        }) = item
        {
            // Build a function signature while doing some sanity checks
            let mut fn_sig = FnSig {
                ident: ident.clone(),
                api_kind: to_api_kind(ident.clone()),
                c_abi: false,
                arg_tys: Vec::new(),
                ret_ty: None,
            };
            // Skip ignored functions:
            if let Some(ignored) = ignored {
                if ignored.contains(&fn_sig.name()) {
                    continue;
                }
            }
            macro_rules! err {
                ($msg:expr) => {{
                    #[cfg(feature = "analyze")]
                    {
                        eprintln!("[error]: Function \"{}\" {}", fn_sig.name(), $msg);
                    }
                    #[allow(unused_assignments)]
                    {
                        e = true;
                    }
                    ()
                }};
            }
            if let Some(syn::Abi {
                name: Some(l),
                extern_token: _,
            }) = abi
            {
                if l.value() == "C" {
                    fn_sig.c_abi = true;
                }
            }
            // If the function signature isn't extern "C", we aren't ABI compatible
            // with libm.
            if !fn_sig.c_abi {
                err!("not `extern \"C\"`");
            }
            // Right now there are no const fn functions. We might add them
            // in the future, and at that point, we should tune this here.
            // In the mean time, error if somebody tries.
            if constness.is_some() {
                err!("is const");
            }
            // No functions should be async fn
            if asyncness.is_some() {
                err!("is async");
            }
            // FIXME: Math functions are not unsafe. Some functions in the
            // libm C API take pointers, but in our API take repr(Rust)
            // tuples (for some reason). Once we fix those to have the same
            // API as C libm, we should use references on their signature
            // instead, and make them safe.
            if unsafety.is_some() {
                let e2 = e;
                err!("is unsafe");
                e = e2;
            }
            let syn::FnDecl {
                fn_token: _,
                generics,
                paren_token: _,
                inputs,
                variadic,
                output,
            } = (**decl).clone();

            // Forbid generic parameters, lifetimes, and consts in public APIs:
            if variadic.is_some() {
                err!(format!(
                    "contains variadic arguments \"{}\"",
                    syn_to_str!(variadic.unwrap())
                ));
            }
            if generics.type_params().count() != 0 {
                err!(format!(
                    "contains generic parameters \"{}\"",
                    syn_to_str!(generics.clone())
                ));
            }
            if generics.lifetimes().count() != 0 {
                err!(format!(
                    "contains lifetime parameters \"{}\"",
                    syn_to_str!(generics.clone())
                ));
            }
            if generics.const_params().count() != 0 {
                err!(format!(
                    "contains const parameters \"{}\"",
                    syn_to_str!(generics.clone())
                ));
            }
            if attrs.is_empty() {
                err!("missing `#[inline]` and `#[no_panic]` attributes");
            } else {
                let attrs = attrs
                    .iter()
                    .map(|a| syn_to_str!(a))
                    .collect::<Vec<_>>()
                    .join(",");
                if !attrs.contains("inline") {
                    err!("missing `#[inline]` attribute");
                }
                if !attrs.contains("no_panic") {
                    err!("missing `#[no_panic]` attributes");
                }
            }
            // Validate and parse output parameters and function arguments:
            match output {
                syn::ReturnType::Default => (),
                syn::ReturnType::Type(_, ref b) if valid_ty(&b) => fn_sig.ret_ty = Some(*b.clone()),
                other => err!(format!("returns unsupported type {}", syn_to_str!(other))),
            }
            for input in inputs {
                match input {
                    syn::FnArg::Captured(ref c) if valid_ty(&c.ty) => {
                        fn_sig.arg_tys.push(c.ty.clone())
                    }
                    other => err!(format!(
                        "takes unsupported argument type {}",
                        syn_to_str!(other)
                    )),
                }
            }
            // If there was an error, we skip the function.
            // Otherwise, the user macro is expanded with
            // the function:
            if e {
                error = true;
            } else {
                functions.push(fn_sig);
            }
        }
    }
    if error {
        // too many errors:
        //        panic!("errors found");
    }
    functions
}

/// Parses a type into a String - arg is true if the type is an argument, and
/// false if its a return value.
fn valid_ty(t: &syn::Type) -> bool {
    match t {
        syn::Type::Ptr(p) => {
            let c = p.const_token.is_some();
            let m = p.mutability.is_some();
            assert!(!(c && m));
            match &*p.elem {
                syn::Type::Path(_) => valid_ty(&p.elem),
                // Only one layer of pointers allowed:
                _ => false,
            }
        }
        syn::Type::Path(p) => {
            assert!(p.qself.is_none());
            assert_eq!(p.path.segments.len(), 1);
            let s = p
                .path
                .segments
                .first()
                .unwrap()
                .into_value()
                .ident
                .to_string();
            match s.as_str() {
                "i8" | "i16" | "i32" | "i64" | "isize" | "u8" | "u16" | "u32" | "u64" | "usize"
                | "f32" | "f64" => true,
                _ => false,
            }
        }
        _ => false,
    }
}

/// Returns a vector containing `len` identifiers.
fn get_arg_ids(len: usize) -> Vec<syn::Ident> {
    let mut ids = Vec::new();
    for i in 0..len {
        let x = format!("x{}", i);
        ids.push(syn::Ident::new(&x, proc_macro2::Span::call_site()));
    }
    ids
}

/// Returns the `ApiKind` enum variant for this function
fn to_api_kind(id: syn::Ident) -> syn::Ident {
    let name = syn_to_str!(id);
    let first = name.chars().nth(0).unwrap();
    let first_upper = first.to_uppercase().nth(0).unwrap();
    let name = name.replacen(first, &first_upper.to_string(), 1);
    syn::Ident::new(&name, proc_macro2::Span::call_site())
}

#[derive(Debug)]
struct Input {
    macro_id: syn::Ident,
    ignored: Option<HashSet<String>>,
}

impl syn::parse::Parse for Input {
    fn parse(input: syn::parse::ParseStream) -> syn::Result<Self> {
        let content;
        let macro_id: syn::Ident = input.parse()?;
        let lookahead = input.lookahead1();
        if lookahead.peek(syn::token::Paren) {
            let _paren_token = syn::parenthesized!(content in input);
            let ignored: syn::Lit = content.parse::<syn::Lit>()?;
            if let syn::Lit::Str(c) = ignored {
                let s = c.value();
                let mut hash_set = HashSet::<String>::new();
                for i in s.split(',') {
                    hash_set.insert(i.to_string());
                }
                Ok(Self {
                    macro_id,
                    ignored: Some(hash_set),
                })
            } else {
                Err(lookahead.error())
            }
        } else {
            Ok(Self {
                macro_id,
                ignored: None,
            })
        }
    }
}
