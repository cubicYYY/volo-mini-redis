use lazy_static::lazy_static;
use volo_gen::volo::redis::RedisCommand;
use std::net::SocketAddr;

lazy_static! {
    static ref CLIENT: volo_gen::volo::redis::ItemServiceClient = {
        let addr: SocketAddr = "127.0.0.1:8080".parse().unwrap();
        volo_gen::volo::redis::ItemServiceClientBuilder::new("volo-redis")
            .address(addr)
            .build()
    };
}

#[volo::main]
async fn main() {
    tracing_subscriber::fmt::init();
    let req = volo_gen::volo::redis::GetItemRequest { cmd: RedisCommand::Ping, args: None};
    let resp = CLIENT.get_item(req).await;
    match resp {
        Ok(info) => tracing::info!("{:?}", info),
        Err(e) => tracing::error!("{:?}", e),
    }
}
