(module
  (type (;0;) (array (mut funcref)))
  (type (;1;) (func (param i32)))
  (type (;2;) (func (param i64)))
  (type (;3;) (func))
  (type (;4;) (func (param (ref 0))))
  (type (;5;) (func (param f64)))
  (type (;6;) (func (param f32)))
  (import "elsewhere" "some.tag" (tag (;0;) (type 5) (param f64)))
  (table (;0;) 10 20 funcref)
  (table (;1;) 30 40 funcref)
  (table (;2;) 50 60 funcref)
  (table (;3;) 70 80 funcref)
  (memory (;0;) 10 20)
  (memory (;1;) 30 40)
  (memory (;2;) 50 60)
  (memory (;3;) 70 80)
  (tag (;1;) (type 1) (param i32))
  (tag (;2;) (type 2) (param i64))
  (tag (;3;) (type 6) (param f32))
  (tag (;4;) (type 5) (param f64))
  (global (;0;) i32 i32.const 1)
  (global (;1;) i32 i32.const 2)
  (global (;2;) i32 i32.const 3)
  (global (;3;) i32 i32.const 4)
  (export "foo" (func 0))
  (export "bar" (func 1))
  (export "keepalive" (func 2))
  (export "foo_1" (func 3))
  (export "other" (func 4))
  (export "keepalive_1" (func 5))
  (export "keepalive.tag" (tag 0))
  (export "other-b" (func 4))
  (elem (;0;) func 0 1)
  (elem (;1;) func 1 0)
  (elem (;2;) func 3 4)
  (elem (;3;) func 4 3)
  (func (;0;) (type 3)
    i32.const 1
    drop
  )
  (func (;1;) (type 3)
    i32.const 2
    drop
  )
  (func (;2;) (type 4) (param (ref 0))
    block (result i32) ;; label = @1
      try_table (catch 1 0 (;@1;)) ;; label = @2
        nop
      end
      i32.const 0
    end
    drop
    block (result i64) ;; label = @1
      try_table (catch 2 0 (;@1;)) ;; label = @2
        nop
      end
      i64.const 0
    end
    drop
    block (result i32) ;; label = @1
      try_table (catch 1 0 (;@1;)) ;; label = @2
        nop
      end
      i32.const 0
    end
    drop
    block (result i64) ;; label = @1
      try_table (catch 2 0 (;@1;)) ;; label = @2
        nop
      end
      i64.const 0
    end
    drop
    i32.const 1
    i32.load
    drop
    i32.const 2
    i32.load 1
    drop
    data.drop 0
    data.drop 1
    i32.const 1
    table.get 0
    drop
    i32.const 2
    table.get 1
    drop
    local.get 0
    i32.const 1
    i32.const 2
    i32.const 3
    array.init_elem 0 0
    local.get 0
    i32.const 4
    i32.const 5
    i32.const 6
    array.init_elem 0 1
    global.get 0
    drop
    global.get 1
    drop
    call 0
    call 1
  )
  (func (;3;) (type 3)
    i32.const 3
    drop
  )
  (func (;4;) (type 3)
    i32.const 4
    drop
  )
  (func (;5;) (type 4) (param (ref 0))
    block (result f32) ;; label = @1
      try_table (catch 3 0 (;@1;)) ;; label = @2
        nop
      end
      f32.const 0x0p+0 (;=0;)
    end
    drop
    block (result f64) ;; label = @1
      try_table (catch 4 0 (;@1;)) ;; label = @2
        nop
      end
      f64.const 0x0p+0 (;=0;)
    end
    drop
    block (result f32) ;; label = @1
      try_table (catch 3 0 (;@1;)) ;; label = @2
        nop
      end
      f32.const 0x0p+0 (;=0;)
    end
    drop
    block (result f64) ;; label = @1
      try_table (catch 4 0 (;@1;)) ;; label = @2
        nop
      end
      f64.const 0x0p+0 (;=0;)
    end
    drop
    i32.const 3
    i32.load 2
    drop
    i32.const 4
    i32.load 3
    drop
    data.drop 2
    data.drop 3
    i32.const 3
    table.get 2
    drop
    i32.const 4
    table.get 3
    drop
    local.get 0
    i32.const 7
    i32.const 8
    i32.const 9
    array.init_elem 0 2
    local.get 0
    i32.const 10
    i32.const 11
    i32.const 12
    array.init_elem 0 3
    global.get 2
    drop
    global.get 3
    drop
    call 3
    call 4
  )
  (data (;0;) (i32.const 1) "abc")
  (data (;1;) (i32.const 2) "def")
  (data (;2;) (memory 2) (i32.const 3) "ghi")
  (data (;3;) (memory 2) (i32.const 4) "jkl")
)
