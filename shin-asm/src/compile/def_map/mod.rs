mod collect;
mod items;
mod registers;

use std::{borrow::Cow, fmt::Display};

pub use items::{DefValue, ResolvedItems};
pub use registers::{LocalRegisters, ResolvedGlobalRegisters};
use rustc_hash::FxHashMap;
use shin_core::format::scenario::instruction_elements::Register;
use smol_str::SmolStr;

use crate::{
    compile::{BlockIdRepr, BlockIdWithFile, Db, MakeWithFile, Program, WithFile},
    syntax::ast::visit::ItemIndex,
};

#[derive(Clone, Eq, PartialEq, Hash, Debug, Ord, PartialOrd)]
pub struct Name(pub SmolStr);

impl Display for Name {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.0.fmt(f)
    }
}

#[derive(Clone, Eq, PartialEq, Hash, Debug, Ord, PartialOrd)]
pub struct RegisterName(pub SmolStr);

impl Display for RegisterName {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.0.fmt(f)
    }
}
#[derive(Debug, Clone, Eq, PartialEq, Ord, PartialOrd)]
pub enum BlockName {
    GlobalBlock(Option<Name>),
    Function(Option<Name>),
    LocalBlock(Option<Name>),
}

#[derive(Debug, Copy, Clone)]
pub enum ResolveKind {
    GlobalOnly,
    LocalAndGlobal(BlockIdWithFile),
}

#[salsa::tracked(debug)]
pub struct DefMap<'db> {
    #[tracked]
    #[returns(ref)]
    items: ResolvedItems,
    #[tracked]
    #[returns(ref)]
    global_registers: ResolvedGlobalRegisters,
    #[tracked]
    #[returns(ref)]
    local_registers: LocalRegisters,
    #[tracked]
    #[returns(ref)]
    block_names: FxHashMap<BlockIdWithFile, BlockName>,
}

impl<'db> DefMap<'db> {
    // pub fn get_item(&self, name: &Name) -> Option<FileDefRef> {
    //     self.items.get(name).copied()
    // }
}

#[salsa::tracked]
impl<'db> DefMap<'db> {
    #[salsa::tracked(returns(clone))]
    pub fn local_register(
        self,
        db: &dyn Db,
        item: WithFile<ItemIndex>,
        name: RegisterName,
    ) -> Option<Register> {
        self.local_registers(db)
            .get(&item)
            .and_then(|v| v.get(&name))
            .copied()
    }

    #[salsa::tracked(returns(clone))]
    pub fn global_register(self, db: &dyn Db, name: RegisterName) -> Option<Register> {
        // TODO: is this flatten correct?
        self.global_registers(db).get(&name).copied().flatten()
    }

    pub fn resolve_register(
        self,
        db: &dyn Db,
        name: RegisterName,
        kind: ResolveKind,
    ) -> Option<Register> {
        if let ResolveKind::LocalAndGlobal(WithFile {
            file,
            value: block_id,
        }) = kind
        {
            let item_index = match block_id.repr() {
                BlockIdRepr::Dummy => panic!("Cannot resolve register in a dummy block"),
                BlockIdRepr::Block { item_index, .. } | BlockIdRepr::Function { item_index } => {
                    item_index
                }
            };

            if let Some(register) = self.local_register(db, item_index.in_file(file), name.clone())
            {
                return Some(register);
            }
        }

        self.global_register(db, name)
    }

    #[salsa::tracked(returns(clone))]
    pub fn resolve_item(self, db: &dyn Db, name: Name) -> Option<DefValue> {
        self.items(db).get(&name).cloned()
    }

    pub fn debug_dump(self, db: &dyn Db) -> String {
        use std::fmt::Write as _;

        let mut output = String::new();

        let mut items = self.items(db).iter().collect::<Vec<_>>();
        items.sort_by_key(|&(name, _)| name);

        writeln!(output, "items:").unwrap();
        for (name, value) in items {
            writeln!(output, "  {}: {:?}", name, value).unwrap();
        }

        let mut global_registers = self.global_registers(db).iter().collect::<Vec<_>>();
        global_registers.sort_by_key(|(name, _)| *name);
        let mut local_registers = self.local_registers(db).iter().collect::<Vec<_>>();
        local_registers.sort_by_key(|&(&index, _)| index);

        writeln!(output, "registers:").unwrap();
        writeln!(output, "  global:").unwrap();
        for (name, value) in global_registers {
            writeln!(
                output,
                "    {}: {}",
                name,
                value.map_or(Cow::from("[ERROR]"), |r| format!("{:?}", r).into())
            )
            .unwrap();
        }
        writeln!(output, "  local:").unwrap();
        for (item_index, registers) in local_registers {
            writeln!(
                output,
                "    item {}@{}: ",
                item_index.value,
                item_index.file.path(db)
            )
            .unwrap();
            for (name, value) in registers {
                writeln!(output, "      {}: {:?}", name, value).unwrap();
            }
        }

        let mut block_names = self.block_names(db).iter().collect::<Vec<_>>();
        block_names.sort();

        writeln!(output, "block names:").unwrap();
        for (block_id, name) in block_names {
            let file_name = block_id.file.path(db);
            let block_id = &block_id.value;

            writeln!(output, "  {:?} @ {}: {:?}", block_id, file_name, name).unwrap();
        }

        output
    }
}

#[salsa::tracked(returns(clone))]
pub fn build_def_map<'db>(db: &'db dyn Db, program: Program) -> DefMap<'db> {
    let items = items::collect_item_defs(db, program);
    let items = items::resolve_item_defs(db, &items);

    let local_registers = registers::collect_local_registers(db, program);
    let global_registers = registers::collect_global_registers(db, program);
    let global_registers = registers::resolve_global_registers(db, &global_registers);
    let block_names = collect::collect_block_names(db, program);

    DefMap::new(db, items, global_registers, local_registers, block_names)
}

#[cfg(test)]
mod tests {
    use expect_test::expect;

    use super::build_def_map;
    use crate::compile::{
        DefMap, File, Program,
        db::Database,
        diagnostics::{HirDiagnosticAccumulator, SourceDiagnosticAccumulator},
    };

    fn parse_def_map<'db>(db: &'db Database, code: &str) -> (DefMap<'db>, Option<String>) {
        let file = File::new(db, "test.sal".to_string(), code.to_string());
        let program = Program::new(db, vec![file]);
        let def_map = build_def_map(db, program);

        let hir_errors = build_def_map::accumulated::<HirDiagnosticAccumulator>(db, program)
            .into_iter()
            .map(|diagnostic| diagnostic.0.clone())
            .collect::<Vec<_>>();
        let source_errors = build_def_map::accumulated::<SourceDiagnosticAccumulator>(db, program)
            .into_iter()
            .map(|diagnostic| diagnostic.0.clone())
            .collect::<Vec<_>>();

        let errors = (!source_errors.is_empty() || !hir_errors.is_empty()).then(|| {
            format!(
                "building def map produced errors:\n\
                source-level: {source_errors:?}\n\
                hir-level: {hir_errors:?}"
            )
        });

        (def_map, errors)
    }

    #[test]
    fn check_map_dump() {
        let db = Database::default();
        let (def_map, errors) = parse_def_map(
            &db,
            r#"
def ABIBA = 3 + 3
def BEBA = ABIBA * 2
def $_aboba = $v17
def $keka = $_aboba

function KEKA($a0, $hello, $keka)
    add $v1, 2, 2
ABOBA:
    add $v1, 3, 3
endfun

    add $v2, 2, 2
LABEL1:
    sub $v2, 2, 2
    j LABEL1
LABEL2:
        "#,
        );

        assert!(errors.is_none());

        expect![[r#"
            items:
              ABIBA: Value(6)
              BEBA: Value(12)
              KEKA: Block(WithFile { value: BlockId { item_index: 4, block_index: None }, file: File { [salsa id]: Id(0) } })
              LABEL1: Block(WithFile { value: BlockId { item_index: 5, block_index: Some(1) }, file: File { [salsa id]: Id(0) } })
              LABEL2: Block(WithFile { value: BlockId { item_index: 5, block_index: Some(2) }, file: File { [salsa id]: Id(0) } })
            registers:
              global:
                _aboba: $v17
                keka: $v17
              local:
                item #4@test.sal: 
                  hello: $a1
                  keka: $a2
            block names:
              BlockId { item_index: 4, block_index: None } @ test.sal: Function(Some(Name("KEKA")))
              BlockId { item_index: 4, block_index: Some(0) } @ test.sal: LocalBlock(None)
              BlockId { item_index: 4, block_index: Some(1) } @ test.sal: LocalBlock(Some(Name("ABOBA")))
              BlockId { item_index: 5, block_index: Some(0) } @ test.sal: GlobalBlock(None)
              BlockId { item_index: 5, block_index: Some(1) } @ test.sal: GlobalBlock(Some(Name("LABEL1")))
              BlockId { item_index: 5, block_index: Some(2) } @ test.sal: GlobalBlock(Some(Name("LABEL2")))
        "#]].assert_eq(&def_map.debug_dump(&db));
    }

    #[test]
    fn register_loop() {
        let db = Database::default();
        let (def_map, errors) = parse_def_map(
            &db,
            r#"
def $a = $b
def $b = $a
        "#,
        );

        expect![[r#"
            building def map produced errors:
            source-level: [Diagnostic { message: "Encountered a loop while resolving register $a", location: Span(WithFile { value: 22..24, file: File { [salsa id]: Id(0) } }), additional_labels: [] }]
            hir-level: []"#]]
            .assert_eq(errors.as_deref().unwrap());

        expect![[r#"
            items:
            registers:
              global:
                a: [ERROR]
                b: [ERROR]
              local:
            block names:
        "#]]
        .assert_eq(&def_map.debug_dump(&db));
    }

    #[test]
    fn constexpr_overflow() {
        let db = Database::default();
        let (def_map, errors) = parse_def_map(
            &db,
            r#"
def A = 65536 * 65536
        "#,
        );

        expect![[r#"
            building def map produced errors:
            source-level: [Diagnostic { message: "Overflow in constant expression", location: Span(WithFile { value: 9..22, file: File { [salsa id]: Id(0) } }), additional_labels: [] }]
            hir-level: []"#]]
        .assert_eq(errors.as_deref().unwrap());

        expect![[r#"
            items:
              A: Value(<error>)
            registers:
              global:
              local:
            block names:
        "#]]
        .assert_eq(&def_map.debug_dump(&db));
    }
}
