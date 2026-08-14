//! WebAssembly feature selection independent of the parser implementation.

bitflags::bitflags! {
    /// WebAssembly proposals accepted while parsing and validating modules.
    ///
    /// This type is owned by wasm-fuse so upgrading its internal Wasm tools
    /// dependencies does not change the public API's type identity.
    #[derive(Clone, Copy, Debug, Hash, PartialEq, Eq)]
    pub struct WasmFeatures: u64 {
        /// The mutable-global proposal.
        const MUTABLE_GLOBAL = 1;
        /// The saturating-float-to-int proposal.
        const SATURATING_FLOAT_TO_INT = 1 << 1;
        /// The sign-extension-ops proposal.
        const SIGN_EXTENSION = 1 << 2;
        /// The reference-types proposal.
        const REFERENCE_TYPES = (1 << 3) | Self::CALL_INDIRECT_OVERLONG.bits();
        /// The multi-value proposal.
        const MULTI_VALUE = 1 << 4;
        /// The bulk-memory proposal.
        const BULK_MEMORY = (1 << 5) | Self::BULK_MEMORY_OPT.bits();
        /// The SIMD proposal.
        const SIMD = 1 << 6;
        /// The relaxed-SIMD proposal.
        const RELAXED_SIMD = 1 << 7;
        /// The threads proposal.
        const THREADS = 1 << 8;
        /// The shared-everything-threads proposal.
        const SHARED_EVERYTHING_THREADS = 1 << 9;
        /// The tail-call proposal.
        const TAIL_CALL = 1 << 10;
        /// Floating-point instructions and types.
        const FLOATS = 1 << 11;
        /// The multi-memory proposal.
        const MULTI_MEMORY = 1 << 12;
        /// The exception-handling proposal.
        const EXCEPTIONS = 1 << 13;
        /// The memory64 proposal.
        const MEMORY64 = 1 << 14;
        /// The extended-const proposal.
        const EXTENDED_CONST = 1 << 15;
        /// The component-model proposal.
        const COMPONENT_MODEL = 1 << 16;
        /// The typed-function-references proposal.
        const FUNCTION_REFERENCES = 1 << 17;
        /// The memory-control proposal.
        const MEMORY_CONTROL = 1 << 18;
        /// The garbage-collection proposal.
        const GC = 1 << 19;
        /// The custom-page-sizes proposal.
        const CUSTOM_PAGE_SIZES = 1 << 20;
        /// Component-model value types.
        const CM_VALUES = 1 << 21;
        /// Component-model nested names.
        const CM_NESTED_NAMES = 1 << 22;
        /// The legacy exception-handling proposal.
        const LEGACY_EXCEPTIONS = 1 << 23;
        /// Garbage-collected reference types.
        const GC_TYPES = 1 << 24;
        /// The stack-switching proposal.
        const STACK_SWITCHING = 1 << 25;
        /// The wide-arithmetic proposal.
        const WIDE_ARITHMETIC = 1 << 26;
        /// Component-model asynchronous built-ins and types.
        const CM_ASYNC = 1 << 27;
        /// The component model's stackful asynchronous ABI.
        const CM_ASYNC_STACKFUL = 1 << 28;
        /// Additional component-model asynchronous built-ins.
        const CM_MORE_ASYNC_BUILTINS = 1 << 29;
        /// Component-model threading.
        const CM_THREADING = 1 << 30;
        /// Component-model error-context annotations.
        const CM_ERROR_CONTEXT = 1 << 31;
        /// Component-model fixed-length lists.
        const CM_FIXED_LENGTH_LISTS = 1 << 32;
        /// Garbage-collected component-model values.
        const CM_GC = 1 << 33;
        /// Overlong `call_indirect` table-index encodings.
        const CALL_INDIRECT_OVERLONG = 1 << 34;
        /// The `memory.copy` and `memory.fill` subset of bulk memory.
        const BULK_MEMORY_OPT = 1 << 35;
        /// The custom-descriptors proposal.
        const CUSTOM_DESCRIPTORS = 1 << 36;
        /// The compact-imports proposal.
        const COMPACT_IMPORTS = 1 << 37;
        /// Component-model map values.
        const CM_MAP = 1 << 38;
        /// 64-bit component-model contexts.
        const CM64 = 1 << 39;
        /// Component-model `implements` directives.
        const CM_IMPLEMENTS = 1 << 40;
        /// Canonical component-model names with version suffixes.
        const CM_CANON_NAMES = 1 << 41;
    }
}

impl Default for WasmFeatures {
    fn default() -> Self {
        Self::MUTABLE_GLOBAL
            | Self::SATURATING_FLOAT_TO_INT
            | Self::SIGN_EXTENSION
            | Self::REFERENCE_TYPES
            | Self::MULTI_VALUE
            | Self::BULK_MEMORY
            | Self::SIMD
            | Self::RELAXED_SIMD
            | Self::THREADS
            | Self::TAIL_CALL
            | Self::FLOATS
            | Self::MULTI_MEMORY
            | Self::EXCEPTIONS
            | Self::MEMORY64
            | Self::EXTENDED_CONST
            | Self::COMPONENT_MODEL
            | Self::FUNCTION_REFERENCES
            | Self::GC
            | Self::GC_TYPES
            | Self::CM_ASYNC
    }
}

impl WasmFeatures {
    /// Convert the stable public feature set into the current parser's private
    /// representation.
    ///
    /// The bit assignments currently coincide. Keeping this conversion inside
    /// the crate allows a future Wasmparser upgrade to translate them
    /// individually if its representation changes.
    pub(crate) fn to_wasmparser(self) -> wasmparser::WasmFeatures {
        wasmparser::WasmFeatures::from_bits_retain(self.bits())
    }
}
