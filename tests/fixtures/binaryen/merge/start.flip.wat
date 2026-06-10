;; RUN: wasm-merge %s first %s.second second --rename-export-conflicts -all -S -o - | filecheck %s

;; Like start, but flipped - now only the first module has a start.

(module


  (start $start)

  (func $start
    (drop
      (i32.const 0)
    )
  )
)

