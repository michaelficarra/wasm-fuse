
;; RUN: wasm-merge %s first %s.second second -all -S -o - | filecheck %s

;; Test that we handle code annotations properly when they appear *only* in
;; functions (not code). Both the first and second wasm files have an annotation
;; that should be preserved.

(module



  (@binaryen.js.called)
  (func $first (export "first")
    (if
      (i32.const 0)
      (then
        (return)
      )
    )
  )
)

