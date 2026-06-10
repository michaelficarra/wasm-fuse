
;; If asked to, we rename the conflicts. The second "func" export will become
;; "func_1".
;; RUN: wasm-merge %s first %s.second second --rename-export-conflicts -S -o - | filecheck %s --check-prefix RENAME

;; If asked to, we can skip conflicting exports from later modules. The second
;; "func" export will not exist.
;; RUN: wasm-merge %s first %s.second second --skip-export-conflicts -S -o - | filecheck %s --check-prefix SKIP_C

(module







  (func $func0 (export "func")
    ;; This export also appears in the second module.
    (drop
      (i32.const 0)
    )
  )
)


