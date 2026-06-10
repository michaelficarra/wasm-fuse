;; RUN: wasm-merge %s first %s.second second %s.third third --output-manifest %t.manifest -o %t.wasm
;; RUN: cat %t.manifest | filecheck %s
;; RUN: wasm-dis %t.wasm -o - | filecheck %s --check-prefix MERGED

;; The first module is the primary module and does not appear in the manifest.


;; The binary should contain the original function names.

(module
  (import "env" "imported_first" (func $imported_first))
  (func $foo (export "foo")
    (call $imported_first)
  )
  (func $bar (export "bar")
    nop
  )
)
