mod common;
use anyhow::Result;
use chorus_nostr::Transport;
use common::*;
use nostr_sdk::prelude::*;
use std::time::Duration;
#[tokio::test]
async fn multiple_relays_offline_restart_and_backfill() -> Result<()> {
    let a = relay().await?;
    let b = relay().await?;
    let keys = Keys::generate();
    let t = Transport::new(keys.clone(), &[config(&a.url), config(&b.url)]).await?;
    t.client.connect().and_wait(Duration::from_secs(3)).await;
    let event = EventBuilder::new(Kind::Custom(17903), "test")
        .tags([Tag::custom("h", ["federation"])])
        .finalize(&keys)?;
    t.publish(&event, &[]).await?;
    let records = t.backfill("federation").await?;
    assert_eq!(records.iter().filter(|e| e.id == event.id).count(), 1);
    // A restarted client recovers signed history from the surviving relay.
    t.client.shutdown().await;
    drop(a);
    let restarted = Transport::new(keys, &[config(&b.url)]).await?;
    restarted
        .client
        .connect()
        .and_wait(Duration::from_secs(3))
        .await;
    assert!(
        restarted
            .backfill("federation")
            .await?
            .iter()
            .any(|e| e.id == event.id)
    );
    restarted.client.shutdown().await;
    Ok(())
}
#[tokio::test]
async fn encrypted_delivery_over_a_real_relay() -> Result<()> {
    let relay = relay().await?;
    let sender = Keys::generate();
    let receiver = Keys::generate();
    let a = Transport::new(sender.clone(), &[config(&relay.url)]).await?;
    let b = Transport::new(receiver.clone(), &[config(&relay.url)]).await?;
    a.client.connect().and_wait(Duration::from_secs(3)).await;
    b.client.connect().and_wait(Duration::from_secs(3)).await;
    let event = EventBuilder::new(Kind::Custom(17903), "private content").finalize(&sender)?;
    a.publish(&event, &[receiver.public_key().to_hex()]).await?;
    let events = b
        .client
        .fetch_events(
            Filter::new()
                .kind(Kind::GiftWrap)
                .pubkey(receiver.public_key()),
        )
        .timeout(Duration::from_secs(3))
        .await?;
    assert_eq!(events.len(), 1);
    assert_eq!(b.unwrap(events.into_iter().next().unwrap()).await?, event);
    a.client.shutdown().await;
    b.client.shutdown().await;
    Ok(())
}
