use colored::Colorize;
use lazy_static::lazy_static;
use mini_redis::{AsciiFilterLayer, TimedLayer};
use pilota::FastStr;
use std::{
    env,
    io::{self, Write},
    net::SocketAddr,
};
use volo_gen::volo::redis::{GetItemResponse, RedisCommand};

const REMOTE_ADDR: &str = "127.0.0.1:8080"; // TODO: specify in cmd args

lazy_static! {
    static ref CLIENT: volo_gen::volo::redis::ItemServiceClient = {
        let addr: SocketAddr = REMOTE_ADDR.parse().unwrap();
        volo_gen::volo::redis::ItemServiceClientBuilder::new("volo-redis")
            .layer_outer(TimedLayer)
            .layer_outer(AsciiFilterLayer)
            .address(addr)
            .build()
    };
}

#[volo::main]
async fn main() {
    tracing_subscriber::fmt::init();
    // let cmd_args: Vec<String> = env::args().collect();

    let mut state = "connected";
    let mut is_subscribe: bool = false;
    let mut channel: String = String::new();
    let mut buf: String = String::with_capacity(4096);

    loop {
        if is_subscribe {
            unimplemented!();
            continue;
        }
        print!("vodis[{state}]>  ");
        let _ = io::stdout().flush();

        buf.clear();
        std::io::stdin().read_line(&mut buf).unwrap();
        let mut arg_iter = buf.trim().split_whitespace().map(|s| s.to_string());

        let command = arg_iter.next();
        if command.is_none() {
            continue;
        }
        let command = command.unwrap();
        let args: Vec<FastStr> = arg_iter.map(|s| FastStr::from(s)).collect();

        let colored_out = move |resp: GetItemResponse| {
            println!(
                "{}{}",
                if resp.ok {
                    "[OK] ".green()
                } else {
                    "[FAILED] ".red()
                },
                if let Some(str_) = resp.data {
                    str_.to_string()
                } else {
                    "(nil)".into()
                }
            )
        };
        match command.as_ref() {
            "ping" => {
                let resp = CLIENT
                    .get_item(volo_gen::volo::redis::GetItemRequest {
                        cmd: RedisCommand::Ping,
                        args: Some(args),
                    })
                    .await;
                match resp {
                    Ok(info) => {
                        colored_out(info);
                    }
                    Err(e) => tracing::error!("{:?}", e),
                }
                continue;
            }
            "get" => {
                let resp = CLIENT
                    .get_item(volo_gen::volo::redis::GetItemRequest {
                        cmd: RedisCommand::Get,
                        args: Some(args),
                    })
                    .await;
                match resp {
                    Ok(info) => {
                        colored_out(info);
                    }
                    Err(e) => tracing::error!("{:?}", e),
                }
                continue;
            }
            "set" => {
                let resp = CLIENT
                    .get_item(volo_gen::volo::redis::GetItemRequest {
                        cmd: RedisCommand::Set,
                        args: Some(args),
                    })
                    .await;
                match resp {
                    Ok(info) => {
                        colored_out(info);
                    }
                    Err(e) => tracing::error!("{:?}", e),
                }
                continue;
            }
            "del" => {
                let resp = CLIENT
                    .get_item(volo_gen::volo::redis::GetItemRequest {
                        cmd: RedisCommand::Del,
                        args: Some(args),
                    })
                    .await;
                match resp {
                    Ok(info) => {
                        colored_out(info);
                        println!("(deleted count)");
                    }
                    Err(e) => tracing::error!("{:?}", e),
                }
                continue;
            }
            "exit " => {
                println!("Bye~");
                return;
            }
            _ => {
                println!("Command {command} is not supported!");
            }
        }
    }
}
