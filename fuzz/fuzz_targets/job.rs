#![no_main]
use chorus_protocol::*;
use libfuzzer_sys::fuzz_target;
fuzz_target!(|data: &[u8]| {
    if let Ok(job) = decode::<Job>(data) {
        let _ = decode::<InputEnvelope>(&job.input);
        assert_eq!(canonical(&job).unwrap(), data);
    }
    let _ = decode::<JobAttestation>(data);
});
