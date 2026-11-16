//! `routes!`: an app's routes read from a folder, as Expo Router reads
//! `app/` (ark_router's crate docs say the conventions). At compile time it
//! walks the folder and writes:
//!
//! - the folder as modules (`app::tabs::budget::budget`, each file where it
//!   is), and each layout's screen at the crate's root by its name
//!   (`budget`, `shell`), where the rest of the crate knows it;
//! - the `Route` enum, a variant per route file — its struct's name, docs
//!   and fields, its path the file's — through `#[derive(Routes)]`, which
//!   writes the parsing and printing; `From` each route file's struct, and
//!   `Route::statics()`, every route a static export writes;
//! - with `type App = …`, the app's declaration (`ark_app::app!`): each
//!   layout's screen, the root's the shell and `(modals)`' over it; and
//!   `ark_app::host::Routed`, what a route does to the screens: the tab it
//!   is in selected, the screen that shows it told, a page pushed.
use std::path::{Path, PathBuf};

use proc_macro2::{Span, TokenStream};
use quote::{format_ident, quote};
use syn::parse::{Parse, ParseStream};
use syn::punctuated::Punctuated;
use syn::{Attribute, Error, Fields, Ident, Item, LitStr, Meta, Token, Type, Visibility};

/// `#[…] pub enum Route in "src/app"; #[…] pub type App = host::Money;`
pub struct Input {
    attrs: Vec<Attribute>,
    vis: Visibility,
    name: Ident,
    folder: LitStr,
    app: Option<(Vec<Attribute>, Visibility, Ident, syn::Path)>,
}

impl Parse for Input {
    fn parse(input: ParseStream) -> syn::Result<Input> {
        let attrs = input.call(Attribute::parse_outer)?;
        let vis = input.parse()?;
        input.parse::<Token![enum]>()?;
        let name = input.parse()?;
        input.parse::<Token![in]>()?;
        let folder = input.parse()?;
        input.parse::<Token![;]>()?;
        let app = match input.is_empty() {
            true => None,
            false => {
                let attrs = input.call(Attribute::parse_outer)?;
                let vis = input.parse()?;
                input.parse::<Token![type]>()?;
                let alias = input.parse()?;
                input.parse::<Token![=]>()?;
                let host = input.parse()?;
                input.parse::<Token![;]>()?;
                Some((attrs, vis, alias, host))
            }
        };
        Ok(Input {
            attrs,
            vis,
            name,
            folder,
            app,
        })
    }
}

/// A route file's field: its name, type, and what it is in the address.
struct Field {
    ident: Ident,
    ty: Type,
    hex: bool,
    slug: bool,
    query: Option<String>,
}

/// A route file, read.
struct RouteFile {
    /// From the folder: `(tabs)/(budget)/budget.rs`.
    rel: PathBuf,
    /// Its module, from the crate's root: `app::tabs::budget::budget`.
    module: Vec<Ident>,
    ident: Ident,
    docs: Vec<Attribute>,
    fields: Vec<Field>,
    unit: bool,
    pattern: String,
    shows: Option<String>,
    tab: Option<String>,
    params: bool,
}

/// A layout's screen.
struct ScreenDecl {
    /// From the folder: `(tabs)/(budget)`.
    dir: PathBuf,
    module: Vec<Ident>,
    name: String,
    ty: Ident,
    view: syn::Path,
    further: Vec<(Ident, syn::Path)>,
    shell: bool,
    over: bool,
    beside: bool,
}

#[derive(Default)]
struct Walk {
    routes: Vec<RouteFile>,
    screens: Vec<ScreenDecl>,
    /// `+not-found.rs`'s `Home`: the variant an address that names nothing
    /// lands on.
    home: Option<Ident>,
}

fn err(span: Span, msg: impl std::fmt::Display) -> Error {
    Error::new(span, msg)
}

/// A file or folder's name as a module's: `(tabs)` `tabs`, `[id].rs`
/// `id`, `+not-found.rs` `not_found`, `expanding-card` `expanding_card`.
fn module_name(fs: &str) -> String {
    let stem = fs.strip_suffix(".rs").unwrap_or(fs);
    stem.trim_start_matches('+')
        .trim_matches(|c| matches!(c, '(' | ')' | '[' | ']'))
        .replace('-', "_")
}

fn is_group(fs: &str) -> bool {
    fs.starts_with('(') && fs.ends_with(')')
}

/// `[id]` → `id`.
fn param_of(fs: &str) -> Option<&str> {
    fs.strip_prefix('[')?.strip_suffix(']')
}

fn attr_is(a: &Attribute, name: &str) -> bool {
    a.path().segments.last().is_some_and(|s| s.ident == name)
}

/// What a route's struct says of itself, and its fields'.
fn read_route(rel: &Path, module: Vec<Ident>, file: &syn::File, span: Span) -> syn::Result<RouteFile> {
    let derives_route = |s: &syn::ItemStruct| {
        s.attrs.iter().any(|a| {
            attr_is(a, "derive")
                && a.parse_args_with(Punctuated::<syn::Path, Token![,]>::parse_terminated)
                    .is_ok_and(|ps| ps.iter().any(|p| p.segments.last().is_some_and(|s| s.ident == "Route")))
        })
    };
    let Some(s) = file.items.iter().find_map(|i| match i {
        Item::Struct(s) if derives_route(s) => Some(s),
        _ => None,
    }) else {
        return Err(err(
            span,
            format!(
                "{} is a route: it says so with `#[derive(ark_router::Route)] pub struct …`",
                rel.display()
            ),
        ));
    };
    let mut shows = None;
    let mut tab = None;
    for a in &s.attrs {
        if attr_is(a, "shows") {
            shows = Some(a.parse_args::<Ident>()?.to_string());
        } else if attr_is(a, "tab") {
            match &a.meta {
                Meta::NameValue(nv) => match &nv.value {
                    syn::Expr::Lit(syn::ExprLit { lit: syn::Lit::Str(l), .. }) => tab = Some(l.value()),
                    _ => return Err(err(span, "`#[tab = \"…\"]` names the tab, its fields in braces")),
                },
                _ => return Err(err(span, "`#[tab = \"…\"]` names the tab, its fields in braces")),
            }
        }
    }
    let docs = s.attrs.iter().filter(|a| attr_is(a, "doc")).cloned().collect();
    let (fields, unit) = match &s.fields {
        Fields::Unit => (vec![], true),
        Fields::Unnamed(_) => return Err(err(span, format!("{}: a route's fields are named, as its path's are", rel.display()))),
        Fields::Named(n) => {
            let mut out = vec![];
            for f in &n.named {
                let mut field = Field {
                    ident: f.ident.clone().unwrap(),
                    ty: f.ty.clone(),
                    hex: false,
                    slug: false,
                    query: None,
                };
                for a in &f.attrs {
                    if attr_is(a, "hex") {
                        field.hex = true;
                    } else if attr_is(a, "slug") {
                        field.slug = true;
                    } else if attr_is(a, "query") {
                        field.query = Some(match &a.meta {
                            Meta::NameValue(nv) => match &nv.value {
                                syn::Expr::Lit(syn::ExprLit { lit: syn::Lit::Str(l), .. }) => l.value(),
                                _ => return Err(err(span, "`#[query = \"key\"]` names the key")),
                            },
                            _ => field.ident.to_string().replace('_', "-"),
                        });
                    }
                }
                out.push(field);
            }
            (out, n.named.is_empty())
        }
    };
    // The path: each folder but a group, the file but an index; `[x]` the
    // field `x`, a row's id where it is `#[hex]`.
    let mut segs = vec![];
    let parts: Vec<String> = rel.iter().map(|p| p.to_string_lossy().into_owned()).collect();
    for (n, part) in parts.iter().enumerate() {
        let last = n + 1 == parts.len();
        let part = match last {
            true => part.strip_suffix(".rs").unwrap_or(part),
            false => part.as_str(),
        };
        if is_group(part) || (last && part == "index") {
            continue;
        }
        match param_of(part) {
            Some(p) => {
                let Some(f) = fields.iter().find(|f| f.ident == p) else {
                    return Err(err(span, format!("{}: `{}` has no field `{p}`", rel.display(), s.ident)));
                };
                segs.push(format!(":{p}{}", if f.hex { "(hex)" } else { "" }));
            }
            None => segs.push(part.to_string()),
        }
    }
    let slugs: Vec<&Field> = fields.iter().filter(|f| f.slug).collect();
    if let [slug] = slugs[..] {
        segs.push(format!("~{}", slug.ident));
    } else if slugs.len() > 1 {
        return Err(err(span, format!("{}: one slug, last", rel.display())));
    }
    let mut pattern = format!("/{}", segs.join("/"));
    let query: Vec<String> = fields
        .iter()
        .filter_map(|f| f.query.as_ref().map(|k| format!("{k}=:{}", f.ident)))
        .collect();
    if !query.is_empty() {
        pattern = format!("{pattern}?{}", query.join("&"));
    }
    let ident = s.ident.clone();
    let params = file.items.iter().any(|i| match i {
        Item::Impl(im) => {
            matches!(&*im.self_ty, Type::Path(p) if p.path.is_ident(&ident))
                && im.items.iter().any(|it| matches!(it, syn::ImplItem::Fn(f) if f.sig.ident == "params"))
        }
        _ => false,
    });
    Ok(RouteFile {
        rel: rel.to_path_buf(),
        module,
        ident,
        docs,
        fields,
        unit,
        pattern,
        shows,
        tab,
        params,
    })
}

/// A layout's screen: the item marked `#[ark_router::screen(view = …)]`.
fn read_layout(rel_dir: &Path, module: Vec<Ident>, file: &syn::File, span: Span) -> syn::Result<ScreenDecl> {
    let marked = file.items.iter().find_map(|i| {
        let (attrs, ident) = match i {
            Item::Struct(s) => (&s.attrs, &s.ident),
            Item::Enum(e) => (&e.attrs, &e.ident),
            _ => return None,
        };
        attrs.iter().find(|a| attr_is(a, "screen")).map(|a| (a, ident))
    });
    let shown = || match rel_dir.as_os_str().is_empty() {
        true => "_layout.rs".to_string(),
        false => format!("{}/_layout.rs", rel_dir.display()),
    };
    let Some((attr, ty)) = marked else {
        return Err(err(
            span,
            format!("{} is a layout: its screen is marked `#[ark_router::screen(view = …)]`", shown()),
        ));
    };
    let shell = rel_dir.as_os_str().is_empty();
    let dir_name = rel_dir.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    let over = dir_name == "(modals)";
    let mut name = match () {
        _ if shell => "shell".to_string(),
        _ if over => "sheets".to_string(),
        _ => module_name(&dir_name),
    };
    let mut view = None;
    let mut further = vec![];
    let mut beside = false;
    for m in attr.parse_args_with(Punctuated::<Meta, Token![,]>::parse_terminated)? {
        match m {
            Meta::Path(p) if p.is_ident("beside") => beside = true,
            Meta::NameValue(nv) if nv.path.is_ident("name") => match &nv.value {
                syn::Expr::Lit(syn::ExprLit { lit: syn::Lit::Str(l), .. }) => name = l.value(),
                _ => return Err(err(span, "`name = \"…\"`: the screen's name")),
            },
            Meta::NameValue(nv) => {
                let syn::Expr::Path(p) = &nv.value else {
                    return Err(err(span, "a view is a path: `view = view::budget`"));
                };
                let key = nv.path.get_ident().cloned().ok_or_else(|| err(span, "a view's name is one word"))?;
                match key == "view" {
                    true => view = Some(p.path.clone()),
                    false => further.push((key, p.path.clone())),
                }
            }
            _ => return Err(err(span, "`#[screen(view = …, name = \"…\", beside, <further> = …)]`")),
        }
    }
    let view = view.ok_or_else(|| err(span, format!("{}: the screen's `view = …`", shown())))?;
    Ok(ScreenDecl {
        dir: rel_dir.to_path_buf(),
        module,
        name,
        ty: ty.clone(),
        view,
        further,
        shell,
        over,
        beside,
    })
}

/// The `Home` that `+not-found.rs` re-exports: `pub use crate::Route::Budget as Home;`.
fn read_not_found(file: &syn::File, span: Span) -> syn::Result<Ident> {
    fn find(tree: &syn::UseTree) -> Option<Ident> {
        match tree {
            syn::UseTree::Path(p) => match &*p.tree {
                syn::UseTree::Rename(r) if r.rename == "Home" => Some(r.ident.clone()),
                t => find(t),
            },
            syn::UseTree::Rename(r) if r.rename == "Home" => Some(r.ident.clone()),
            _ => None,
        }
    }
    file.items
        .iter()
        .find_map(|i| match i {
            Item::Use(u) => find(&u.tree),
            _ => None,
        })
        .ok_or_else(|| {
            err(
                span,
                "+not-found.rs names where a path that names nothing lands: `pub use crate::Route::Budget as Home;`",
            )
        })
}

/// The folder at `dir` (`rel` from its root) as modules, read into `walk`.
fn walk(root: &Path, rel: &Path, module: &[Ident], w: &mut Walk, span: Span) -> syn::Result<TokenStream> {
    let dir = root.join(rel);
    let mut names: Vec<String> = std::fs::read_dir(&dir)
        .map_err(|e| err(span, format!("{}: {e}", dir.display())))?
        .filter_map(|e| e.ok())
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .collect();
    names.sort();
    let mut out = TokenStream::new();
    let mut taken: Vec<String> = vec![];
    for fs in &names {
        let path = dir.join(fs);
        let is_dir = path.is_dir();
        // `x.rs` and `x/` together: the folder is the file's own modules.
        if is_dir && names.contains(&format!("{fs}.rs")) {
            continue;
        }
        if !is_dir && !fs.ends_with(".rs") {
            continue;
        }
        let name = module_name(fs);
        if taken.contains(&name) {
            return Err(err(span, format!("{}: two of `{name}` in one folder", rel.join(fs).display())));
        }
        taken.push(name.clone());
        let ident = format_ident!("{}", name);
        let mut here = module.to_vec();
        here.push(ident.clone());
        let fs_lit = LitStr::new(fs, span);
        if is_dir {
            let inner = walk(root, &rel.join(fs), &here, w, span)?;
            out.extend(quote! {
                #[path = #fs_lit]
                pub mod #ident { #inner }
            });
            continue;
        }
        let text = std::fs::read_to_string(&path).map_err(|e| err(span, format!("{}: {e}", path.display())))?;
        let file = syn::parse_file(&text).map_err(|e| err(span, format!("{}: {e}", rel.join(fs).display())))?;
        match fs.as_str() {
            "_layout.rs" => w.screens.push(read_layout(rel, here, &file, span)?),
            "+not-found.rs" => w.home = Some(read_not_found(&file, span)?),
            f if f.starts_with('_') || f.starts_with('+') => {
                return Err(err(
                    span,
                    format!("{}: a file here is a route, `_layout.rs` or `+not-found.rs`", rel.join(fs).display()),
                ))
            }
            _ => w.routes.push(read_route(&rel.join(fs), here, &file, span)?),
        }
        // A file whose name is its module's loads as any module does (its
        // own modules in the folder of its name); any other by its path.
        out.extend(match fs.strip_suffix(".rs") == Some(name.as_str()) {
            true => quote! { pub mod #ident; },
            false => quote! {
                #[path = #fs_lit]
                pub mod #ident;
            },
        });
    }
    Ok(out)
}

fn path_of(idents: &[Ident]) -> TokenStream {
    quote!(#(#idents)::*)
}

pub fn routes(input: Input) -> syn::Result<TokenStream> {
    let span = input.folder.span();
    let folder = input.folder.value();
    let manifest = std::env::var("CARGO_MANIFEST_DIR").map_err(|_| err(span, "routes! reads its folder from the crate's directory"))?;
    let root = Path::new(&manifest).join(&folder);
    let top = root
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .ok_or_else(|| err(span, "the routes' folder, from the crate's directory: \"src/app\""))?;
    let top_ident = format_ident!("{}", module_name(&top));
    // The folder from the file the macro is in, which is beside it (the
    // crate's root, `src/lib.rs`, beside `src/app`).
    let from_root = LitStr::new(&top, span);
    let mut w = Walk::default();
    let tree = walk(&root, Path::new(""), std::slice::from_ref(&top_ident), &mut w, span)?;

    // A path's own words before a field in the same place (`/about/`
    // before `/:name`), as Expo Router ranks them: the derive reads its
    // variants in order. Otherwise the folder's.
    let rank = |r: &RouteFile| -> Vec<u8> {
        let path = r.pattern.split('?').next().unwrap_or_default();
        path.split('/')
            .filter(|p| !p.is_empty() && !p.starts_with('~'))
            .map(|p| u8::from(p.starts_with(':')))
            .collect()
    };
    w.routes.sort_by_key(|r| rank(r));
    let name = &input.name;
    let vis = &input.vis;
    let attrs = &input.attrs;
    let home = match &w.home {
        Some(h) => h.clone(),
        None => w
            .routes
            .iter()
            .find(|r| r.pattern == "/")
            .map(|r| r.ident.clone())
            .ok_or_else(|| err(span, "no home: an `index.rs` at the folder's root, or a `+not-found.rs` that names one"))?,
    };
    if !w.routes.iter().any(|r| r.ident == home && r.unit) {
        return Err(err(
            span,
            format!("`{home}`, where a path that names nothing lands, is a route that carries nothing"),
        ));
    }
    for (i, r) in w.routes.iter().enumerate() {
        if let Some(o) = w.routes[..i].iter().find(|o| o.ident == r.ident || o.pattern == r.pattern) {
            return Err(err(span, format!("{} and {} are one route", o.rel.display(), r.rel.display())));
        }
    }

    // The enum, through the derive.
    let variants = w.routes.iter().map(|r| {
        let ident = &r.ident;
        let docs = &r.docs;
        let pattern = LitStr::new(&r.pattern, span);
        let mark = (*ident == home).then(|| quote!(#[home]));
        let fields = r.fields.iter().map(|f| {
            let (i, t) = (&f.ident, &f.ty);
            quote!(#i: #t)
        });
        let body = match r.unit {
            true => quote!(),
            false => quote!({ #(#fields,)* }),
        };
        quote! {
            #(#docs)*
            #mark
            #[route(#pattern)]
            #ident #body,
        }
    });
    let froms = w.routes.iter().map(|r| {
        let ident = &r.ident;
        let ty = path_of(&r.module);
        match r.unit {
            true => quote! {
                impl ::core::convert::From<#ty::#ident> for #name {
                    fn from(_: #ty::#ident) -> #name {
                        #name::#ident
                    }
                }
            },
            false => {
                let fs: Vec<&Ident> = r.fields.iter().map(|f| &f.ident).collect();
                quote! {
                    impl ::core::convert::From<#ty::#ident> for #name {
                        fn from(r: #ty::#ident) -> #name {
                            #name::#ident { #(#fs: r.#fs,)* }
                        }
                    }
                }
            }
        }
    });
    let statics = w.routes.iter().map(|r| {
        let ident = &r.ident;
        let ty = path_of(&r.module);
        match (r.unit, r.params) {
            (true, _) => quote!(out.push(#name::#ident);),
            (false, true) => quote!(out.extend(#ty::#ident::params().into_iter().map(#name::from));),
            (false, false) => quote!(),
        }
    });

    let mut out = quote! {
        #[path = #from_root]
        pub mod #top_ident { #tree }

        #(#attrs)*
        #[derive(::ark_router::Routes)]
        #vis enum #name { #(#variants)* }

        #(#froms)*

        impl #name {
            /// Every route a static export writes a page of: each that
            /// carries nothing, and each a route file lists (its
            /// `params()`), in the folder's order.
            pub fn statics() -> ::std::vec::Vec<#name> {
                let mut out = ::std::vec::Vec::new();
                #(#statics)*
                out
            }
        }
    };

    let Some((app_attrs, app_vis, alias, host)) = &input.app else {
        if !w.screens.is_empty() {
            return Err(err(span, "a folder with layouts declares its app: `pub type App = host::Money;`"));
        }
        return Ok(out);
    };

    // Each screen at the crate's root by its name; the app's declaration.
    let mut order: Vec<&ScreenDecl> = w.screens.iter().filter(|s| !s.shell && !s.over).collect();
    order.extend(w.screens.iter().filter(|s| s.over));
    order.extend(w.screens.iter().filter(|s| s.shell));
    let mut decls = vec![];
    for s in &order {
        let n = format_ident!("{}", s.name);
        let module = path_of(&s.module);
        out.extend(quote! {
            pub use #module as #n;
        });
        let ty = &s.ty;
        let view = &s.view;
        let dir = match s.dir.as_os_str().is_empty() {
            true => format!("{folder}/_layout"),
            false => format!("{folder}/{}/_layout", s.dir.display()),
        };
        let dir = LitStr::new(&dir, span);
        let mark = match () {
            _ if s.shell => quote!(#[shell]),
            _ if s.over => quote!(#[over]),
            _ if s.beside => quote!(#[beside]),
            _ => quote!(),
        };
        let further = match s.further.is_empty() {
            true => quote!(),
            false => {
                let fs = s.further.iter().map(|(k, p)| quote!(#k: #n::#p));
                quote!({ #(#fs,)* })
            }
        };
        decls.push(quote! {
            #mark
            #n: #n::#ty = #n::#view in #dir #further
        });
    }
    out.extend(quote! {
        ::ark_app::app! {
            #(#app_attrs)*
            #app_vis type #alias = #host {
                #(#decls,)*
            }
        }
    });

    // What each route does to the screens.
    let Some(shell) = w.screens.iter().find(|s| s.shell) else {
        return Ok(out);
    };
    let screen_ty = |s: &ScreenDecl| {
        let n = format_ident!("{}", s.name);
        let ty = &s.ty;
        quote!(#n::#ty)
    };
    let shell_ty = screen_ty(shell);
    let mut arms = vec![];
    for r in &w.routes {
        let ident = &r.ident;
        // The screen it shows: its own word, else its nearest layout's.
        let screen = match &r.shows {
            Some(n) => w
                .screens
                .iter()
                .find(|s| s.name == *n)
                .ok_or_else(|| err(span, format!("{}: no screen is named `{n}`", r.rel.display())))?,
            None => {
                let mut dir = r.rel.parent().map(Path::to_path_buf).unwrap_or_default();
                loop {
                    if let Some(s) = w.screens.iter().find(|s| s.dir == dir) {
                        break s;
                    }
                    if !dir.pop() {
                        break shell;
                    }
                }
            }
        };
        if screen.beside {
            return Err(err(span, format!("{}: a screen beside its host shows no route", r.rel.display())));
        }
        // Its tab: its own word, else the first folder under `(tabs)`.
        let parts: Vec<String> = r.rel.iter().map(|p| p.to_string_lossy().into_owned()).collect();
        let tab = r.tab.clone().or_else(|| {
            let at = parts.iter().position(|p| p == "(tabs)")?;
            parts.get(at + 1).map(|p| module_name(p).replace('_', "-"))
        });
        let pushed = tab.is_none() && !screen.shell && !screen.over;
        if tab.is_none() && screen.shell {
            continue;
        }
        let select = tab.map(|t| {
            let lit = LitStr::new(&t, span);
            quote! {
                let m = <#shell_ty as ::ark_app::shell::Navigates>::select(::std::format!(#lit));
                <Self as ::ark_app::host::Has<#shell_ty>>::screen_mut(self).send(m, cx);
            }
        });
        let ty = screen_ty(screen);
        let shown = quote! {
            if let ::core::option::Option::Some(m) = <#ty as ::ark_app::App>::shown(&route) {
                <Self as ::ark_app::host::Has<#ty>>::screen_mut(self).send(m, cx);
            }
        };
        let push = pushed.then(|| {
            quote! {
                if let ::core::option::Option::Some(m) = <#shell_ty as ::ark_app::shell::Navigates>::push(&route) {
                    <Self as ::ark_app::host::Has<#shell_ty>>::screen_mut(self).send(m, cx);
                }
            }
        });
        let pat = match r.unit {
            true => quote!(#name::#ident),
            false => {
                let fs = r.fields.iter().map(|f| &f.ident);
                quote!(#name::#ident { #(#fs,)* })
            }
        };
        arms.push(quote! {
            #pat => {
                #select
                #shown
                #push
                true
            }
        });
    }
    out.extend(quote! {
        impl ::ark_app::host::Routed for #host {
            #[allow(unused_variables, clippy::match_single_binding)]
            fn routed(&mut self, route: #name, cx: &mut ::ark_app::host::Cx<'_, Self>) -> ::core::option::Option<#name> {
                let shown = match &route {
                    #(#arms)*
                    _ => false,
                };
                match shown {
                    true => ::core::option::Option::None,
                    false => ::core::option::Option::Some(route),
                }
            }
        }
    });
    Ok(out)
}
