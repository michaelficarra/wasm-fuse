
;; RUN: wasm-merge %s first %s.second second --rename-export-conflicts -all -S -o - | filecheck %s

;; An empty module. The interesting part is in the second module: we should
;; copy the i64 table properly.
(module
)




