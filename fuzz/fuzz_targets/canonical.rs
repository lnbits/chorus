#![no_main]
use chorus_protocol::*;
use libfuzzer_sys::fuzz_target;
fuzz_target!(|data: &[u8]| {
    if let Ok(state) = decode::<FederationState>(data) {
        assert_eq!(canonical(&state).unwrap(), data);
        let _ = state.validate();
        let _ = digest(&state);
    }
});
