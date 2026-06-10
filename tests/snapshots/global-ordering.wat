(module
  (type (;0;) (func (result i32)))
  (global (;0;) i32 i32.const 42)
  (global (;1;) (mut i32) global.get 0)
  (export "run" (func 0))
  (export "second.global.export" (global 0))
  (func (;0;) (type 0) (result i32)
    global.get 1
  )
)
