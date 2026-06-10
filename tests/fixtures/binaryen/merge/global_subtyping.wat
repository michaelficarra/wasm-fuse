
;; RUN: wasm-merge %s primary %s.second secondary --skip-export-conflicts -all -S -o - | filecheck %s

;; Export a global with a subtype. It is imported using the supertype, and
;; after we merge, the type must be updated, including in the global.get from
;; the second module (that goes from super to sub).
(module
 (type $super (sub (func)))
 (type $sub (sub final $super (func)))


 (global $sub (ref $sub) (ref.func $sub))

 (export "sub" (global $sub))


 (func $sub (type $sub)
  (drop
   (global.get $sub)
  )
 )
)

