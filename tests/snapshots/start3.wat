(module
  (type (;0;) (func))
  (export "start" (func 0))
  (export "user" (func 1))
  (start 3)
  (func (;0;) (type 0)
    (local i32)
    local.get 0
    drop
    i32.const 1
    drop
  )
  (func (;1;) (type 0)
    call 0
    call 0
  )
  (func (;2;) (type 0)
    (local f64)
    local.get 0
    drop
    i32.const 2
    drop
  )
  (func (;3;) (type 0)
    call 0
    call 2
  )
)
