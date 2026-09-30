#![no_main]
use chorus_protocol::*;
use libfuzzer_sys::fuzz_target;
fuzz_target!(|data: &[u8]| {
    if let Ok(p) = decode::<FederationProposal>(data) {
        let _ = p.next_state.validate();
        assert_eq!(canonical(&p).unwrap(), data);
    }
    let _ = decode::<FederationApproval>(data);
});
