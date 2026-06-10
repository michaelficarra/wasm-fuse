;; RUN: wasm-merge %s first %s.second second --rename-export-conflicts -all -S -o - | filecheck %s

;; Test that we properly merge start functions with control flow. The two
;; start functions have returns, and a naive merge of their bodies would end up
;; skipping the second (after the first return).

(module
  (start $start-a)



  (func $start-a
    (drop
      (i32.const 0)
    )
    (return)
  )
)


