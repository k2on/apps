//! The static types of the IR, with constructors an author can write
//! inline: `Ty::option(Ty::Text)`, `Ty::list(m.row_ty("track"))`.

use std::collections::BTreeMap;

use ark::schema::Ty as IrTy;

/// `Ark.Schema.Ty`, as the builder spells it.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Ty {
    Bool,
    Int,
    Text,
    Bytes,
    /// An id naming a row of this table.
    Id(String),
    Enum(Vec<String>),
    Option(Box<Ty>),
    List(Box<Ty>),
    Struct(BTreeMap<String, Ty>),
}

impl Ty {
    /// `TId table`.
    pub fn id(table: &str) -> Ty {
        Ty::Id(table.to_string())
    }

    /// `TOption t`.
    pub fn option(t: Ty) -> Ty {
        Ty::Option(Box::new(t))
    }

    /// `TList t`.
    pub fn list(t: Ty) -> Ty {
        Ty::List(Box::new(t))
    }

    /// `TStruct fields`.
    pub fn structure<K: Into<String>>(fields: impl IntoIterator<Item = (K, Ty)>) -> Ty {
        Ty::Struct(fields.into_iter().map(|(k, t)| (k.into(), t)).collect())
    }

    /// `TEnum variants`.
    pub fn enumeration<S: Into<String>>(variants: impl IntoIterator<Item = S>) -> Ty {
        Ty::Enum(variants.into_iter().map(Into::into).collect())
    }

    /// The `ark` crate's type.
    pub fn to_ir(&self) -> IrTy {
        match self {
            Ty::Bool => IrTy::Bool,
            Ty::Int => IrTy::Int,
            Ty::Text => IrTy::Text,
            Ty::Bytes => IrTy::Bytes,
            Ty::Id(t) => IrTy::Id(t.clone()),
            Ty::Enum(vs) => IrTy::Enum(vs.clone()),
            Ty::Option(t) => IrTy::Option(Box::new(t.to_ir())),
            Ty::List(t) => IrTy::List(Box::new(t.to_ir())),
            Ty::Struct(fs) => IrTy::Struct(fs.iter().map(|(k, t)| (k.clone(), t.to_ir())).collect()),
        }
    }
}

impl From<Ty> for IrTy {
    fn from(t: Ty) -> IrTy {
        t.to_ir()
    }
}

impl From<&IrTy> for Ty {
    fn from(t: &IrTy) -> Ty {
        match t {
            IrTy::Bool => Ty::Bool,
            IrTy::Int => Ty::Int,
            IrTy::Text => Ty::Text,
            IrTy::Bytes => Ty::Bytes,
            IrTy::Id(t) => Ty::Id(t.clone()),
            IrTy::Enum(vs) => Ty::Enum(vs.clone()),
            IrTy::Option(t) => Ty::Option(Box::new(Ty::from(&**t))),
            IrTy::List(t) => Ty::List(Box::new(Ty::from(&**t))),
            IrTy::Struct(fs) => Ty::Struct(fs.iter().map(|(k, t)| (k.clone(), Ty::from(t))).collect()),
        }
    }
}

impl From<IrTy> for Ty {
    fn from(t: IrTy) -> Ty {
        Ty::from(&t)
    }
}
