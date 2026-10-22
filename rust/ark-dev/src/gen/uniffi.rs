//! UniFFI's own Kotlin and Swift for a domain's twins (`docs/plan-v5.md`,
//! Landed G1), with no library built: the metadata the twins' derives
//! would embed (`ark-macros/src/ffi.rs`: a record or enum per twin, the
//! `<T>View` object with its constructor, `rows` and `take`, and
//! `<T>Taken`) is made from the source ([`metadata`]), encoded as UniFFI's
//! macros encode it ([`encode`]: what its checksums are of), laid in a
//! library file's symbols as a built library holds them ([`library`]), and
//! handed to UniFFI's generators ([`bindings`]), which read it as they read
//! any library's.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use camino::Utf8PathBuf;
pub use uniffi_meta as meta;

use super::source::{Crosses, Shape, Twin};
use uniffi_bindgen::bindings::{generate_with_bindgen_paths, GenerateOptions, TargetLanguage};
use uniffi_bindgen::{BindgenPaths, BindgenPathsLayer};
use uniffi_meta::{
    ConstructorMetadata, CustomTypeMetadata, EnumMetadata, EnumShape, FieldMetadata, FnMetadata, FnParamMetadata, Metadata, MethodMetadata,
    NamespaceMetadata, ObjectImpl, ObjectMetadata, RecordMetadata, TraitKind, TraitMethodMetadata, Type, VariantMetadata,
};

/// The metadata codes UniFFI's macros write (`uniffi_core::metadata::codes`).
mod codes {
    pub const FUNC: u8 = 0;
    pub const METHOD: u8 = 1;
    pub const RECORD: u8 = 2;
    pub const ENUM: u8 = 3;
    pub const INTERFACE: u8 = 4;
    pub const NAMESPACE: u8 = 6;
    pub const CONSTRUCTOR: u8 = 7;
    pub const TRAIT_METHOD: u8 = 10;
    pub const TRAIT_INTERFACE: u8 = 12;
    pub const CUSTOM_TYPE: u8 = 15;
    pub const TYPE_TRAIT_INTERFACE: u8 = 24;
    pub const TYPE_U8: u8 = 0;
    pub const TYPE_U16: u8 = 1;
    pub const TYPE_U32: u8 = 2;
    pub const TYPE_U64: u8 = 3;
    pub const TYPE_I8: u8 = 4;
    pub const TYPE_I16: u8 = 5;
    pub const TYPE_I32: u8 = 6;
    pub const TYPE_I64: u8 = 7;
    pub const TYPE_F32: u8 = 8;
    pub const TYPE_F64: u8 = 9;
    pub const TYPE_BOOL: u8 = 10;
    pub const TYPE_STRING: u8 = 11;
    pub const TYPE_OPTION: u8 = 12;
    pub const TYPE_RECORD: u8 = 13;
    pub const TYPE_ENUM: u8 = 14;
    pub const TYPE_INTERFACE: u8 = 16;
    pub const TYPE_VEC: u8 = 17;
    pub const TYPE_CUSTOM: u8 = 22;
    pub const TYPE_RESULT: u8 = 23;
    pub const TYPE_BOX: u8 = 26;
    pub const TYPE_UNIT: u8 = 255;
}

fn kind(k: TraitKind) -> u8 {
    match k {
        TraitKind::RustOnly => 0,
        TraitKind::Both => 1,
        TraitKind::ForeignOnly => 2,
    }
}

/// A metadata buffer as `uniffi_core::metadata::MetadataBuffer` builds one.
#[derive(Default)]
struct Buf(Vec<u8>);

impl Buf {
    fn u8(&mut self, v: u8) -> &mut Self {
        self.0.push(v);
        self
    }

    fn bool(&mut self, v: bool) -> &mut Self {
        self.u8(v as u8)
    }

    fn str(&mut self, s: &str) -> &mut Self {
        assert!(s.len() < 256, "a metadata string is under 256 bytes: {s}");
        self.u8(s.len() as u8);
        self.0.extend_from_slice(s.as_bytes());
        self
    }

    fn opt_str(&mut self, s: &Option<String>) -> &mut Self {
        match s {
            Some(s) => self.bool(true).str(s),
            None => self.bool(false),
        }
    }

    /// A docstring: its length as a u16, none being empty.
    fn doc(&mut self, s: &Option<String>) -> &mut Self {
        let s = s.as_deref().unwrap_or("");
        self.0.extend_from_slice(&(s.len() as u16).to_le_bytes());
        self.0.extend_from_slice(s.as_bytes());
        self
    }

    fn ty(&mut self, t: &Type) -> Result<&mut Self, String> {
        use codes::*;
        match t {
            Type::UInt8 => self.u8(TYPE_U8),
            Type::Int8 => self.u8(TYPE_I8),
            Type::UInt16 => self.u8(TYPE_U16),
            Type::Int16 => self.u8(TYPE_I16),
            Type::UInt32 => self.u8(TYPE_U32),
            Type::Int32 => self.u8(TYPE_I32),
            Type::UInt64 => self.u8(TYPE_U64),
            Type::Int64 => self.u8(TYPE_I64),
            Type::Float32 => self.u8(TYPE_F32),
            Type::Float64 => self.u8(TYPE_F64),
            Type::Boolean => self.u8(TYPE_BOOL),
            Type::String => self.u8(TYPE_STRING),
            Type::Bytes => self.u8(TYPE_VEC).u8(TYPE_U8),
            Type::Record { module_path, name } => self.u8(TYPE_RECORD).str(module_path).str(name),
            Type::Enum { module_path, name } => self.u8(TYPE_ENUM).str(module_path).str(name),
            Type::Object {
                module_path,
                name,
                imp: ObjectImpl::Struct,
            } => self.u8(TYPE_INTERFACE).str(module_path).str(name),
            Type::Object {
                module_path,
                name,
                imp: ObjectImpl::Trait(k),
            } => self.u8(TYPE_TRAIT_INTERFACE).str(module_path).str(name).u8(kind(*k)),
            Type::Custom { module_path, name, builtin } => return self.u8(TYPE_CUSTOM).str(module_path).str(name).ty(builtin),
            Type::Optional { inner_type } => return self.u8(TYPE_OPTION).ty(inner_type),
            Type::Box { inner_type } => return self.u8(TYPE_BOX).ty(inner_type),
            Type::Sequence { inner_type } => return self.u8(TYPE_VEC).ty(inner_type),
            other => return Err(format!("no encoding here for the UniFFI type {other:?}")),
        };
        Ok(self)
    }

    fn opt_ty(&mut self, t: &Option<Type>) -> Result<&mut Self, String> {
        match t {
            Some(t) => self.ty(t),
            None => Ok(self.u8(codes::TYPE_UNIT)),
        }
    }

    fn returns(&mut self, ok: &Option<Type>, err: &Option<Type>) -> Result<&mut Self, String> {
        match err {
            None => self.opt_ty(ok),
            Some(_) => self.u8(codes::TYPE_RESULT).opt_ty(ok)?.opt_ty(err),
        }
    }

    fn fields(&mut self, fs: &[FieldMetadata]) -> Result<&mut Self, String> {
        self.u8(fs.len() as u8);
        for f in fs {
            if f.default.is_some() {
                return Err(format!("{}: no encoding here for a field's default", f.name));
            }
            self.str(&f.name).opt_str(&f.orig_name).ty(&f.ty)?.bool(false).doc(&f.docstring);
        }
        Ok(self)
    }

    fn inputs(&mut self, ps: &[FnParamMetadata]) -> Result<&mut Self, String> {
        self.u8(ps.len() as u8);
        for p in ps {
            if p.default.is_some() {
                return Err(format!("{}: no encoding here for an argument's default", p.name));
            }
            self.str(&p.name).ty(&p.ty)?.bool(p.by_ref).bool(false);
        }
        Ok(self)
    }
}

/// `m` as UniFFI's macros encode it: what a built library holds in its
/// symbol, and what a function's checksum is of. The kinds a twin makes
/// and the bridge's own are encoded; any other is refused.
pub fn encode(m: &Metadata) -> Result<Vec<u8>, String> {
    let mut b = Buf::default();
    match m {
        Metadata::Namespace(NamespaceMetadata { crate_name, name }) => drop(b.u8(codes::NAMESPACE).str(crate_name).str(name)),
        Metadata::Record(RecordMetadata {
            module_path,
            name,
            orig_name,
            fields,
            docstring,
            ..
        }) => drop(
            b.u8(codes::RECORD)
                .str(module_path)
                .str(name)
                .opt_str(orig_name)
                .fields(fields)?
                .doc(docstring),
        ),
        Metadata::Enum(EnumMetadata {
            module_path,
            name,
            orig_name,
            shape,
            variants,
            discr_type,
            non_exhaustive,
            docstring,
            ..
        }) => {
            b.u8(codes::ENUM).str(module_path).str(name).opt_str(orig_name).u8(shape.as_u8());
            match discr_type {
                Some(t) => drop(b.bool(true).ty(t)?),
                None => drop(b.bool(false)),
            }
            b.u8(variants.len() as u8);
            for VariantMetadata {
                name,
                orig_name,
                discr,
                fields,
                docstring,
            } in variants
            {
                b.str(name).opt_str(orig_name);
                if *shape != (EnumShape::Error { flat: true }) {
                    if discr.is_some() {
                        return Err(format!("{name}: no encoding here for a variant's discriminant"));
                    }
                    b.bool(false).fields(fields)?;
                }
                b.doc(docstring);
            }
            b.bool(*non_exhaustive).doc(docstring);
        }
        Metadata::Object(ObjectMetadata {
            module_path,
            name,
            orig_name,
            imp: ObjectImpl::Struct,
            docstring,
            ..
        }) => drop(b.u8(codes::INTERFACE).str(module_path).str(name).opt_str(orig_name).doc(docstring)),
        Metadata::Object(ObjectMetadata {
            module_path,
            name,
            orig_name,
            imp: ObjectImpl::Trait(k),
            docstring,
            ..
        }) => drop(
            b.u8(codes::TRAIT_INTERFACE)
                .str(module_path)
                .str(name)
                .opt_str(orig_name)
                .doc(docstring)
                .u8(kind(*k)),
        ),
        Metadata::TraitMethod(TraitMethodMetadata {
            module_path,
            trait_name,
            index,
            name,
            orig_name,
            is_async,
            inputs,
            return_type,
            throws,
            docstring,
            ..
        }) => {
            b.u8(codes::TRAIT_METHOD).str(module_path).str(trait_name);
            b.0.extend_from_slice(&index.to_le_bytes());
            b.str(name).opt_str(orig_name).bool(*is_async);
            b.inputs(inputs)?.returns(return_type, throws)?.doc(docstring);
        }
        Metadata::Constructor(ConstructorMetadata {
            module_path,
            self_name,
            name,
            orig_name,
            is_async,
            inputs,
            throws,
            docstring,
            ..
        }) => {
            let me = Some(Type::Object {
                module_path: module_path.clone(),
                name: self_name.clone(),
                imp: ObjectImpl::Struct,
            });
            b.u8(codes::CONSTRUCTOR)
                .str(module_path)
                .str(self_name)
                .str(name)
                .opt_str(orig_name)
                .bool(*is_async);
            b.inputs(inputs)?.returns(&me, throws)?.doc(docstring);
        }
        Metadata::Func(FnMetadata {
            module_path,
            name,
            orig_name,
            is_async,
            inputs,
            return_type,
            throws,
            docstring,
            ..
        }) => {
            b.u8(codes::FUNC).str(module_path).str(name).opt_str(orig_name).bool(*is_async);
            b.inputs(inputs)?.returns(return_type, throws)?.doc(docstring);
        }
        Metadata::Method(MethodMetadata {
            module_path,
            self_name,
            name,
            orig_name,
            is_async,
            inputs,
            return_type,
            throws,
            docstring,
            ..
        }) => {
            b.u8(codes::METHOD)
                .str(module_path)
                .str(self_name)
                .str(name)
                .opt_str(orig_name)
                .bool(*is_async);
            b.inputs(inputs)?.returns(return_type, throws)?.doc(docstring);
        }
        Metadata::CustomType(CustomTypeMetadata {
            module_path,
            name,
            builtin,
            docstring,
            ..
        }) => drop(b.u8(codes::CUSTOM_TYPE).str(module_path).str(name).ty(builtin)?.doc(docstring)),
        other => return Err(format!("no encoding here for {other:?}")),
    }
    Ok(b.0)
}

/// A shared library's file holding `items` as a built one does — each in
/// a global symbol `UNIFFI_META_*` of a data section — and nothing else:
/// what UniFFI's generators read a library's metadata from (ELF, 64-bit,
/// little-endian).
pub fn library(items: &[Metadata]) -> Result<Vec<u8>, String> {
    let (mut data, mut strtab, mut syms) = (vec![], vec![0u8], vec![0u8; 24]);
    for (i, m) in items.iter().enumerate() {
        let (at, bytes) = (data.len(), encode(m)?);
        let name = strtab.len() as u32;
        strtab.extend_from_slice(format!("UNIFFI_META_{i}").as_bytes());
        strtab.push(0);
        syms.extend_from_slice(&name.to_le_bytes());
        syms.extend_from_slice(&[0x11, 0]); // global, an object
        syms.extend_from_slice(&1u16.to_le_bytes()); // in .data
        syms.extend_from_slice(&(at as u64).to_le_bytes());
        syms.extend_from_slice(&(bytes.len() as u64).to_le_bytes());
        data.extend(bytes);
    }
    let shstrtab = b"\0.data\0.symtab\0.strtab\0.shstrtab\0".to_vec();
    let blobs = [data, syms, strtab, shstrtab];
    let mut at = 64;
    let mut offsets = vec![];
    for b in &blobs {
        offsets.push(at);
        at += b.len();
    }
    let shoff = (at + 7) & !7;
    let mut out = vec![0x7f, b'E', b'L', b'F', 2, 1, 1, 0, 0, 0, 0, 0, 0, 0, 0, 0];
    out.extend_from_slice(&3u16.to_le_bytes()); // a shared object
    out.extend_from_slice(&62u16.to_le_bytes()); // x86-64
    out.extend_from_slice(&1u32.to_le_bytes());
    out.extend_from_slice(&0u64.to_le_bytes()); // entry
    out.extend_from_slice(&0u64.to_le_bytes()); // no program headers
    out.extend_from_slice(&(shoff as u64).to_le_bytes());
    out.extend_from_slice(&0u32.to_le_bytes());
    for v in [64u16, 0, 0, 64, 5, 4] {
        out.extend_from_slice(&v.to_le_bytes());
    }
    for b in &blobs {
        out.extend_from_slice(b);
    }
    out.resize(shoff, 0);
    // name, type, flags, addr, offset, size, link, info, align, entsize
    let header = |out: &mut Vec<u8>, name: u32, ty: u32, flags: u64, off: usize, size: usize, link: u32, info: u32, entsize: u64| {
        out.extend_from_slice(&name.to_le_bytes());
        out.extend_from_slice(&ty.to_le_bytes());
        out.extend_from_slice(&flags.to_le_bytes());
        out.extend_from_slice(&0u64.to_le_bytes());
        out.extend_from_slice(&(off as u64).to_le_bytes());
        out.extend_from_slice(&(size as u64).to_le_bytes());
        out.extend_from_slice(&link.to_le_bytes());
        out.extend_from_slice(&info.to_le_bytes());
        out.extend_from_slice(&1u64.to_le_bytes());
        out.extend_from_slice(&entsize.to_le_bytes());
    };
    header(&mut out, 0, 0, 0, 0, 0, 0, 0, 0);
    header(&mut out, 1, 1, 3, offsets[0], blobs[0].len(), 0, 0, 0); // .data: PROGBITS, WA
    header(&mut out, 7, 2, 0, offsets[1], blobs[1].len(), 3, 1, 24); // .symtab
    header(&mut out, 15, 3, 0, offsets[2], blobs[2].len(), 0, 0, 0); // .strtab
    header(&mut out, 23, 3, 0, offsets[3], blobs[3].len(), 0, 0, 0); // .shstrtab
    Ok(out)
}

/// Where each crate's `uniffi.toml` is: its root.
struct Roots(BTreeMap<String, PathBuf>);

impl BindgenPathsLayer for Roots {
    fn get_crate_root(&self, crate_name: &str) -> Option<Utf8PathBuf> {
        let p = self.0.get(crate_name)?;
        Utf8PathBuf::from_path_buf(p.clone()).ok()
    }
}

/// What UniFFI's generator for `language` writes of crate `of` from
/// `items`, as the library `lib` (`harken_ffi`: its name is what the
/// bindings load) would have them, each crate's configuration read from
/// its root in `roots`: every file, by its path under the output.
pub fn bindings(
    items: &[Metadata],
    lib: &str,
    of: &str,
    language: &str,
    roots: &BTreeMap<String, PathBuf>,
    scratch: &Path,
) -> Result<BTreeMap<String, String>, String> {
    std::fs::create_dir_all(scratch).map_err(|e| e.to_string())?;
    let source = scratch.join(format!("lib{lib}.so"));
    std::fs::write(&source, library(items)?).map_err(|e| e.to_string())?;
    of_library(&source, of, language, roots, scratch)
}

/// What UniFFI's generator for `language` writes of crate `of` from the
/// library at `source`, as `uniffi-bindgen generate --library` does, each
/// crate's configuration read from its root in `roots`.
pub fn of_library(
    source: &Path,
    of: &str,
    language: &str,
    roots: &BTreeMap<String, PathBuf>,
    scratch: &Path,
) -> Result<BTreeMap<String, String>, String> {
    let e = |e: &dyn std::fmt::Display| e.to_string();
    let language = match language {
        "kotlin" => TargetLanguage::Kotlin,
        "swift" => TargetLanguage::Swift,
        other => return Err(format!("no UniFFI generator here for {other}")),
    };
    let out = scratch.join("out");
    let _ = std::fs::remove_dir_all(&out);
    std::fs::create_dir_all(&out).map_err(|x| e(&x))?;
    let utf8 = |p: PathBuf| Utf8PathBuf::from_path_buf(p).map_err(|p| format!("{}: not UTF-8", p.display()));
    let mut paths = BindgenPaths::default();
    paths.add_layer(Roots(roots.clone()));
    let options = GenerateOptions {
        languages: vec![language],
        source: utf8(source.into())?,
        out_dir: utf8(out.clone())?,
        crate_filter: Some(of.into()),
        ..GenerateOptions::default()
    };
    generate_with_bindgen_paths(options, paths).map_err(|x| format!("{x:#}"))?;
    let mut files = BTreeMap::new();
    let mut dirs = vec![out.clone()];
    while let Some(d) = dirs.pop() {
        for f in std::fs::read_dir(&d).map_err(|x| e(&x))? {
            let p = f.map_err(|x| e(&x))?.path();
            if p.is_dir() {
                dirs.push(p);
            } else {
                let rel = p.strip_prefix(&out).unwrap().to_string_lossy().into_owned();
                files.insert(rel, std::fs::read_to_string(&p).map_err(|x| e(&x))?);
            }
        }
    }
    Ok(files)
}

/// The metadata a built library holds: what [`metadata`] is held to.
pub fn read_library(path: &Path) -> Result<Vec<Metadata>, String> {
    let p = Utf8PathBuf::from_path_buf(path.into()).map_err(|p| format!("{}: not UTF-8", p.display()))?;
    uniffi_bindgen::macro_metadata::extract_from_library(&p).map_err(|e| format!("{e:#}"))
}

/// The bridge's names a twin's metadata refers to (`ark_ffi`, `typed.rs`).
const BRIDGE: &str = "ark_ffi";
const TYPED: &str = "ark_ffi::typed";

fn crosses(c: &Crosses) -> Type {
    match c {
        Crosses::I64 => Type::Int64,
        Crosses::U64 => Type::UInt64,
        Crosses::I32 => Type::Int32,
        Crosses::U32 => Type::UInt32,
        Crosses::U8 => Type::UInt8,
        Crosses::Bool => Type::Boolean,
        Crosses::String => Type::String,
        Crosses::Id => Type::Custom {
            module_path: TYPED.into(),
            name: "RowId".into(),
            builtin: Box::new(Type::String),
        },
        Crosses::Option(x) => Type::Optional {
            inner_type: Box::new(crosses(x)),
        },
        Crosses::Vec(x) if **x == Crosses::U8 => Type::Bytes,
        Crosses::Vec(x) => Type::Sequence {
            inner_type: Box::new(crosses(x)),
        },
        Crosses::Record(m, n) => Type::Record {
            module_path: m.clone(),
            name: n.clone(),
        },
        Crosses::Enum(m, n) => Type::Enum {
            module_path: m.clone(),
            name: n.clone(),
        },
    }
}

fn field(name: &str, ty: Type) -> FieldMetadata {
    FieldMetadata {
        name: name.into(),
        orig_name: None,
        ty,
        default: None,
        docstring: None,
    }
}

/// The metadata `crate_name`'s twins embed in a library (`ark-macros/src/
/// ffi.rs`), its namespace's (`uniffi::setup_scaffolding!()`), and the
/// bridge's namespace, which its types are named under.
pub fn metadata(crate_name: &str, twins: &[Twin]) -> Vec<Metadata> {
    let ns = |c: &str| {
        Metadata::Namespace(NamespaceMetadata {
            crate_name: c.into(),
            name: c.into(),
        })
    };
    let mut out = vec![ns(crate_name), ns(BRIDGE)];
    for t in twins {
        let (module_path, name) = (t.module.clone(), t.name.clone());
        let fields = |fs: &[(String, Crosses)]| fs.iter().map(|(n, c)| field(n, crosses(c))).collect::<Vec<_>>();
        let me = match &t.shape {
            Shape::Record(fs) => {
                out.push(Metadata::Record(RecordMetadata {
                    module_path: module_path.clone(),
                    name: name.clone(),
                    orig_name: Some(format!("{name}Ffi")),
                    remote: false,
                    fields: fields(fs),
                    docstring: None,
                }));
                Type::Record {
                    module_path: module_path.clone(),
                    name: name.clone(),
                }
            }
            Shape::Enum(vs) => {
                out.push(Metadata::Enum(EnumMetadata {
                    module_path: module_path.clone(),
                    name: name.clone(),
                    orig_name: Some(format!("{name}Ffi")),
                    shape: EnumShape::Enum,
                    remote: false,
                    variants: vs
                        .iter()
                        .map(|(v, fs)| VariantMetadata {
                            name: v.clone(),
                            orig_name: None,
                            discr: None,
                            fields: fields(fs),
                            docstring: None,
                        })
                        .collect(),
                    discr_type: None,
                    non_exhaustive: false,
                    docstring: None,
                }));
                Type::Enum {
                    module_path: module_path.clone(),
                    name: name.clone(),
                }
            }
        };
        if !t.view {
            continue;
        }
        let (view, taken) = (format!("{name}View"), format!("{name}Taken"));
        let rows = Type::Sequence {
            inner_type: Box::new(me.clone()),
        };
        out.push(Metadata::Object(ObjectMetadata {
            module_path: module_path.clone(),
            name: view.clone(),
            orig_name: None,
            remote: false,
            imp: ObjectImpl::Struct,
            docstring: Some(format!("A view of `{name}` rows, read as they are: no JSON.")),
        }));
        out.push(Metadata::Record(RecordMetadata {
            module_path: module_path.clone(),
            name: taken.clone(),
            orig_name: None,
            remote: false,
            fields: vec![
                field(
                    "update",
                    Type::Enum {
                        module_path: TYPED.into(),
                        name: "Taken".into(),
                    },
                ),
                field("rows", rows.clone()),
            ],
            docstring: None,
        }));
        let arg = |n: &str, ty: Type| FnParamMetadata::simple(n, ty);
        let constructor = ConstructorMetadata {
            module_path: module_path.clone(),
            self_name: view.clone(),
            name: "new".into(),
            orig_name: None,
            is_async: false,
            inputs: vec![
                arg(
                    "peer",
                    Type::Object {
                        module_path: BRIDGE.into(),
                        name: "Peer".into(),
                        imp: ObjectImpl::Struct,
                    },
                ),
                arg("name", Type::String),
                arg("input", Type::String),
            ],
            throws: Some(Type::Enum {
                module_path: BRIDGE.into(),
                name: "Refusal".into(),
            }),
            checksum: None,
            docstring: Some("The view `name` (whose rows are this type) on its input's JSON.".into()),
        };
        out.push(Metadata::Constructor(constructor));
        let method = |n: &str, ret: Type| MethodMetadata {
            module_path: module_path.clone(),
            self_name: view.clone(),
            name: n.into(),
            orig_name: None,
            is_async: false,
            inputs: vec![],
            return_type: Some(ret),
            throws: None,
            takes_self_by_arc: false,
            checksum: None,
            docstring: None,
        };
        out.push(Metadata::Method(method("rows", rows)));
        out.push(Metadata::Method(method(
            "take",
            Type::Record {
                module_path: module_path.clone(),
                name: taken,
            },
        )));
    }
    // As a library's are read: each with the checksum of its encoding.
    out.into_iter()
        .map(|m| Metadata::read(&encode(&m).expect("a twin's metadata encodes")).expect("and reads back"))
        .collect()
}
