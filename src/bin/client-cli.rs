use colored::Colorize;
use lazy_static::lazy_static;
use mini_redis::{AsciiFilterLayer, TimedLayer};
use pilota::FastStr;
use rustyline::{error::ReadlineError, Editor};
use shell_words::split;
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
async fn subscribe(handle: String) -> ! {
    loop {
        let resp = CLIENT
            .get_item(volo_gen::volo::redis::GetItemRequest {
                cmd: RedisCommand::Fetch,
                args: Some(vec![handle.clone().into()]),
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
    }
}

#[volo::main]
async fn main() {
    tracing_subscriber::fmt::init();
    // let cmd_args: Vec<String> = env::args().collect();

    let mut state: String = "connected".into();
    let mut cmdline = Editor::<()>::new();

    loop {
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
        let mut arg_iter;
        match split(line.trim()) {
            Ok(item) => arg_iter = item.into_iter(),
            Err(e) => {
                println!("Error: {:?}", e);
                continue;
            }
        }
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
                            state = args.join("").into();
                            subscribe(info.data.unwrap().into()).await;
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
