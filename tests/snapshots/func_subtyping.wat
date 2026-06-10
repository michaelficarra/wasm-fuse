(module
  (type (;0;) (sub (func)))
  (type (;1;) (sub final 0 (func)))
  (type (;2;) (sub (func)))
  (export "sub" (func 0))
  (export "caller" (func 1))
  (func (;0;) (type 1))
  (func (;1;) (type 2)
    ref.func 0
    call_ref 2
  )
)
