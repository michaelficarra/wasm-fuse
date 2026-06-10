//! Collects the contents of one input module into a form the merger can work
//! with: every section's items, plus per-kind import lists and an export map
//! used during import resolution.

use std::collections::HashMap;
use std::ops::{Index, IndexMut};

use wasmparser::{
    Data, Element, Export, ExternalKind, FunctionBody, Global, Import, MemoryType, Parser, Payload,
    RecGroup, Table, TagType, TypeRef,
};

use crate::merge::MergeError;

/// One of the five importable/exportable index spaces.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) enum Kind {
    Func,
    Table,
    Memory,
    Global,
    Tag,
}

impl Kind {
    pub(crate) const ALL: [Kind; 5] = [
        Kind::Func,
        Kind::Table,
        Kind::Memory,
        Kind::Global,
        Kind::Tag,
    ];

    pub(crate) fn of_import(ty: TypeRef) -> Kind {
        match ty {
            TypeRef::Func(_) | TypeRef::FuncExact(_) => Kind::Func,
            TypeRef::Table(_) => Kind::Table,
            TypeRef::Memory(_) => Kind::Memory,
            TypeRef::Global(_) => Kind::Global,
            TypeRef::Tag(_) => Kind::Tag,
        }
    }

    pub(crate) fn of_export(kind: ExternalKind) -> Kind {
        match kind {
            ExternalKind::Func | ExternalKind::FuncExact => Kind::Func,
            ExternalKind::Table => Kind::Table,
            ExternalKind::Memory => Kind::Memory,
            ExternalKind::Global => Kind::Global,
            ExternalKind::Tag => Kind::Tag,
        }
    }

    /// The kind as it should appear in error messages.
    pub(crate) fn noun(self) -> &'static str {
        match self {
            Kind::Func => "function",
            Kind::Table => "table",
            Kind::Memory => "memory",
            Kind::Global => "global",
            Kind::Tag => "tag",
        }
    }
}

/// A value per [`Kind`].
#[derive(Clone, Debug, Default)]
pub(crate) struct KindMap<T>([T; 5]);

impl<T> Index<Kind> for KindMap<T> {
    type Output = T;
    fn index(&self, kind: Kind) -> &T {
        &self.0[kind as usize]
    }
}

impl<T> IndexMut<Kind> for KindMap<T> {
    fn index_mut(&mut self, kind: Kind) -> &mut T {
        &mut self.0[kind as usize]
    }
}

/// The relevant contents of a single input module.
pub(crate) struct ParsedModule<'a> {
    pub(crate) name: String,
    pub(crate) types: Vec<RecGroup>,
    /// Imports of each kind, in import-section order; positions within these
    /// lists are the import slots of each index space.
    pub(crate) imports: KindMap<Vec<Import<'a>>>,
    /// Type indices of defined functions.
    pub(crate) func_types: Vec<u32>,
    pub(crate) tables: Vec<Table<'a>>,
    pub(crate) memories: Vec<MemoryType>,
    pub(crate) globals: Vec<Global<'a>>,
    pub(crate) tags: Vec<TagType>,
    pub(crate) exports: Vec<Export<'a>>,
    /// Export name → exported item's module-local index, used to resolve other
    /// modules' imports against this module. Keyed by original export names,
    /// independent of any output renaming.
    pub(crate) export_map: HashMap<(Kind, &'a str), u32>,
    pub(crate) start: Option<u32>,
    pub(crate) elements: Vec<Element<'a>>,
    pub(crate) datas: Vec<Data<'a>>,
    pub(crate) code: Vec<FunctionBody<'a>>,
    pub(crate) has_data_count: bool,
}

impl<'a> ParsedModule<'a> {
    pub(crate) fn import_count(&self, kind: Kind) -> u32 {
        self.imports[kind].len() as u32
    }

    pub(crate) fn defined_count(&self, kind: Kind) -> u32 {
        let count = match kind {
            Kind::Func => self.func_types.len(),
            Kind::Table => self.tables.len(),
            Kind::Memory => self.memories.len(),
            Kind::Global => self.globals.len(),
            Kind::Tag => self.tags.len(),
        };
        count as u32
    }

    /// Total size of an index space (imports followed by definitions).
    pub(crate) fn item_count(&self, kind: Kind) -> u32 {
        self.import_count(kind) + self.defined_count(kind)
    }

    /// The total number of types across all rec groups.
    pub(crate) fn type_count(&self) -> u32 {
        self.types
            .iter()
            .map(|group| group.types().len() as u32)
            .sum()
    }
}

/// Parse `bytes` (a binary module) into a [`ParsedModule`].
pub(crate) fn parse_module<'a>(
    name: &str,
    bytes: &'a [u8],
) -> Result<ParsedModule<'a>, MergeError> {
    let mut module = ParsedModule {
        name: name.to_string(),
        types: Vec::new(),
        imports: KindMap::default(),
        func_types: Vec::new(),
        tables: Vec::new(),
        memories: Vec::new(),
        globals: Vec::new(),
        tags: Vec::new(),
        exports: Vec::new(),
        export_map: HashMap::new(),
        start: None,
        elements: Vec::new(),
        datas: Vec::new(),
        code: Vec::new(),
        has_data_count: false,
    };

    let invalid = |source: wasmparser::BinaryReaderError| MergeError::InvalidModule {
        name: name.to_string(),
        source,
    };

    for payload in Parser::new(0).parse_all(bytes) {
        match payload.map_err(invalid)? {
            Payload::TypeSection(section) => {
                for group in section {
                    module.types.push(group.map_err(invalid)?);
                }
            }
            Payload::ImportSection(section) => {
                // into_imports flattens compact import groups into plain imports
                for import in section.into_imports() {
                    let import = import.map_err(invalid)?;
                    module.imports[Kind::of_import(import.ty)].push(import);
                }
            }
            Payload::FunctionSection(section) => {
                for type_index in section {
                    module.func_types.push(type_index.map_err(invalid)?);
                }
            }
            Payload::TableSection(section) => {
                for table in section {
                    module.tables.push(table.map_err(invalid)?);
                }
            }
            Payload::MemorySection(section) => {
                for memory in section {
                    module.memories.push(memory.map_err(invalid)?);
                }
            }
            Payload::TagSection(section) => {
                for tag in section {
                    module.tags.push(tag.map_err(invalid)?);
                }
            }
            Payload::GlobalSection(section) => {
                for global in section {
                    module.globals.push(global.map_err(invalid)?);
                }
            }
            Payload::ExportSection(section) => {
                for export in section {
                    let export = export.map_err(invalid)?;
                    module.exports.push(export);
                    module
                        .export_map
                        .insert((Kind::of_export(export.kind), export.name), export.index);
                }
            }
            Payload::StartSection { func, .. } => module.start = Some(func),
            Payload::ElementSection(section) => {
                for element in section {
                    module.elements.push(element.map_err(invalid)?);
                }
            }
            Payload::DataCountSection { .. } => module.has_data_count = true,
            Payload::DataSection(section) => {
                for datum in section {
                    module.datas.push(datum.map_err(invalid)?);
                }
            }
            Payload::CodeSectionEntry(body) => module.code.push(body),
            // Custom sections (including names) are dropped for now; see
            // PLAN.md phase 4.
            _ => {}
        }
    }

    Ok(module)
}
