(module
  (type (;0;) (func))
  (export "first" (func 0))
  (export "second" (func 1))
  (func (;0;) (type 0)
    (@metadata.code.branch_hint "/00")
    i32.const 0
    if ;; label = @1
      return
    end
  )
  (func (;1;) (type 0)
    (@metadata.code.branch_hint "/01")
    i32.const 0
    if ;; label = @1
      return
    end
  )
)
