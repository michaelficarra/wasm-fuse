(module
  (type (;0;) (sub (func)))
  (type (;1;) (sub final 0 (func)))
  (type (;2;) (sub (func)))
  (global (;0;) (ref 1) ref.func 0)
  (export "sub" (global 0))
  (export "second-user" (func 1))
  (func (;0;) (type 1)
    global.get 0
    drop
  )
  (func (;1;) (type 2)
    block (result (ref 2)) ;; label = @1
      global.get 0
    end
    drop
  )
)
