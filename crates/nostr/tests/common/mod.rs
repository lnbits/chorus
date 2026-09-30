//! Real WebSocket transport tests against an in-process, signature-checking Nostr relay.
use anyhow::Result;
use chorus_nostr::RelayConfig;
use futures_util::{SinkExt, StreamExt};
use nostr_sdk::prelude::*;
use serde_json::{Value, json};
use std::sync::Arc;
use tokio::{
    net::TcpListener,
    sync::{Mutex, broadcast},
};
use tokio_tungstenite::{accept_async, tungstenite::Message};
pub struct Relay {
    pub url: String,
    task: tokio::task::JoinHandle<()>,
}
impl Drop for Relay {
    fn drop(&mut self) {
        self.task.abort();
    }
}
pub async fn relay() -> Result<Relay> {
    let listener = TcpListener::bind("127.0.0.1:0").await?;
    let url = format!("ws://{}", listener.local_addr()?);
    let history = Arc::new(Mutex::new(Vec::<Event>::new()));
    let (tx, _) = broadcast::channel::<Event>(1024);
    let task = tokio::spawn(async move {
        while let Ok((tcp, _)) = listener.accept().await {
            let history = history.clone();
            let tx = tx.clone();
            let mut rx = tx.subscribe();
            tokio::spawn(async move {
                let Ok(mut ws) = accept_async(tcp).await else {
                    return;
                };
                let mut subscriptions = std::collections::BTreeMap::<String, Filter>::new();
                loop {
                    tokio::select! {
                     message=ws.next()=>{let text=match message {
                       Some(Ok(Message::Text(t)))=>t,
                       Some(Ok(Message::Ping(p)))=>{let _=ws.send(Message::Pong(p)).await;continue;},
                       Some(Ok(Message::Pong(_)))=>continue,
                       _=>break,
                     };let Ok(v)=serde_json::from_str::<Value>(&text) else{continue;};match v[0].as_str(){
                      Some("EVENT")=>{if let Ok(event)=serde_json::from_value::<Event>(v[1].clone()){let valid=event.verify().is_ok();let _=ws.send(Message::Text(json!(["OK",event.id,valid,""]).to_string().into())).await;if valid{let mut h=history.lock().await;if !h.iter().any(|e|e.id==event.id){h.push(event.clone());let _=tx.send(event);}}}},
                      Some("REQ")=>{let id=v[1].as_str().unwrap_or_default().to_owned();if let Ok(filter)=serde_json::from_value::<Filter>(v[2].clone()){for event in history.lock().await.iter().filter(|e|filter.match_event(e,MatchEventOptions::default())){let _=ws.send(Message::Text(json!(["EVENT",id,event]).to_string().into())).await;}let _=ws.send(Message::Text(json!(["EOSE",id]).to_string().into())).await;subscriptions.insert(id,filter);}},
                      Some("CLOSE")=>{subscriptions.remove(v[1].as_str().unwrap_or_default());},_=>{}
                     }},
                     event=rx.recv()=>{if let Ok(event)=event {for(id,filter)in &subscriptions {if filter.match_event(&event,MatchEventOptions::default()){let _=ws.send(Message::Text(json!(["EVENT",id,event]).to_string().into())).await;}}}}
                    }
                }
            });
        }
    });
    Ok(Relay { url, task })
}
pub fn config(url: &str) -> RelayConfig {
    RelayConfig {
        url: url.into(),
        enabled: true,
        read: true,
        write: true,
        private: false,
    }
}
