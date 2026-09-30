#![no_main]
use chorus_protocol::*;
use libfuzzer_sys::fuzz_target;
fuzz_target!(|data: &[u8]| {
    if let Ok(input) = decode::<InputEnvelope>(data) {
        assert_eq!(canonical(&input).unwrap(), data);
    }
    if let Ok(output) = decode::<OutputEnvelope>(data) {
        assert_eq!(canonical(&output).unwrap(), data);
    }
});
