#![no_main]
use chorus_protocol::*;
use libfuzzer_sys::fuzz_target;
use nostr_sdk::prelude::*;
fuzz_target!(|data: &[u8]| {
    if data.len() > MAX_MESSAGE * 2 {
        return;
    }
    if let Ok(event) = Event::from_json(data) {
        if let Some(tag) = event
            .tags
            .iter()
            .find(|t| t.as_slice().first().is_some_and(|s| s == "h"))
        {
            if let Some(fid) = tag.as_slice().get(1) {
                let _ = chorus_federation::payload::<Job>(&event, chorus_federation::JOB, fid);
            }
        }
    }
});
