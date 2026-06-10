;; RUN: wasm-merge %s first %s.second second %s.third third -all -S -o - | filecheck %s

;; Test that we merge start functions. The first module here has none, but the
;; second and third do, so we'll first copy in the second's and then merge in
;; the third's.

(module
)







