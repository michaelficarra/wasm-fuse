
;; RUN: wasm-merge %s primary %s.second secondary --skip-export-conflicts -all -S -o - | filecheck %s

;; Export a function with a subtype. It is imported using the supertype, and
;; after we merge, the type must be updated. That update will then propagate to
;; the call_ref.
(module
 (type $super (sub (func)))
 (type $sub (sub final $super (func)))





 (func $sub (export "sub") (type $sub)
 )
)

