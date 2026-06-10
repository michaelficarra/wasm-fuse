
;; RUN: wasm-merge %s first %s.second second -S -o - | filecheck %s --check-prefix=CHECK-TEXT
;; RUN: wasm-as %s -o %t.wasm --source-map %t.map
;; RUN: wasm-as %s.second -o %t.second.wasm --source-map %t.second.map
;; RUN: wasm-merge %t.wasm first --input-source-map %t.map  %t.second.wasm second --input-source-map %t.second.map -o %t.merged.wasm --output-source-map %t.merged.map
;; RUN: wasm-dis %t.merged.wasm --source-map %t.merged.map -o - | filecheck %s --check-prefix=CHECK-BIN

;; Test that sourcemap information is preserved

(module
  ;;@ a:1:2
  (func (export "f")
     ;;@ a:3:4:myFunction
     (nop)
     ;;@ a:5:6
  )
)









