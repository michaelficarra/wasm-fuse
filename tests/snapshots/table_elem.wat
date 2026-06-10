(module
  (type (;0;) (array funcref))
  (type (;1;) (func))
  (table (;0;) 1 funcref)
  (table (;1;) 10 funcref)
  (table (;2;) 100 funcref)
  (table (;3;) 1000 funcref)
  (export "keepalive2" (func 0))
  (export "keepalive2_1" (func 1))
  (elem (;0;) (table 0) (i32.const 0) func)
  (elem (;1;) (table 1) (i32.const 0) func)
  (elem (;2;) (table 2) (i32.const 0) func)
  (elem (;3;) (table 3) (i32.const 0) func)
  (func (;0;) (type 1)
    i32.const 1
    table.get 0
    drop
    i32.const 1
    table.get 1
    drop
    i32.const 1
    i32.const 2
    array.new_elem 0 0
    drop
    i32.const 3
    i32.const 4
    array.new_elem 0 1
    drop
  )
  (func (;1;) (type 1)
    i32.const 1
    table.get 2
    drop
    i32.const 1
    table.get 3
    drop
    i32.const 5
    i32.const 6
    array.new_elem 0 2
    drop
    i32.const 7
    i32.const 8
    array.new_elem 0 3
    drop
  )
)
