(module
  (type (;0;) (func))
  (type (;1;) (func))
  (type (;2;) (func))
  (export "forward" (func 0))
  (export "reverse" (func 1))
  (export "forward_1" (func 2))
  (export "reverse_1" (func 3))
  (export "forward_2" (func 4))
  (export "reverse_2" (func 5))
  (func (;0;) (type 0)
    i32.const 1
    drop
    call 2
  )
  (func (;1;) (type 0)
    i32.const -1
    drop
    call 5
  )
  (func (;2;) (type 1)
    i32.const 2
    drop
    call 4
  )
  (func (;3;) (type 1)
    i32.const -2
    drop
    call 1
  )
  (func (;4;) (type 2)
    i32.const 3
    drop
    call 0
  )
  (func (;5;) (type 2)
    i32.const -3
    drop
    call 3
  )
)
