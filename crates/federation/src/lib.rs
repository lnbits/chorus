//! Signature boundaries: membership is always checked against an authenticated epoch.
use anyhow::{Result, ensure};
use chorus_protocol::*;
use nostr_sdk::prelude::*;
pub const STATE: u16 = 17900;
pub const PROPOSAL: u16 = 17901;
pub const APPROVAL: u16 = 17902;
pub const JOB: u16 = 17903;
pub const ATTESTATION: u16 = 17904;
pub const FINALIZED: u16 = 17905;
pub const CONTRACT: u16 = 17906;

pub fn signed<T: Canonical>(keys: &Keys, kind: u16, federation: &str, value: &T) -> Result<Event> {
    Ok(
        EventBuilder::new(Kind::Custom(kind), hex_encode(&canonical(value)?))
            .tags([
                Tag::custom("h", [federation]),
                Tag::custom("protocol", ["chorus/1"]),
            ])
            .finalize(keys)?,
    )
}
fn hex_encode(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}
pub fn payload<T: Canonical>(event: &Event, kind: u16, federation: &str) -> Result<T> {
    ensure!(
        event.content.len() <= MAX_MESSAGE * 2 && event.tags.len() <= 16,
        "event exceeds limit"
    );
    event.verify()?;
    ensure!(event.kind == Kind::Custom(kind), "wrong event kind");
    ensure!(
        event
            .tags
            .iter()
            .filter(|t| t.as_slice().first().is_some_and(|v| v == "h"))
            .count()
            == 1,
        "ambiguous federation tag"
    );
    ensure!(
        event.tags.iter().any(|t| t.as_slice() == ["h", federation]),
        "wrong federation tag"
    );
    let s = event.content.as_bytes();
    ensure!(
        s.len().is_multiple_of(2)
            && s.iter()
                .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(b)),
        "noncanonical event hex"
    );
    let bytes: Result<Vec<u8>, _> = s
        .as_chunks::<2>()
        .0
        .iter()
        .map(|p| u8::from_str_radix(std::str::from_utf8(p).unwrap_or("!"), 16))
        .collect();
    decode(&bytes?)
}
pub fn member(event: &Event, state: &FederationState) -> Result<String> {
    let member = event.pubkey.to_hex();
    ensure!(
        state.members.binary_search(&member).is_ok(),
        "signer is not an epoch member"
    );
    Ok(member)
}
pub fn validate_genesis(event: &Event, pinned_hash: &str) -> Result<FederationState> {
    let federation = event
        .tags
        .iter()
        .find_map(|t| {
            let s = t.as_slice();
            if s.first().is_some_and(|v| v == "h") {
                s.get(1)
            } else {
                None
            }
        })
        .ok_or_else(|| anyhow::anyhow!("missing federation tag"))?;
    let state: FederationState = payload(event, STATE, federation)?;
    state.validate()?;
    ensure!(
        state.epoch == 0 && digest(&state)? == pinned_hash,
        "genesis does not match trust anchor"
    );
    member(event, &state)?;
    for key in &state.members {
        PublicKey::from_hex(key)?;
    }
    Ok(state)
}
