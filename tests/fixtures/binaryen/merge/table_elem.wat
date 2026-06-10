
;; RUN: wasm-merge %s first %s.second second --rename-export-conflicts -all -S -o - | filecheck %s

;; Test we rename tables and element segments properly at the module scope.
;; Table $foo has a name collision, and both of the element segments' names do
;; as well. This test verifies that element segments refer to the right tables
;; even after such name changes.

(module
  (type $vec (array funcref))


  (table $foo 1 funcref)

  (table $bar 10 funcref)



  (elem $a (table $foo) (i32.const 0) func)

  (elem $b (table $bar) (i32.const 0) func)





  (func $keepalive2 (export "keepalive2")
    (drop
      (table.get $foo
        (i32.const 1)
      )
    )
    (drop
      (table.get $bar
        (i32.const 1)
      )
    )
    ;; GC operations are the only things that can keep alive an elem segment.
    (drop
      (array.new_elem $vec $a
        (i32.const 1)
        (i32.const 2)
      )
    )
    (drop
      (array.new_elem $vec $b
        (i32.const 3)
        (i32.const 4)
      )
    )
  )
)
