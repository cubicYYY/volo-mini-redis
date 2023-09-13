#![feature(impl_trait_in_assoc_type)]
use mini_redis::S;
use mini_redis::{AsciiFilterLayer, TimedLayer};
use std::fs::File;
use std::io::{BufRead, BufReader};
use std::net::SocketAddr;

#[volo::main]
async fn main() {
    let addr: SocketAddr = "[::]:8080".parse().unwrap();
    let addr = volo::net::Address::from(addr);
 
    let s = S::new();
    //let redis = redis.write().unwrap();
    let file = File::open("/Users/a1234/Desktop/Mini_Redis/src/AOF_FILE").unwrap();
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
        //let milliseconds = parts[4];
        match command {
            "SET" => {
                let mut s_clone = s.redis.write().await;
                s_clone.set(id, title, miliseconds);
                //println!("aaa");
            }
            "DEL" => {
                let mut s_clone = s.redis.write().await;
                s_clone.del(id);
                //println!("bbb");
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
