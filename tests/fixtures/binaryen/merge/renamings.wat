;; RUN: wasm-merge %s first %s.second second --rename-export-conflicts -all -S -o - | filecheck %s

;; Test that we rename items in the second module to avoid name collisions.

(module
  (type $array (array (mut (ref null func))))

  ;; This tag has a conflict in second.wat, and so second.wat's $foo
  ;; will be renamed.







  (global $foo i32 (i32.const 1))

  ;; This global has a conflict in second.wat, and so second.wat's $bar
  ;; will be renamed.
  (global $bar i32 (i32.const 2))

  ;; This memory has a conflict in second.wat, and so second.wat's $foo
  ;; will be renamed.


  (memory $foo 10 20)

  (memory $bar 30 40)



  (data $foo (i32.const 1) "abc")

  ;; This data segment has a conflict in second.wat, and so second.wat's $bar
  ;; will be renamed.
  (data $bar (i32.const 2) "def")

  ;; This table has a conflict in second.wat, and so second.wat's $foo
  ;; will be renamed.


  (table $foo 10 20 funcref)

  (table $bar 30 40 funcref)



  (elem $foo func $foo $bar)

  ;; This elem has a conflict in second.wat, and so second.wat's $bar
  ;; will be renamed.
  (elem $bar func $bar $foo)



  (tag $foo (param i32))

  (tag $bar (param i64))

  ;; This export has a conflict in second.wat, and so second.wat's $foo
  ;; will be renamed.


  (export "foo" (func $foo))

  (export "bar" (func $bar))

  (export "keepalive" (func $uses))






  (func $foo
    ;; This function has a conflict in second.wat, and so second.wat's $foo
    ;; will be renamed.
    (drop
      (i32.const 1)
    )
  )

  (func $bar
    (drop
      (i32.const 2)
    )
  )

  (func $uses (param $array (ref $array))
    ;; Tags.
    ;; Adapted for wasm-fuse: binaryen's legacy (try (do) (catch ... (pop)))
    ;; here was rewritten as standard try_table; see NOTICE.
    (drop
      (block $legacy_catch_foo (result i32)
        (try_table (catch $foo $legacy_catch_foo)
          (nop)
        )
        (i32.const 0)
      )
    )
    (drop
      (block $legacy_catch_bar (result i64)
        (try_table (catch $bar $legacy_catch_bar)
          (nop)
        )
        (i64.const 0)
      )
    )
    (drop
      (block $catch (result i32)
        (try_table (catch $foo $catch)
          (nop)
        )
        (i32.const 0)
      )
    )
    (drop
      (block $catch (result i64)
        (try_table (catch $bar $catch)
          (nop)
        )
        (i64.const 0)
      )
    )

    ;; Memories
    (drop
      (i32.load $foo
        (i32.const 1)
      )
    )
    (drop
      (i32.load $bar
        (i32.const 2)
      )
    )

    ;; Data segments
    (data.drop $foo)
    (data.drop $bar)

    ;; Tables
    (drop
      (table.get $foo
        (i32.const 1)
      )
    )
    (drop
      (table.get $bar
        (i32.const 2)
      )
    )

    ;; Element segments
    (array.init_elem $array $foo
      (local.get $array)
      (i32.const 1)
      (i32.const 2)
      (i32.const 3)
    )
    (array.init_elem $array $bar
      (local.get $array)
      (i32.const 4)
      (i32.const 5)
      (i32.const 6)
    )

    ;; Globals
    (drop
      (global.get $foo)
    )
    (drop
      (global.get $bar)
    )

    ;; Functions.
    (call $foo)
    (call $bar)
  )
)


