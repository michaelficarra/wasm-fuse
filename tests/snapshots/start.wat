(module
  (type (;0;) (func))
  (type (;1;) (func))
  (export "start" (func 1))
  (export "user" (func 2))
  (start 1)
  (func (;0;) (type 0)
    i32.const 0
    drop
  )
  (func (;1;) (type 1)
    i32.const 1
    drop
  )
  (func (;2;) (type 1)
    call 1
    call 1
  )
)
