use lazy_static::lazy_static;
use proc_macro::TokenStream;
use proc_macro2::Span;
use quote::{quote, ToTokens};
use serde::Deserialize;
use std::collections::BTreeMap;
use std::sync::Mutex;
use syn::parse::{Parse, ParseStream};
use syn::punctuated::Punctuated;
use syn::spanned::Spanned;
use syn::{
    parse_macro_input, Error, FnArg, Ident, ImplItemMethod, Pat, PatIdent, PatTuple, ReturnType,
    Signature, Token, Type, TypeTuple, VisPublic, Visibility,
};

#[derive(Default, Deserialize, Debug)]
struct ApiAttrParameters {
    #[serde(rename = "trait", default)]
    pub is_trait: bool,
}

pub(crate) fn api_method(
    method_type: &str,
    attr: TokenStream,
    item: TokenStream,
    is_management_api: bool,
    with_args: bool,
) -> TokenStream {
    let mut input = parse_macro_input!(item as ImplItemMethod);

    if method_type == "update"
        && *crate::derive::IS_METRIC_CANISTER.lock().unwrap()
        && input.sig.ident != "collect_metrics"
    {
        let collect_metrics_stmt = syn::parse2::<syn::Stmt>(quote! {
            self.collect_metrics();
        })
        .unwrap();
        input.block.stmts.insert(0, collect_metrics_stmt);
    }

    let input = input;
    let method = &input.sig.ident;
    let orig_vis = input.vis.clone();

    let parameters =
        serde_tokenstream::from_tokenstream::<ApiAttrParameters>(&attr.into()).unwrap();

    let _ = &input
        .sig
        .generics
        .params
        .iter()
        .filter_map(|generic| match generic {
            syn::GenericParam::Lifetime(_) => Some(generic),
            _ => panic!("candid method does not support generics that are not lifetimes"),
        })
        .collect::<Vec<_>>();

    if let Err(e) = store_candid_definitions(method_type, &input.sig) {
        return e.to_compile_error().into();
    }

    let method_name = method.to_string();
    let export_name = if !is_management_api {
        format!("canister_{method_type} {method_name}")
    } else {
        format!("canister_{method_type}")
    };

    let internal_method = Ident::new(&format!("__{method_name}"), method.span());

    let internal_method_notify = Ident::new(&format!("___{method_name}"), method.span());

    let return_type = &input.sig.output;
    let reply_call = if is_management_api {
        if *return_type != ReturnType::Default {
            panic!("{method_type} method cannot have a return type.");
        }

        quote! {}
    } else {
        match return_type {
            ReturnType::Default => quote! {::ic_cdk::api::call::reply(())},
            ReturnType::Type(_, t) => match t.as_ref() {
                Type::Tuple(_) => quote! {::ic_cdk::api::call::reply(result)},
                _ => quote! {::ic_cdk::api::call::reply((result,))},
            },
        }
    };

    let inner_return_type = match return_type {
        ReturnType::Default => quote! {()},
        ReturnType::Type(_, t) => quote! {#t},
    };

    let args = &input.sig.inputs;
    let mut arg_types = Punctuated::new();
    let mut args_destr = Punctuated::new();
    let mut has_self = false;

    let mut self_lifetime = quote! {};

    for arg in args {
        let (arg_type, arg_pat) = match arg {
            FnArg::Receiver(r) => {
                has_self = true;
                match &r.reference {
                    Some((_, Some(lt))) => {
                        self_lifetime = quote! {#lt};
                        continue;
                    }
                    _ => continue,
                }
            }
            FnArg::Typed(t) => (&t.ty, t.pat.as_ref()),
        };

        let arg_name = match arg_pat {
            Pat::Ident(x) => &x.ident,
            _ => panic!("Invalid arg name"),
        };

        arg_types.push_value(arg_type.as_ref().clone());
        arg_types.push_punct(Default::default());

        let ident = PatIdent {
            attrs: vec![],
            by_ref: None,
            mutability: None,
            ident: arg_name.clone(),
            subpat: None,
        };
        args_destr.push_value(Pat::Ident(ident));
        args_destr.push_punct(Default::default());
    }

    if !with_args && !args_destr.is_empty() {
        return syn::Error::new(
            input.span(),
            format!("{} method cannot have arguments", method_type),
        )
        .to_compile_error()
        .into();
    }

    let return_lifetime = if parameters.is_trait || input.sig.asyncness.is_none() {
        quote! { #self_lifetime }
    } else {
        quote! { '_ }
    };

    if !has_self {
        return TokenStream::from(
            syn::Error::new(input.span(), "API method must have a `&self` argument")
                .to_compile_error(),
        );
    }

    let arg_type = TypeTuple {
        paren_token: Default::default(),
        elems: arg_types,
    };

    let args_destr_tuple = PatTuple {
        attrs: vec![],
        paren_token: Default::default(),
        elems: args_destr.clone(),
    };

    let is_async_return_type = if let ReturnType::Type(_, ty) = &input.sig.output {
        let extracted = crate::derive::extract_type_if_matches("AsyncReturn", ty);
        &**ty != extracted
    } else {
        false
    };

    let await_call = if input.sig.asyncness.is_some() {
        quote! { .await }
    } else {
        quote! {}
    };

    let await_call_if_result_is_async = if is_async_return_type {
        quote! { .await }
    } else {
        quote! {}
    };

    let export_function = if parameters.is_trait {
        let mut methods = METHODS_EXPORTS.lock().unwrap();
        methods.push(ExportMethodData {
            method_name,
            export_name,
            arg_count: args.len(),
            is_async: input.sig.asyncness.is_some(),
            is_return_type_async: is_async_return_type,
            return_type: match return_type {
                ReturnType::Default => ReturnVariant::Default,
                ReturnType::Type(_, t) => match t.as_ref() {
                    Type::Tuple(_) => ReturnVariant::Tuple,
                    _ => ReturnVariant::Type,
                },
            },
        });
        quote! {}
    } else {
        let args_destr_tuple = if with_args {
            quote! {
                let #args_destr_tuple: #arg_type = ::ic_cdk::api::call::arg_data();
            }
        } else {
            quote! {}
        };
        quote! {
            #[cfg(all(target_arch = "wasm32", not(feature = "no_api")))]
            #[export_name = #export_name]
            fn #internal_method() {
                ::ic_cdk::setup();
                ::ic_cdk::spawn(async {
                    #args_destr_tuple
                    let mut instance = Self::init_instance();
                    let result = instance. #method(#args_destr) #await_call #await_call_if_result_is_async;
                    #reply_call
                });
            }
        }
    };

    let expanded = quote! {
        #[allow(dead_code)]
        #input

        #export_function

        #[cfg(not(target_arch = "wasm32"))]
        #[allow(dead_code)]
        #orig_vis fn #internal_method<#self_lifetime>(#args) -> ::std::pin::Pin<Box<dyn ::core::future::Future<Output = ::ic_cdk::api::call::CallResult<#inner_return_type>> + #return_lifetime>> {
            // todo: trap handler
            let result = self. #method(#args_destr);
            Box::pin(async move { Ok(result #await_call) })
        }

        #[cfg(not(target_arch = "wasm32"))]
        #[allow(unused_mut)]
        #[allow(unused_must_use)]
        #orig_vis fn #internal_method_notify<#self_lifetime>(#args) -> ::std::result::Result<(), ::ic_cdk::api::call::RejectionCode> {
            // todo: trap handler
            self. #method(#args_destr);
            Ok(())
        }
    };

    TokenStream::from(expanded)
}

#[derive(Clone)]
enum ReturnVariant {
    Default,
    Type,
    Tuple,
}

#[derive(Clone)]
struct ExportMethodData {
    method_name: String,
    export_name: String,
    arg_count: usize,
    is_async: bool,
    is_return_type_async: bool,
    return_type: ReturnVariant,
}

lazy_static! {
    static ref METHODS_EXPORTS: Mutex<Vec<ExportMethodData>> = Mutex::new(Default::default());
}

struct GenerateExportsInput {
    trait_name: Ident,
    struct_name: Ident,
    struct_vis: Visibility,
}

impl Parse for GenerateExportsInput {
    fn parse(input: ParseStream) -> syn::Result<Self> {
        let trait_name = input.parse::<Ident>()?;
        let (struct_name, struct_vis) = if input.is_empty() {
            (
                Ident::new(&format!("__{}_Ident", trait_name.to_string()), input.span()),
                Visibility::Inherited,
            )
        } else {
            input.parse::<Token![,]>()?;
            (
                input.parse::<Ident>()?,
                Visibility::Public(VisPublic {
                    pub_token: Default::default(),
                }),
            )
        };

        Ok(Self {
            trait_name,
            struct_name,
            struct_vis,
        })
    }
}

pub(crate) fn generate_exports(input: TokenStream) -> TokenStream {
    let generate_input = parse_macro_input!(input as GenerateExportsInput);
    let GenerateExportsInput {
        trait_name,
        struct_name,
        struct_vis,
    } = generate_input;
    let methods = METHODS_EXPORTS.lock().unwrap();

    let methods = methods.iter().map(|method| {
        let owned: ExportMethodData = method.clone();
        let ExportMethodData { method_name, export_name, arg_count, is_async, is_return_type_async, return_type } = owned;

        let method = Ident::new(&method_name, Span::call_site());
        let internal_method = Ident::new(&format!("__{method}"), Span::call_site());

        // skip first argument as it is always self
        let (args_destr_tuple, args_destr) = if arg_count > 1 {
            let args: Vec<Ident> = (1..arg_count).map(|x| Ident::new(&format!("__arg_{x}"), Span::call_site())).collect();
            (
                quote! { let ( #(#args),* , ) = ::ic_cdk::api::call::arg_data(); },
                quote! { #(#args),* }
            )
        } else {
            (quote! {}, quote! {})
        };

        let await_call = if is_async { quote! {.await}} else {quote! {}};
        let await_call_if_result_is_async = if is_return_type_async { quote! {.await} } else {quote! {}};
        let reply_call = match return_type {
            ReturnVariant::Default => quote! { ::ic_cdk::api::call::reply(()); },
            ReturnVariant::Type => quote! {::ic_cdk::api::call::reply((result,)); },
            ReturnVariant::Tuple => quote! { ::ic_cdk::api::call::reply(result); },
        };

        quote! {
            #[cfg(all(target_arch = "wasm32", not(feature = "no_api")))]
            #[export_name = #export_name]
            fn #internal_method() {
                ::ic_cdk::setup();
                ::ic_cdk::spawn(async {
                    #args_destr_tuple
                    let mut instance = #struct_name ::init_instance();
                    let result = instance. #method(#args_destr) #await_call #await_call_if_result_is_async;

                    #reply_call
                });
            }
        }
    });

    let expanded = quote! {
        #[derive(::std::clone::Clone, ::ic_canister::Canister)]
        #[allow(non_camel_case_types)]
        #struct_vis struct #struct_name {
            #[id]
            principal: ::ic_cdk::export::Principal,
        }

        impl #trait_name for #struct_name {}

        #(#methods)*
    };
    expanded.into()
}

#[derive(Clone)]
pub struct Method {
    args: Vec<String>,
    rets: Vec<String>,
    modes: String,
}

// There is no official way to communicate information across proc macro invocations.
// lazy_static works for now, but may get incomplete info with incremental compilation.
// See https://github.com/rust-lang/rust/issues/44034
// Hopefully, we can have an attribute on impl, then we don't need global state.
lazy_static! {
    static ref METHODS: Mutex<BTreeMap<String, Method>> = Mutex::new(Default::default());
    static ref INIT: Mutex<Option<Vec<String>>> = Mutex::new(None);
}

fn store_candid_definitions(modes: &str, sig: &Signature) -> Result<(), syn::Error> {
    let name = sig.ident.to_string();

    let (args, rets) = get_args(sig)?;

    let args: Vec<String> = args
        .iter()
        .map(|t| format!("{}", t.to_token_stream()))
        .collect();

    let rets: Vec<String> = rets
        .iter()
        .map(|t| format!("{}", t.to_token_stream()))
        .collect();

    if modes == "oneway" && !rets.is_empty() {
        return Err(Error::new_spanned(
            &sig.output,
            "oneway function should have no return value",
        ));
    }

    // Insert init
    if modes == "init" && !rets.is_empty() {
        return Err(Error::new_spanned(
            &sig.output,
            "init method should have no return value or return Self",
        ));
    }

    if modes == "init" {
        match &mut *INIT.lock().unwrap() {
            Some(_) => return Err(Error::new_spanned(&sig.ident, "duplicate init method")),
            ret @ None => *ret = Some(args),
        }
        return Ok(());
    }

    if modes == "pre_upgrade" || modes == "post_upgrade" {
        return Ok(());
    }

    // Insert method
    let mut map = METHODS.lock().unwrap();

    if map.contains_key(&name) {
        return Err(Error::new_spanned(
            &name,
            format!("duplicate method name {name}"),
        ));
    }

    let method = Method {
        args,
        rets,
        modes: modes.to_string(),
    };

    map.insert(name, method);

    Ok(())
}

pub(crate) fn generate_idl() -> TokenStream {
    let candid = quote! { ::ic_cdk::export::candid };

    // Init
    let init = INIT.lock().unwrap().as_mut().map(|args| {
        let args = args
            .drain(..)
            .map(|t| generate_arg(quote! { init_args }, &t))
            .collect::<Vec<_>>();

        let res = quote! {
            let mut init_args = Vec::new();
            #(#args)*
        };

        res
    });

    let mut methods = METHODS.lock().unwrap();
    let gen_tys = methods.iter().map(|(name, Method { args, rets, modes })| {
        let args = args
            .iter()
            .map(|t| generate_arg(quote! { args }, t))
            .collect::<Vec<_>>();

        let rets = rets
            .iter()
            .map(|t| generate_arg(quote! { rets }, t))
            .collect::<Vec<_>>();

        let modes = match modes.as_ref() {
            "query" => quote! { vec![#candid::parser::types::FuncMode::Query] },
            "oneway" => quote! { vec![#candid::parser::types::FuncMode::Oneway] },
            "update" => quote! { vec![] },
            _ => unreachable!(),
        };

        quote! {
            {
                let mut args = Vec::new();
                #(#args)*
                let mut rets = Vec::new();
                #(#rets)*
                let func = Function { args, rets, modes: #modes };
                service.push((#name.to_string(), Type::Func(func)));
            }
        }
    });

    let service = quote! {
        use #candid::types::{CandidType, Function, Type};
        let mut service = Vec::<(String, Type)>::new();
        let mut env = #candid::types::internal::TypeContainer::new();
        #(#gen_tys)*
        service.sort_unstable_by_key(|(name, _)| name.clone());
        let ty = Type::Service(service);
    };

    methods.clear();

    let actor = match init {
        Some(init) => quote! {
            #init
            let actor = Type::Class(init_args, Box::new(ty));
        },
        None => quote! { let actor = ty; },
    };

    let res = quote! {
        {
            #service
            #actor
            ::ic_canister::Idl::new(env, actor)
        }
    };

    TokenStream::from(res)
}

fn generate_arg(name: proc_macro2::TokenStream, ty: &str) -> proc_macro2::TokenStream {
    let ty = syn::parse_str::<Type>(ty).unwrap();
    quote! {
        #name.push(env.add::<#ty>());
    }
}

fn get_args(sig: &Signature) -> Result<(Vec<Type>, Vec<Type>), Error> {
    let mut args = Vec::new();
    for arg in &sig.inputs {
        match arg {
            syn::FnArg::Receiver(r) => {
                if r.reference.is_none() {
                    return Err(Error::new_spanned(
                        arg,
                        "cannot take `self` by value, consider borrowing the value: `&self`",
                    ));
                }
            }
            syn::FnArg::Typed(syn::PatType { ty, .. }) => args.push(ty.as_ref().clone()),
        }
    }
    let rets = match &sig.output {
        ReturnType::Default => Vec::new(),
        ReturnType::Type(_, ty) => match ty.as_ref() {
            Type::Tuple(tuple) => tuple.elems.iter().cloned().collect(),
            ty => {
                // Some types in trait canisters had to be marked as `AsyncReturn` as implementation detail
                // but we do not need this when exporting them to candid files as ic calls them correctly
                // in any case.
                let extracted_type = crate::derive::extract_type_if_matches("AsyncReturn", ty);
                vec![extracted_type.clone()]
            }
        },
    };
    Ok((args, rets))
}
