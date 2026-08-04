//! The `#[op]` attribute macro.
//!
//! Resolves the mechanism question in ticket #9: one annotated async function becomes a
//! CLI subcommand, an MCP tool, and an HTTP route, with no central registration list.
//!
//! ```ignore
//! /// Report host readiness.
//! #[op]
//! pub async fn doctor(ctx: &Ctx, args: DoctorArgs) -> anyhow::Result<DoctorReport> { … }
//!
//! /// Fetch a URL into the store.
//! #[op(long_running)]
//! pub async fn ingest(ctx: &Ctx, args: IngestArgs, p: &Progress) -> anyhow::Result<IngestOut> { … }
//! ```
//!
//! **Why a proc macro rather than build-time codegen or a runtime registry.** The
//! competing mechanisms were weighed in #9. Codegen puts generated source in the tree
//! and makes the definition site not the source of truth. A runtime registry needs an
//! explicit `register(…)` call per op — exactly the central list this avoids, and
//! exactly the thing people forget. Link-time submission via `inventory` keeps the
//! annotation adjacent to the function and makes forgetting impossible.
//!
//! *Accepted cost:* proc macros degrade error messages. Mitigated by keeping expansion
//! thin — the macro emits four small functions and a registry entry, and every real
//! behaviour stays in ordinary code that the compiler can talk about normally.

use proc_macro::TokenStream;
use quote::{format_ident, quote};
use syn::{FnArg, ItemFn, LitBool, LitStr, ReturnType, Type, spanned::Spanned};

#[derive(Default)]
struct OpAttr {
    name: Option<String>,
    about: Option<String>,
    group: Option<(String, proc_macro2::Span)>,
    long_running: bool,
    mcp: Option<bool>,
    local_only: bool,
}

/// Registers an async function as a Centinel op.
///
/// # Signature
///
/// ```ignore
/// async fn f(ctx: &Ctx, args: A) -> anyhow::Result<O>
/// async fn f(ctx: &Ctx, args: A, progress: &Progress) -> anyhow::Result<O>
/// ```
///
/// `A` must derive `clap::Args`, `serde::Serialize`, `serde::Deserialize` and
/// `schemars::JsonSchema`. `O` must derive `serde::Serialize` and `serde::Deserialize`,
/// and must implement [`centinel_core::render::Render`] — the CLI renders reports rather
/// than printing their JSON at a person, and there is no structural fallback to hide
/// behind. Writing `-> anyhow::Result<O>` with a named `O` is therefore required; a
/// `impl Trait` or aliased return type cannot be given a renderer.
///
/// # Options
///
/// - `name = "…"` — override the derived kebab-case name
/// - `about = "…"` — override the description (defaults to the first doc-comment line)
/// - `group = "…"` — which heading this op lists under in `centinel --help`:
///   `pipeline`, `stage`, `corpus` (the default) or `host`
/// - `long_running` — hint that surfaces should stream progress
/// - `mcp = false` — exclude from the MCP tool list while keeping CLI and HTTP
/// - `local_only` — act on the host; exclude from **both** MCP and HTTP
#[proc_macro_attribute]
pub fn op(attr: TokenStream, item: TokenStream) -> TokenStream {
    let mut parsed = OpAttr::default();
    let attr_parser = syn::meta::parser(|meta| {
        if meta.path.is_ident("name") {
            parsed.name = Some(meta.value()?.parse::<LitStr>()?.value());
        } else if meta.path.is_ident("about") {
            parsed.about = Some(meta.value()?.parse::<LitStr>()?.value());
        } else if meta.path.is_ident("group") {
            let lit = meta.value()?.parse::<LitStr>()?;
            parsed.group = Some((lit.value(), lit.span()));
        } else if meta.path.is_ident("long_running") {
            // Bare flag, or `long_running = true`.
            parsed.long_running = match meta.value() {
                Ok(v) => v.parse::<LitBool>()?.value(),
                Err(_) => true,
            };
        } else if meta.path.is_ident("local_only") {
            parsed.local_only = match meta.value() {
                Ok(v) => v.parse::<LitBool>()?.value(),
                Err(_) => true,
            };
        } else if meta.path.is_ident("mcp") {
            parsed.mcp = Some(match meta.value() {
                Ok(v) => v.parse::<LitBool>()?.value(),
                Err(_) => true,
            });
        } else {
            return Err(meta.error(
                "unknown `op` option; expected one of: name, about, group, long_running, mcp, \
                 local_only",
            ));
        }
        Ok(())
    });
    syn::parse_macro_input!(attr with attr_parser);

    let func = syn::parse_macro_input!(item as ItemFn);
    match expand(parsed, func) {
        Ok(ts) => ts.into(),
        Err(e) => e.to_compile_error().into(),
    }
}

fn expand(attr: OpAttr, func: ItemFn) -> syn::Result<proc_macro2::TokenStream> {
    let sig = &func.sig;

    if sig.asyncness.is_none() {
        return Err(syn::Error::new(
            sig.span(),
            "#[op] requires an `async fn` — every surface awaits the result",
        ));
    }

    let arity = sig.inputs.len();
    if arity != 2 && arity != 3 {
        return Err(syn::Error::new(
            sig.inputs.span(),
            "#[op] expects `(ctx: &Ctx, args: A)` or `(ctx: &Ctx, args: A, progress: &Progress)`",
        ));
    }

    // The second parameter's type is the argument struct — the single schema source
    // that clap, MCP and HTTP all read.
    let args_ty = match &sig.inputs[1] {
        FnArg::Typed(pt) => (*pt.ty).clone(),
        FnArg::Receiver(r) => {
            return Err(syn::Error::new(
                r.span(),
                "#[op] cannot be used on methods; ops are free functions",
            ));
        }
    };

    // The report type, dug out of `-> anyhow::Result<O>`. The CLI needs to name `O` to
    // put the concrete type back on before rendering; the other two surfaces never do.
    let out_ty = report_type(&sig.output)?;

    let fn_ident = sig.ident.clone();
    let name = attr
        .name
        .unwrap_or_else(|| fn_ident.to_string().replace('_', "-"));
    let about = attr
        .about
        .or_else(|| doc_summary(&func))
        .unwrap_or_else(|| name.clone());
    let long_running = attr.long_running;
    let mcp = attr.mcp.unwrap_or(true);
    let local_only = attr.local_only;
    let group = group_variant(attr.group)?;

    // A private module per op keeps four generated helpers out of the parent namespace
    // while still letting `use super::*` see the function and its argument type.
    let mod_ident = format_ident!("__centinel_op_{}", fn_ident);

    let call = if arity == 3 {
        quote! { super::#fn_ident(&__ctx, __args, &__progress).await? }
    } else {
        quote! { super::#fn_ident(&__ctx, __args).await? }
    };

    // `progress` is genuinely unused in the 2-arity case; bind it away at the call site
    // rather than blanket-allowing unused variables in generated code.
    let bind_progress = if arity == 3 {
        quote! {}
    } else {
        quote! { let _ = &__progress; }
    };

    Ok(quote! {
        #func

        #[doc(hidden)]
        mod #mod_ident {
            #[allow(unused_imports)]
            use super::*;

            use ::centinel_core::op::__private as __p;

            fn __augment(cmd: __p::clap::Command) -> __p::clap::Command {
                <#args_ty as __p::clap::Args>::augment_args(cmd)
            }

            fn __from_matches(
                m: &__p::clap::ArgMatches,
            ) -> __p::anyhow::Result<__p::serde_json::Value> {
                __p::args_to_json::<#args_ty>(m)
            }

            fn __schema() -> __p::serde_json::Value {
                __p::schema_of::<#args_ty>()
            }

            fn __render(
                __value: &__p::serde_json::Value,
                __p_out: &mut __p::Painter<'_>,
            ) -> __p::anyhow::Result<()> {
                __p::render_as::<#out_ty>(__value, __p_out)
            }

            fn __invoke(
                __ctx: ::std::sync::Arc<__p::Ctx>,
                __args_json: __p::serde_json::Value,
                __progress: __p::Progress,
            ) -> __p::futures::future::BoxFuture<
                'static,
                __p::anyhow::Result<__p::serde_json::Value>,
            > {
                ::std::boxed::Box::pin(async move {
                    #bind_progress
                    let __args: #args_ty = __p::serde_json::from_value(__args_json)
                        .map_err(|e| __p::anyhow::anyhow!(
                            "invalid arguments for op `{}`: {}", #name, e
                        ))?;
                    let __out = #call;
                    ::std::result::Result::Ok(__p::serde_json::to_value(__out)?)
                })
            }

            __p::inventory::submit! {
                __p::OpDef {
                    name: #name,
                    about: #about,
                    group: __p::Group::#group,
                    long_running: #long_running,
                    mcp: #mcp,
                    local_only: #local_only,
                    augment_clap: __augment,
                    args_from_matches: __from_matches,
                    schema: __schema,
                    invoke: __invoke,
                    render: __render,
                }
            }
        }
    })
}

/// Maps `group = "…"` to a `centinel_core::op::Group` variant.
///
/// Resolved here rather than passed through as a string so a typo is a compile error at
/// the annotation, naming the four headings. A misfiled op is only cosmetic, but the
/// cost of catching it is one match arm.
fn group_variant(group: Option<(String, proc_macro2::Span)>) -> syn::Result<proc_macro2::Ident> {
    let Some((name, span)) = group else {
        // An op that does not say is one that reads the corpus — the common addition,
        // and the heading where an unclassified verb is least surprising.
        return Ok(format_ident!("Corpus"));
    };
    let variant = match name.as_str() {
        "pipeline" => "Pipeline",
        "stage" => "Stage",
        "corpus" => "Corpus",
        "host" => "Host",
        other => {
            return Err(syn::Error::new(
                span,
                format!(
                    "unknown op group `{other}`; expected one of: pipeline, stage, corpus, host"
                ),
            ));
        }
    };
    Ok(format_ident!("{}", variant))
}

/// Extracts `O` from `-> anyhow::Result<O>`.
///
/// Matched structurally on the last path segment's first type argument rather than on the
/// literal text `anyhow::Result`, so `Result<O>`, `anyhow::Result<O>` and a crate-local
/// alias all work. The error message names the constraint rather than the parse failure,
/// because "your return type is unusual" is not what an author needs to hear.
fn report_type(output: &ReturnType) -> syn::Result<Type> {
    let unsupported = |span| {
        syn::Error::new(
            span,
            "#[op] needs a named report type: write `-> anyhow::Result<MyReport>`. \
             The CLI renders `MyReport` for a person, so the type has to be nameable.",
        )
    };

    let ReturnType::Type(_, ty) = output else {
        return Err(unsupported(output.span()));
    };
    let Type::Path(path) = &**ty else {
        return Err(unsupported(ty.span()));
    };
    let last = path
        .path
        .segments
        .last()
        .ok_or_else(|| unsupported(ty.span()))?;
    let syn::PathArguments::AngleBracketed(args) = &last.arguments else {
        return Err(unsupported(ty.span()));
    };
    args.args
        .iter()
        .find_map(|a| match a {
            syn::GenericArgument::Type(t) => Some(t.clone()),
            _ => None,
        })
        .ok_or_else(|| unsupported(ty.span()))
}

/// Uses the first non-empty doc-comment line as the description.
///
/// Sourcing the CLI help and the MCP tool description from the doc comment means the
/// three surfaces cannot drift from each other, or from the code.
fn doc_summary(func: &ItemFn) -> Option<String> {
    for attr in &func.attrs {
        if !attr.path().is_ident("doc") {
            continue;
        }
        let syn::Meta::NameValue(nv) = &attr.meta else {
            continue;
        };
        let syn::Expr::Lit(syn::ExprLit {
            lit: syn::Lit::Str(s),
            ..
        }) = &nv.value
        else {
            continue;
        };
        let line = s.value().trim().to_string();
        if !line.is_empty() {
            return Some(line);
        }
    }
    None
}
