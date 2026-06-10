(module
  (type (;0;) (sub (func (param i32) (result anyref))))
  (type (;1;) (sub final 0 (func (param i32) (result (ref any)))))
  (type (;2;) (sub (func (param i32) (result anyref))))
  (type (;3;) (func (result anyref)))
  (export "sub" (func 0))
  (export "caller" (func 1))
  (export "caller-unreachable" (func 2))
  (func (;0;) (type 1) (param i32) (result (ref any))
    unreachable
  )
  (func (;1;) (type 3) (result anyref)
    block (result anyref) ;; label = @1
      i32.const 42
      call 0
    end
  )
  (func (;2;) (type 3) (result anyref)
    block (result anyref) ;; label = @1
      unreachable
      call 0
    end
  )
)
