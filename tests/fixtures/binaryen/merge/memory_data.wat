
;; RUN: wasm-merge %s first %s.second second --rename-export-conflicts -all -S -o - | filecheck %s

;; Test we rename memories and data segments properly at the module scope.
;; Memory $bar has a name collision, and both of the element segments' names.
;; This test verifies that data segments refer to the right tables even after
;; such name changes.

(module

  (memory $foo 1)

  (memory $bar 10)

  (memory $shared 10)



  (data $a (memory $foo) (i32.const 0) "a")

  (data $b (memory $bar) (i32.const 0) "b")



  (export "keepalive" (memory $foo))

  (export "keepalive1" (memory $bar))

)


