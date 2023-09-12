use colored::Colorize;
use lazy_static::lazy_static;
use mini_redis::{AsciiFilterLayer, TimedLayer};
use pilota::FastStr;
use rustyline::{error::ReadlineError, Editor};
use std::{net::SocketAddr, thread, time::Duration};
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

    let mut state: String = "connected".into();
    let mut is_subscribed: bool = false;
    let mut channel_hd: String = String::new();
    let mut cmdline = Editor::<()>::new();

    loop {
        if is_subscribed {
            // polling: 1 res/sec
            let resp = CLIENT
                .get_item(volo_gen::volo::redis::GetItemRequest {
                    cmd: RedisCommand::Fetch,
                    args: Some(vec![channel_hd.clone().into()]),
                })
                .await;
            match resp {
                Ok(info) => {
                    if !info.ok {
                        thread::sleep(Duration::from_secs(1));
                        continue;
                    }
                    println!("[GOT] {}", info.data.clone().unwrap());
                }
                Err(e) => tracing::error!("{:?}", e),
            }
            continue;
        }
        let line = match cmdline.readline(format!("vodis[{}]>  ", state.clone()).as_ref()) {
            Ok(line) => line,
            Err(ReadlineError::Interrupted) | Err(ReadlineError::Eof) => {
                println!("Bye~~~");
                return;
            }
            Err(err) => {
                println!("Error: {:?}", err);
                return;
            }
        };
        let mut arg_iter = line.trim().split_whitespace().map(|s| s.to_string());

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
            "publish" => {
                let resp = CLIENT
                    .get_item(volo_gen::volo::redis::GetItemRequest {
                        cmd: RedisCommand::Publish,
                        args: Some(args),
                    })
                    .await;
                match resp {
                    Ok(info) => {
                        colored_out(info);
                        println!("(received client count)");
                    }
                    Err(e) => tracing::error!("{:?}", e),
                }
                continue;
            }
            "subscribe" => {
                // handle this carefully
                let resp = CLIENT
                    .get_item(volo_gen::volo::redis::GetItemRequest {
                        cmd: RedisCommand::Subscribe,
                        args: Some(args.clone()),
                    })
                    .await;
                match resp {
                    Ok(info) => {
                        if !info.ok {
                            colored_out(info);
                        } else {
                            // NOT STABLE:
                            // info.data.inspect(|data|{println!("LISTENING: {}", data)});
                            println!("Listening handle: {}", info.data.clone().unwrap());
                            is_subscribed = true;
                            channel_hd = info.data.unwrap().into();
                            state = args.join("").into();
                            continue;
                        }
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
