;; RUN: wasm-merge %s first %s.second second %s.third third --rename-export-conflicts -all -S -o - | filecheck %s

;; Test chains of imports / exports: the first module export a function,
;; which is reexported by the second and imported by the third.

(module
  (func (export "f"))
)





