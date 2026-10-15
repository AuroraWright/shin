use shin_core::format::scenario::instruction_elements::Register;

use crate::{
    compile::{
        Db, DefMap,
        def_map::{DefValue, Name, RegisterName, ResolveKind},
    },
    syntax::ast::{self},
};

#[derive(Debug, Copy, Clone)]
enum ResolveContextInner<'db> {
    Empty,
    Real {
        def_map: DefMap<'db>,
        resolve_kind: ResolveKind,
    },
}

pub struct ResolveContext<'db> {
    db: &'db dyn Db,
    inner: ResolveContextInner<'db>,
}

impl<'db> ResolveContext<'db> {
    pub fn new_empty(db: &'db dyn Db) -> Self {
        Self {
            db,
            inner: ResolveContextInner::Empty,
        }
    }

    pub fn new(db: &'db dyn Db, def_map: DefMap<'db>, resolve_kind: ResolveKind) -> Self {
        Self {
            db,
            inner: ResolveContextInner::Real {
                def_map,
                resolve_kind,
            },
        }
    }

    pub fn resolve_register(&self, register: &ast::RegisterIdentKind) -> Option<Register> {
        match register {
            &ast::RegisterIdentKind::Register(register) => Some(register),
            ast::RegisterIdentKind::Alias(name) => match self.inner {
                ResolveContextInner::Empty => None,
                ResolveContextInner::Real {
                    def_map,
                    resolve_kind,
                } => def_map.resolve_register(self.db, RegisterName(name.clone()), resolve_kind),
            },
        }
    }

    pub fn resolve_item(&self, name: &Name) -> Option<DefValue> {
        match self.inner {
            ResolveContextInner::Empty => None,
            ResolveContextInner::Real {
                def_map,
                resolve_kind: _,
            } => def_map.resolve_item(self.db, name.clone()),
        }
    }
}
