;; RUN: wasm-merge %s first %s.second second %s.third third --rename-export-conflicts -all -S -o - | filecheck %s

;; Test a cycle of imports: the first module imports from the second, which
;; imports from the third, and we have a reverse cycle as well.

(module
  (import "second" "forward" (func $second.forward))

  (import "second" "reverse" (func $second.reverse))

  (import "third" "forward" (func $third.forward))

  (import "third" "reverse" (func $third.reverse))










  (func $forward (export "forward")
    (drop
      (i32.const 1)
    )
    (call $second.forward)
  )

  (func $reverse (export "reverse")
    (drop
      (i32.const -1)
    )
    (call $third.reverse)
  )
)



