;; RUN: wasm-merge -g %s first %s.second second -all -o %t.wasm
;; RUN: wasm-opt -all %t.wasm -S -o - | filecheck %s
(module











































   (func $func0 (export "f0"))
   (func (export "f1"))

   (table $table0 (export "t0") 1 funcref)
   (table (export "t1") 1 funcref)

   (global $glob0 (export "g0") i32 (i32.const 0))
   (global (export "g1") i32 (i32.const 0))

   (memory $mem0 (export "m0") 0)
   (memory (export "m1") 0)

   (elem $elem0 func)
   (elem func)

   (data $data0 "")
   (data "")

   (tag $tag0 (export "tag0"))
   (tag (export "tag1"))

   (type $t (struct (field $a i32) (field $b i32)))

   (func (export "func") (param $x (ref $t)))
)




