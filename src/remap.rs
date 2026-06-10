//! Mechanical re-encoding of one input module's items with merged indices.
//!
//! [`Remapper`] implements `wasm_encoder`'s [`Reencode`] trait, overriding
//! only the index hooks; everything else (instructions, types, const
//! expressions, segments) is re-encoded by the trait's defaults, which route
//! every index they encounter through these hooks.

use wasm_encoder::reencode::{Error as ReencodeError, Reencode};

use crate::merge::MergeError;
use crate::parse::Kind;

/// Old-index → merged-index tables for every index space of one input module.
#[derive(Debug, Default)]
pub(crate) struct RemapTables {
    pub(crate) types: Vec<u32>,
    pub(crate) funcs: Vec<u32>,
    pub(crate) tables: Vec<u32>,
    pub(crate) memories: Vec<u32>,
    pub(crate) globals: Vec<u32>,
    pub(crate) tags: Vec<u32>,
    pub(crate) elements: Vec<u32>,
    pub(crate) datas: Vec<u32>,
}

impl RemapTables {
    pub(crate) fn kind(&self, kind: Kind) -> &Vec<u32> {
        match kind {
            Kind::Func => &self.funcs,
            Kind::Table => &self.tables,
            Kind::Memory => &self.memories,
            Kind::Global => &self.globals,
            Kind::Tag => &self.tags,
        }
    }

    pub(crate) fn kind_mut(&mut self, kind: Kind) -> &mut Vec<u32> {
        match kind {
            Kind::Func => &mut self.funcs,
            Kind::Table => &mut self.tables,
            Kind::Memory => &mut self.memories,
            Kind::Global => &mut self.globals,
            Kind::Tag => &mut self.tags,
        }
    }
}

pub(crate) struct Remapper<'a> {
    pub(crate) module_name: &'a str,
    pub(crate) tables: &'a RemapTables,
}

impl Remapper<'_> {
    fn map(
        &self,
        table: &[u32],
        index: u32,
        kind: &'static str,
    ) -> Result<u32, ReencodeError<MergeError>> {
        table.get(index as usize).copied().ok_or_else(|| {
            ReencodeError::UserError(MergeError::OutOfBoundsIndex {
                module: self.module_name.to_string(),
                kind,
                index,
            })
        })
    }
}

impl Reencode for Remapper<'_> {
    type Error = MergeError;

    fn type_index(&mut self, ty: u32) -> Result<u32, ReencodeError<MergeError>> {
        self.map(&self.tables.types, ty, "type")
    }

    fn function_index(&mut self, func: u32) -> Result<u32, ReencodeError<MergeError>> {
        self.map(&self.tables.funcs, func, "function")
    }

    fn table_index(&mut self, table: u32) -> Result<u32, ReencodeError<MergeError>> {
        self.map(&self.tables.tables, table, "table")
    }

    fn memory_index(&mut self, memory: u32) -> Result<u32, ReencodeError<MergeError>> {
        self.map(&self.tables.memories, memory, "memory")
    }

    fn global_index(&mut self, global: u32) -> Result<u32, ReencodeError<MergeError>> {
        self.map(&self.tables.globals, global, "global")
    }

    fn tag_index(&mut self, tag: u32) -> Result<u32, ReencodeError<MergeError>> {
        self.map(&self.tables.tags, tag, "tag")
    }

    fn element_index(&mut self, element: u32) -> Result<u32, ReencodeError<MergeError>> {
        self.map(&self.tables.elements, element, "element segment")
    }

    fn data_index(&mut self, data: u32) -> Result<u32, ReencodeError<MergeError>> {
        self.map(&self.tables.datas, data, "data segment")
    }
}

/// Convert a re-encoding error into a [`MergeError`], attributing parse
/// failures to the module being re-encoded.
pub(crate) fn flatten_error(error: ReencodeError<MergeError>, module: &str) -> MergeError {
    match error {
        ReencodeError::UserError(error) => error,
        ReencodeError::ParseError(source) => MergeError::InvalidModule {
            name: module.to_string(),
            source,
        },
        other => MergeError::Reencode {
            module: module.to_string(),
            message: other.to_string(),
        },
    }
}
