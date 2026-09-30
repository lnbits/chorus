//! No imports, no WASI, bounded memory and deterministic fuel. Fresh store per execution.
use anyhow::{Result, ensure};
use chorus_protocol::*;
use wasmtime::{Config, Engine, Instance, Module, Store, StoreLimits, StoreLimitsBuilder};
pub const MEMORY_LIMIT: usize = 16 * 1024 * 1024;
pub const OUTPUT_LIMIT: usize = 256 * 1024;
pub const FUEL_LIMIT: u64 = 10_000_000;
#[derive(Clone)]
pub struct FedVm {
    engine: std::sync::Arc<Engine>,
}
impl FedVm {
    pub fn new() -> Result<Self> {
        let mut c = Config::new();
        c.epoch_interruption(true)
            .consume_fuel(true)
            .cranelift_nan_canonicalization(true)
            .wasm_simd(false)
            .wasm_relaxed_simd(false)
            .wasm_memory64(false)
            .wasm_multi_memory(false)
            .wasm_tail_call(false)
            .wasm_gc(false)
            .max_wasm_stack(256 * 1024);
        let engine = std::sync::Arc::new(Engine::new(&c)?);
        let weak = std::sync::Arc::downgrade(&engine);
        std::thread::spawn(move || {
            loop {
                std::thread::sleep(std::time::Duration::from_millis(100));
                let Some(engine) = weak.upgrade() else {
                    break;
                };
                engine.increment_epoch();
            }
        });
        Ok(Self { engine })
    }
    pub fn validate(&self, wasm: &[u8], expected_hash: &str) -> Result<Module> {
        ensure!(
            wasm.len() <= MAX_WASM && hash(wasm) == expected_hash,
            "WASM size or hash mismatch"
        );
        // Validate a deliberately small feature set before compiling attacker-controlled bytes.
        let features = (wasmparser::WasmFeatures::WASM1
            | wasmparser::WasmFeatures::SIGN_EXTENSION
            | wasmparser::WasmFeatures::BULK_MEMORY)
            .difference(wasmparser::WasmFeatures::FLOATS);
        wasmparser::Validator::new_with_features(features).validate_all(wasm)?;
        for item in wasmparser::Parser::new(0).parse_all(wasm) {
            match item? {
                wasmparser::Payload::StartSection { .. } => {
                    anyhow::bail!("start functions are forbidden")
                }
                wasmparser::Payload::ImportSection(imports) => ensure!(
                    imports.count() == 0,
                    "all host imports, including WASI, are forbidden"
                ),
                wasmparser::Payload::CodeSectionStart { count, .. } => {
                    ensure!(count <= 10000, "too many functions")
                }
                _ => {}
            }
        }
        let module = Module::new(&self.engine, wasm)?;
        ensure!(
            module.imports().next().is_none(),
            "all host imports are forbidden"
        );
        Ok(module)
    }
    pub fn execute(&self, wasm: &[u8], expected_hash: &str, input: &[u8]) -> Result<Vec<u8>> {
        ensure!(input.len() <= MAX_INPUT, "input exceeds limit");
        let _: InputEnvelope = decode(input)?;
        let module = self.validate(wasm, expected_hash)?;
        let limits = StoreLimitsBuilder::new()
            .memory_size(MEMORY_LIMIT)
            .memories(1)
            .instances(1)
            .tables(1)
            .table_elements(4096)
            .trap_on_grow_failure(true)
            .build();
        let mut store: Store<StoreLimits> = Store::new(&self.engine, limits);
        store.limiter(|s| s);
        store.set_fuel(FUEL_LIMIT)?;
        // Secondary wall-clock failure only; timeout never produces a signed result.
        store.set_epoch_deadline(50);
        let instance = Instance::new(&mut store, &module, &[])?;
        let memory = instance
            .get_memory(&mut store, "memory")
            .ok_or_else(|| anyhow::anyhow!("missing memory export"))?;
        let alloc = instance.get_typed_func::<i32, i32>(&mut store, "alloc")?;
        let execute = instance.get_typed_func::<(i32, i32), i64>(&mut store, "execute")?;
        let ptr = alloc.call(&mut store, input.len() as i32)? as u32 as usize;
        memory.write(&mut store, ptr, input)?;
        let packed = execute.call(&mut store, (ptr as i32, input.len() as i32))? as u64;
        let out_ptr = (packed >> 32) as usize;
        let len = (packed & 0xffff_ffff) as usize;
        ensure!(len <= OUTPUT_LIMIT, "output exceeds limit");
        let mut output = vec![0; len];
        memory.read(&store, out_ptr, &mut output)?;
        let _: OutputEnvelope = decode(&output)?;
        Ok(output)
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    fn run(wat: &str) -> Result<Vec<u8>> {
        let vm = FedVm::new()?;
        let w = wat::parse_str(wat)?;
        vm.execute(
            &w,
            &hash(&w),
            &canonical(&InputEnvelope {
                previous_state: None,
                command: vec![],
                attestations: vec![],
            })?,
        )
    }
    #[test]
    fn infinite_loop_is_fuel_bounded() {
        assert!(run("(module (memory (export \"memory\") 1) (func (export \"alloc\") (param i32) (result i32) i32.const 0) (func (export \"execute\") (param i32 i32) (result i64) (loop br 0) i64.const 0))").is_err());
    }
    #[test]
    fn memory_bomb_is_rejected() {
        assert!(run("(module (memory (export \"memory\") 65536))").is_err());
    }
    #[test]
    fn imports_forbidden() {
        assert!(run("(module (import \"env\" \"clock\" (func)))").is_err());
    }
    #[test]
    fn malformed_and_wrong_hash() {
        let vm = FedVm::new().unwrap();
        assert!(vm.validate(b"bad", &hash(b"bad")).is_err());
        assert!(
            vm.validate(&wat::parse_str("(module)").unwrap(), &"0".repeat(64))
                .is_err()
        );
    }
    #[test]
    fn oversized_output_and_out_of_bounds_pointer_are_rejected() {
        for packed in [OUTPUT_LIMIT as u64 + 1, (u32::MAX as u64) << 32 | 32] {
            assert!(run(&format!("(module (memory (export \"memory\") 1) (func (export \"alloc\") (param i32) (result i32) i32.const 0) (func (export \"execute\") (param i32 i32) (result i64) i64.const {}))", packed as i64)).is_err());
        }
    }
    #[test]
    fn start_and_float_instructions_are_rejected() {
        let vm = FedVm::new().unwrap();
        for code in [
            "(module (func $start) (start $start))",
            "(module (func (result f32) f32.const 1))",
        ] {
            let bytes = wat::parse_str(code).unwrap();
            assert!(vm.validate(&bytes, &hash(&bytes)).is_err());
        }
    }
    #[test]
    fn deterministic_output() {
        let out = canonical(&OutputEnvelope {
            result: b"hello".to_vec(),
            next_state: None,
            declared_actions: vec![],
        })
        .unwrap();
        let data = out.iter().map(|v| format!("\\{v:02x}")).collect::<String>();
        let code = format!(
            "(module (memory (export \"memory\") 1) (data (i32.const 1024) \"{data}\") (func (export \"alloc\") (param i32) (result i32) i32.const 0) (func (export \"execute\") (param i32 i32) (result i64) i64.const {}))",
            (1024u64 << 32) | out.len() as u64
        );
        assert_eq!(run(&code).unwrap(), out);
        assert_eq!(run(&code).unwrap(), out);
    }
}
