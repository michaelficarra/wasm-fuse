
;; RUN: wasm-merge %s primary %s.second secondary --skip-export-conflicts -all -S -o - | filecheck %s

;; Export a function with a subtype. It is imported using the supertype, and
;; after we merge, the refined return type must be updated - the call
;; instruction now returns something new.
(module

 (type $super (sub (func (param i32) (result anyref))))

 (type $sub (sub final $super (func (param i32) (result (ref any)))))




 (func $sub (export "sub") (type $sub)
  (unreachable)
 )
)


