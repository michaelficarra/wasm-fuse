(module
  (type (;0;) (func))
  (type (;1;) (func))
  (type (;2;) (func))
  (start 2)
  (func (;0;) (type 0)
    i32.const 0
    drop
    return
  )
  (func (;1;) (type 1)
    i32.const 1
    drop
    return
  )
  (func (;2;) (type 2)
    call 0
    call 1
  )
)
