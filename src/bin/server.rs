#![feature(impl_trait_in_assoc_type)]
use mini_redis::S;
use mini_redis::{AsciiFilterLayer, TimedLayer};
use std::fs::File;
use std::io::{BufRead, BufReader};
use std::net::SocketAddr;
use std::time::{SystemTime, UNIX_EPOCH};
use tokio::sync::mpsc;
#[volo::main]
async fn main() {
    let addr: SocketAddr = "127.0.0.1:8080".parse().unwrap();
    let addr = volo::net::Address::from(addr);

    let s = S::new();
    let file = File::open("./src/AOF_FILE").unwrap();
    let reader = BufReader::new(file);

    for line in reader.lines() {
        let line = line.unwrap();
        let parts: Vec<&str> = line.trim().splitn(4, ' ').collect();

        if parts.len() != 4 {
            eprintln!("Invalid command: {}", line);
            continue;
        }

        let command = parts[0];
        let id = parts[1];
        let title = parts[2];
        let miliseconds = parts[3].parse::<u128>().unwrap();
        match command {
            "SET" => {
                let mut s_clone = s.redis.write().await;
                if miliseconds > now() || miliseconds == 0{
                    s_clone.set_after(id, title, miliseconds);
                }else{
                    s_clone.del(id);
                }
                
            }
            "DEL" => {
                let mut s_clone = s.redis.write().await;
                s_clone.del(id);
            }
            _ => {
                eprintln!("Unknown command: {}", command);
            }
        }
    }

    tracing_subscriber::fmt::init();
    volo_gen::volo::redis::ItemServiceServer::new(s)
        .layer_front(TimedLayer)
        .layer_front(AsciiFilterLayer)
        .run(addr)
        .await
        .unwrap();
}

fn now() -> u128 {
    let current_timestamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("Failed to get current timestamp");

    current_timestamp.as_millis()
}
