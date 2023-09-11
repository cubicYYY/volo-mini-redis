use std::{
    collections::HashMap,
    time::{SystemTime, UNIX_EPOCH},
};

pub type Timestamp = u128;

#[derive(Debug, Clone)]
struct TimedValue {
    pub value: String,

    /// None = never expire, otherwise a timestamp
    pub expired_at: Option<Timestamp>,
}

pub struct Redis {
    /// Key-Value
    kvs: HashMap<String, TimedValue>,
}
impl Redis {
    pub fn new() -> Self {
        Self {
            kvs: HashMap::new(),
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
            ts > Self::now()
        } else {
            false
        }
    }

    pub fn get(&mut self, key: &str) -> Option<String> {
        if let Some(tv) = self.kvs.get(key) {
            if Self::expired(tv.expired_at) {
                self.kvs.remove(key);
                None
            } else {
                Some(tv.value.clone())
            }
        } else {
            None
        }
    }

    // `exp_after`: milliseconds, 0 means never
    pub fn set(&mut self, key: &str, value: &str, exp_after: u128) -> bool {
        if self.kvs.contains_key(key) {
            self.kvs.insert(
                key.to_string(),
                TimedValue {
                    value: value.to_string(),
                    expired_at: if exp_after == 0 {
                        None
                    } else {
                        Some(Self::now() + exp_after)
                    },
                },
            );
            true
        } else {
            false
        }
    }

    pub fn del(&mut self, key: &str) -> bool {
        if let Some(_) = self.kvs.remove(key) {
            true
        } else {
            false
        }
    }
}
