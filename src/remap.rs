//! Mechanical re-encoding of one input module's items with merged indices.
//!
//! [`Remapper`] implements `wasm_encoder`'s [`Reencode`] trait, overriding
//! only the index hooks; everything else (instructions, types, const
//! expressions, segments) is re-encoded by the trait's defaults, which route
//! every index they encounter through these hooks.

use wasm_encoder::reencode::{Error as ReencodeError, Reencode};

use crate::merge::MergeError;
use crate::parse::Kind;

/// Sentinel for an item removed by pruning: it has no merged index, and by
/// construction no live item references it. A lookup hitting this value is an
/// internal liveness-analysis bug, reported as an error rather than silently
/// emitting a wrong index.
pub(crate) const PRUNED: u32 = u32::MAX;

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
    /// When set, [`Reencode::parse_function_body`] records, per instruction,
    /// the input offset (absolute, within the input binary) and the output
    /// offset (relative to the start of the re-encoded function body) — the
    /// basis for translating branch hints and source maps.
    pub(crate) instruction_offsets: Option<&'a mut Vec<(usize, u32)>>,
}

impl Remapper<'_> {
    fn map(
        &self,
        table: &[u32],
        index: u32,
        kind: &'static str,
    ) -> Result<u32, ReencodeError<MergeError>> {
        match table.get(index as usize).copied() {
            Some(PRUNED) => Err(ReencodeError::UserError(MergeError::Reencode {
                module: self.module_name.to_string(),
                message: format!(
                    "internal error: {kind} {index} was pruned but is still referenced"
                ),
            })),
            Some(mapped) => Ok(mapped),
            None => Err(ReencodeError::UserError(MergeError::OutOfBoundsIndex {
                module: self.module_name.to_string(),
                kind,
                index,
            })),
        }
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

    // As the default implementation, with optional per-instruction offset
    // recording.
    fn parse_function_body(
        &mut self,
        code: &mut wasm_encoder::CodeSection,
        func: wasmparser::FunctionBody<'_>,
    ) -> Result<(), ReencodeError<MergeError>> {
        let mut function = self.new_function_with_parsed_locals(&func)?;
        let mut reader = func.get_operators_reader()?;
        while !reader.eof() {
            let input_offset = reader.original_position();
            let output_offset = function.byte_len() as u32;
            let instruction = self.parse_instruction(&mut reader)?;
            function.instruction(&instruction);
            if let Some(offsets) = self.instruction_offsets.as_deref_mut() {
                offsets.push((input_offset, output_offset));
            }
        }
        code.function(&function);
        Ok(())
    }
}

/// Convert a re-encoding error into a [`MergeError`], attributing parse
/// failures to the module being re-encoded.
pub(crate) fn flatten_error(error: ReencodeError<MergeError>, module: &str) -> MergeError {
    match error {
        ReencodeError::UserError(error) => error,
        ReencodeError::ParseError(source) => MergeError::invalid_module(module, source),
        other => MergeError::Reencode {
            module: module.to_string(),
            message: other.to_string(),
        },
    }
}
