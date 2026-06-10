(module
  (type (;0;) (func))
  (type (;1;) (func (result i32)))
  (type (;2;) (func))
  (type (;3;) (func (result i32)))
  (import "third" "missing" (func (;0;) (type 0)))
  (memory (;0;) 2)
  (memory (;1;) 2)
  (tag (;0;) (type 0))
  (export "foo" (func 1))
  (export "bar" (func 2))
  (export "keepalive" (func 3))
  (export "mem" (memory 0))
  (export "exn" (tag 0))
  (export "mem_1" (memory 1))
  (export "foo_1" (func 4))
  (export "bar_1" (func 5))
  (export "keepalive2" (func 6))
  (export "keepalive3" (func 7))
  (func (;1;) (type 0)
    i32.const 1
    drop
    call 4
  )
  (func (;2;) (type 0)
    i32.const 2
    drop
    call 5
    call 0
  )
  (func (;3;) (type 1) (result i32)
    i32.const 10
    i32.load 1
  )
  (func (;4;) (type 2)
    call 1
    i32.const 3
    drop
  )
  (func (;5;) (type 2)
    call 2
    i32.const 4
    drop
  )
  (func (;6;) (type 3) (result i32)
    i32.const 10
    i32.load
  )
  (func (;7;) (type 2)
    throw 0
  )
)
