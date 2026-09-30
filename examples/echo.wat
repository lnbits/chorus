;; Chorus ABI v1 stateless echo. The host validates the canonical envelope.
;; Input header = 26 bytes; previous_state=None = 1 byte;
;; command length = 4 bytes little endian; command begins at offset 31.
(module
  (memory (export "memory") 16 256)
  ;; "CHORUS\0", version=1, domain-length=16, "execution-output"
  (data (i32.const 524288) "CHORUS\00\01\00\10\00execution-output")
  (func (export "alloc") (param i32) (result i32) i32.const 4096)
  (func (export "execute") (param $input i32) (param $length i32) (result i64)
    (local $size i32)
    ;; Stateful input is deliberately unsupported by this example.
    local.get $input i32.const 26 i32.add i32.load8_u
    if unreachable end
    local.get $input i32.const 27 i32.add i32.load local.set $size
    i32.const 524315 local.get $size i32.store
    i32.const 524319 local.get $input i32.const 31 i32.add local.get $size memory.copy
    ;; next_state=None and empty declared_actions (5 bytes).
    i32.const 524319 local.get $size i32.add i64.const 0 i64.store
    i64.const 2251799813685248
    local.get $size i32.const 36 i32.add i64.extend_i32_u i64.or))
