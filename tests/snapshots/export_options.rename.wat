(module
  (type (;0;) (func))
  (type (;1;) (func))
  (export "func" (func 0))
  (export "func_1" (func 1))
  (export "other" (func 2))
  (func (;0;) (type 0)
    i32.const 0
    drop
  )
  (func (;1;) (type 1)
    i32.const 1
    drop
  )
  (func (;2;) (type 1)
    i32.const 2
    drop
  )
)
