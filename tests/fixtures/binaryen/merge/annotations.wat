
;; RUN: wasm-merge %s first %s.second second -all -S -o - | filecheck %s

;; Test that we handle code annotations properly. Both the first and second
;; wasm files have annotations that should be preserved.

(module



  (@binaryen.js.called)
  (func $first (export "first")
    (@metadata.code.branch_hint "\00")
    (if
      (i32.const 0)
      (then
        (return)
      )
    )
  )
)

