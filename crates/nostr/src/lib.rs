//! Untrusted multi-relay transport. Membership/epochs are validated by the engine.
use anyhow::{Result, ensure};
use chorus_protocol::MAX_MESSAGE;
use nostr_sdk::prelude::*;
use serde::{Deserialize, Serialize};
use std::{sync::Arc, time::Duration};
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct RelayConfig {
    pub url: String,
    pub enabled: bool,
    pub read: bool,
    pub write: bool,
    pub private: bool,
}
/// Initial node configuration; saved relay choices, including an empty list, take precedence.
pub fn default_relays() -> Vec<RelayConfig> {
    [
        "wss://relay.nostr.com",
        "wss://relay.damus.io",
        "wss://nos.lol",
        "wss://nostr.bitcoiner.social",
        "wss://nostr.mom",
        "wss://relay.snort.social",
    ]
    .into_iter()
    .map(|url| RelayConfig {
        url: url.into(),
        enabled: true,
        read: true,
        write: true,
        private: false,
    })
    .collect()
}
#[derive(Clone)]
pub struct Transport {
    pub client: Client,
    keys: Arc<Keys>,
}
impl Transport {
    pub async fn new(keys: Keys, relays: &[RelayConfig]) -> Result<Self> {
        let mut limits = RelayLimits::default();
        limits.messages.max_size = Some((MAX_MESSAGE * 4) as u32);
        limits.events.max_size = Some((MAX_MESSAGE * 3) as u32);
        let client = Client::builder().relay_limits(limits).build();
        let transport = Self {
            client,
            keys: Arc::new(keys),
        };
        transport.configure(relays).await?;
        Ok(transport)
    }
    pub async fn configure(&self, relays: &[RelayConfig]) -> Result<()> {
        self.client.remove_all_relays().await?;
        for r in relays.iter().filter(|r| r.enabled) {
            ensure!(
                r.url.starts_with("wss://") || r.url.starts_with("ws://127.0.0.1:"),
                "TLS relay required except loopback tests"
            );
            let mut capabilities = RelayCapabilities::from_bits(0);
            if r.read {
                capabilities |= RelayCapabilities::READ;
            }
            if r.write {
                capabilities |= RelayCapabilities::WRITE;
            }
            self.client
                .add_relay(&r.url)
                .capabilities(capabilities)
                .await?;
        }
        self.client.connect().await;
        Ok(())
    }
    pub async fn subscribe(
        &self,
        federations: &[String],
        communication_key: PublicKey,
    ) -> Result<()> {
        if self.client.relays().await.is_empty() {
            return Ok(());
        }
        self.client.unsubscribe_all().await?;
        if !federations.is_empty() {
            self.client
                .subscribe(
                    Filter::new()
                        .kinds((17900..=17906).map(Kind::Custom))
                        .custom_tags(SingleLetterTag::from_char('h')?, federations.to_vec()),
                )
                .await?;
        }
        self.client
            .subscribe(Filter::new().kind(Kind::GiftWrap).pubkey(communication_key))
            .await?;
        Ok(())
    }
    pub async fn backfill(&self, federation: &str) -> Result<Vec<Event>> {
        let events = self
            .client
            .fetch_events(
                Filter::new()
                    .kinds((17900..=17906).map(Kind::Custom))
                    .custom_tag(SingleLetterTag::from_char('h')?, federation)
                    .limit(5000),
            )
            .timeout(Duration::from_secs(10))
            .await?;
        let mut events: Vec<_> = events.into_iter().collect();
        events.sort_by_key(|e| (e.created_at, e.kind.as_u16(), e.id));
        Ok(events)
    }
    pub async fn private_backfill(&self) -> Result<Vec<Event>> {
        Ok(self
            .client
            .fetch_events(
                Filter::new()
                    .kind(Kind::GiftWrap)
                    .pubkey(self.keys.public_key())
                    .limit(5000),
            )
            .timeout(Duration::from_secs(10))
            .await?
            .into_iter()
            .collect())
    }
    pub async fn publish(&self, event: &Event, recipients: &[String]) -> Result<()> {
        if recipients.is_empty() {
            let output = self.client.send_event(event).await?;
            ensure!(!output.success.is_empty(), "no relay acknowledged event");
        } else {
            for recipient in recipients {
                let wrapped = PrivateDirectMessageBuilder::new(
                    PublicKey::from_hex(recipient)?,
                    event.as_json(),
                )
                .finalize(self.keys.as_ref())?;
                let output = self.client.send_event(&wrapped).await?;
                ensure!(!output.success.is_empty(), "private event unacknowledged");
            }
        }
        Ok(())
    }
    pub async fn unwrap(&self, event: Event) -> Result<Event> {
        ensure!(event.content.len() <= MAX_MESSAGE * 3, "oversized event");
        event.verify()?;
        if event.kind != Kind::GiftWrap {
            return Ok(event);
        }
        let unwrapped = UnwrappedGift::from_gift_wrap(self.keys.as_ref(), &event)?;
        ensure!(
            unwrapped.rumor.kind == Kind::PrivateDirectMessage,
            "not a private protocol message"
        );
        let event = Event::from_json(&unwrapped.rumor.content)?;
        event.verify()?;
        Ok(event)
    }
    pub async fn health(&self) -> Vec<serde_json::Value> {
        let mut rows = vec![];
        for (url, relay) in self.client.relays().await {
            rows.push(serde_json::json!({"url":url.to_string(),"status":format!("{:?}",relay.status()).to_lowercase()}));
        }
        rows.sort_by_key(|v| v["url"].as_str().unwrap_or("").to_owned());
        rows
    }
}
