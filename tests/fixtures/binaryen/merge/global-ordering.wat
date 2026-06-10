
;; RUN: wasm-merge %s first %s.second second -all -S -o - | filecheck %s

;; After the merge this module's global will read a value from a global that is
;; appended after it, from the second module. Those must be reordered so that
;; we validate, as a global can only read from previous ones.

(module
  (import "second" "second.global.export" (global i32))



  (global $first.global (mut i32) (global.get 0))



  (func $run (export "run") (result i32)
    ;; Use the global to avoid it being removed.
    (global.get $first.global)
  )
)
