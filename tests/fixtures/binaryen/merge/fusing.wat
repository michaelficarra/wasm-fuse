
;; RUN: wasm-merge %s first %s.second second --rename-export-conflicts -all -S -o - | filecheck %s

;; Test that we fuse imports to exports across modules.
;;
;; We test functions and memories here, and not every possible entity in a
;; comprehensive way, since they all go through the same code path. (But we test
;; two to at least verify we differentiate them.)
;;
;; We also test importing memories and tags from another file than the
;; first one, which was initially broken.

(module
  ;; The first two imports here will be resolved to direct calls into the
  ;; second module's merged contents.
  (import "second" "foo" (func $other.foo))

  (import "second" "bar" (func $other.bar))

  (import "second" "mem" (memory $other.mem 1))

  ;; This import will remain unresolved.


  (import "third" "missing" (func $other.missing))














  (func $first.foo (export "foo")
    (drop
      (i32.const 1)
    )
    (call $other.foo)
  )

  (func $bar (export "bar")
    (drop
      (i32.const 2)
    )
    (call $other.bar)
    (call $other.missing)
  )

  (func $keepalive (export "keepalive") (result i32)
    ;; Load from the memory imported from the second module.
    (i32.load $other.mem
      (i32.const 10)
    )
  )

  (memory $first.mem 2)

  (export "mem" (memory $first.mem))

  (tag $exn (export "exn"))
)



