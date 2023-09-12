#![feature(impl_trait_in_assoc_type)]
use std::net::SocketAddr;

use mini_redis::S;
use mini_redis::{AsciiFilterLayer, TimedLayer};

#[volo::main]
async fn main() {
    let addr: SocketAddr = "[::]:8080".parse().unwrap();
    let addr = volo::net::Address::from(addr);

    tracing_subscriber::fmt::init();
    volo_gen::volo::redis::ItemServiceServer::new(S::new())
        .layer_front(TimedLayer)
        .layer_front(AsciiFilterLayer)
        .run(addr)
        .await
        .unwrap();
}
