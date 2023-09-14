use rmp_serde::{Deserializer, Serializer};
use serde::{Deserialize, Serialize};
use std::sync::mpsc::{channel, Receiver, RecvTimeoutError, Sender};
use std::time::Duration;
use std::{
    collections::HashMap,
    time::{SystemTime, UNIX_EPOCH},
};

pub type Timestamp = u128;
pub type RcvHandle = usize;

const CLUSTER_MAX_SIZE: u16 = 16384;

#[derive(Debug, Clone, Serialize, Deserialize)]
struct TimedValue {
    pub value: String,

    /// None = never expire, otherwise a timestamp
    pub expired_at: Option<Timestamp>,
}

#[derive(Debug, Serialize, Deserialize)]
struct StoredKV {
    pub data: HashMap<String, TimedValue>,
}

pub struct Redis {
    /// Key-Value
    kvs: StoredKV,

    /// Channel name-Senders
    channels: HashMap<String, Vec<Sender<String>>>,
    rcv: HashMap<RcvHandle, Receiver<String>>,
}

/// SAFETY: Sender&Receiver is dispatched to only a single client
/// ! WARNING: unauthorized client can perform illegal access by providing fake handles, which may lead to a racing
unsafe impl Send for Redis {}
unsafe impl Sync for Redis {}

impl Redis {
    pub fn new() -> Self {
        Self {
            kvs: StoredKV {
                data: HashMap::new(),
            },
            channels: HashMap::new(),
            rcv: HashMap::new(),
        }
    }
    fn now() -> Timestamp {
        let current_timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("Failed to get current timestamp");

        current_timestamp.as_millis()
    }

    fn expired(ts: Option<Timestamp>) -> bool {
        if let Some(ts) = ts {
            Self::now() > ts
        } else {
            false
        }
    }

    pub fn get(&mut self, key: &str) -> Option<String> {
        if let Some(tv) = self.kvs.data.get(key) {
            if Self::expired(tv.expired_at) {
                self.kvs.data.remove(key);
                None
            } else {
                Some(tv.value.clone())
            }
        } else {
            None
        }
    }

    // TODO: Option with From trait to avoid special judge for exp=0
    /// `exp_after`: milliseconds, 0 means never
    pub fn set_after(&mut self, key: &str, value: &str, exp_after: u128) {
        self.set_at(
            key,
            value,
            if exp_after == 0 {
                0
            } else {
                Self::now() + exp_after
            },
        );
    }

    /// `exp_at`: milliseconds, 0 means never
    pub fn set_at(&mut self, key: &str, value: &str, exp_at: u128) {
        self.kvs.data.insert(
            key.to_string(),
            TimedValue {
                value: value.to_string(),
                expired_at: if exp_at == 0 { None } else { Some(exp_at) },
            },
        );
    }

    pub fn del(&mut self, key: &str) -> bool {
        if let Some(_) = self.kvs.data.remove(key) {
            true
        } else {
            false
        }
    }

    pub fn add_subscriber(&mut self, channel_name: &str) -> RcvHandle {
        let (tx, rx): (Sender<String>, Receiver<String>) = channel();
        self.channels
            .entry(channel_name.into())
            .or_insert(vec![])
            .push(tx);
        let hd: RcvHandle = self.rcv.len();
        self.rcv.insert(hd, rx);
        hd
    }
    pub fn fetch(&self, hd: RcvHandle) -> Result<String, RecvTimeoutError> {
        self.rcv
            .get(&hd)
            .unwrap()
            .recv_timeout(Duration::from_millis(0))
    }

    /// return: numbers
    pub fn broadcast(&mut self, channel_name: &str, content: &str) -> usize {
        let mut cnt = 0;
        self.channels.entry(channel_name.into()).and_modify(|sds| {
            for sender in &mut *sds {
                match sender.send(content.into()) {
                    Ok(_) => {}
                    Err(_) => {
                        // subscriber died / disconnected
                        // TODO...
                    }
                }
            }
            cnt = sds.len();
        });
        cnt
    }

    /// Serialize the data stored
    /// TODO: asynchronously
    pub fn serialize(&self) -> Vec<u8> {
        let mut buf = Vec::new();
        self.kvs.serialize(&mut Serializer::new(&mut buf)).unwrap();
        buf
    }

    /// De-serialize the data, WITH CURRENT DATA CLEARED
    /// TODO: asynchronously
    pub fn deserialize(&mut self, data: Vec<u8>) {
        self.kvs = rmp_serde::from_slice(&data).unwrap();
    }

    /// New node added to current cluster
    pub fn new_node(&mut self) {}
}
