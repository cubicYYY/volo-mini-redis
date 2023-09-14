use clap::{Parser, Subcommand};
use colored::Colorize;
use lazy_static::lazy_static;
use mini_redis::cmdargs::{self, ProxyConfig};
use mini_redis::{AsciiFilterLayer, TimedLayer};
use pilota::FastStr;
use rustyline::{error::ReadlineError, DefaultEditor};
use shell_words::split;
use tracing::info;
use std::collections::hash_map::DefaultHasher;
use std::{net::SocketAddr, thread, time::Duration};
use volo_gen::volo::redis::{GetItemResponse, RedisCommand};
use std::hash::{Hash, Hasher};

#[derive(Debug, Parser)]
#[command(author, version, about, long_about = None)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand, Debug)]
enum Commands {
    /// ping the server
    Ping { args: Option<Vec<String>> },
    /// set a key-value pair
    Set {
        /// key
        key: String,
        /// value
        value: String,
        // type
        ttype: Option<String>,
        // time
        tnum: Option<u128>,
        // transaction_id
        #[clap(short, long)]
        transaction_id: Option<String>,
    },
    /// get a value by key, nil if key not exist
    Get {
        /// key
        key: String,
        /// transaction_id
        #[clap(short, long)]
        transaction_id: Option<String>,
    },
    /// delete a key-value pair, error if key not exist
    Del {
        /// key
        key: String,
    },
    /// quit the client
    Exit,
    /// publish a message to a channel
    Publish {
        /// channel
        channel: String,
        /// message
        message: String,
    },
    /// subscribe a channel
    Subscribe {
        /// channel
        channel: String,
    },
    /// watch a transaction
    Watch {
        /// key to watch
        key: String,
    },
    /// start a transaction
    Multi,
    /// execute a transaction
    Exec,
}

lazy_static! {
    static ref CMD_ARGS: ProxyConfig = ProxyConfig::parse();
    static ref TO_PROXY_SELF: volo_gen::volo::redis::ItemServiceClient = {
        let addr: SocketAddr = CMD_ARGS
            .attach_to
            .clone()
            .unwrap_or("127.0.0.1:8080".into()) //TODO: parse from config
            .parse()
            .unwrap();
        volo_gen::volo::redis::ItemServiceClientBuilder::new("volo-redis")
            .layer_outer(TimedLayer)
            .layer_outer(AsciiFilterLayer)
            .address(addr)
            .build()
    };
}
use mini_redis::get_client;

async fn subscribe(handle: String) -> ! {
    loop {
        let resp = TO_PROXY_SELF
            .get_item(volo_gen::volo::redis::GetItemRequest {
                cmd: RedisCommand::Fetch,
                args: Some(vec![handle.clone().into()]),
                client_id: None,
                transaction_id: None,
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
const SLOTS: usize = 16384;
struct CountingBloom {
    
}

#[volo::main]
async fn main() {
    tracing_subscriber::fmt::init();
    // ==================Allocate slots to all cluster members
    let mut masters: Vec<SocketAddr> = Vec::with_capacity(SLOTS); // TODO: given by .toml config

    //--testonly!
    masters.push(SocketAddr::new(
        "127.0.0.1".to_string().parse().unwrap(),
        8888,
    ));
     //TODO: parse from config
    //--testonly!

    let mut slot_belong: Vec<usize> = Vec::with_capacity(SLOTS); // node (that this slot belongs to) id in `masters`
    
    if masters.len() == 0 {
        panic!("Empty cluster!");
    }

    let each_node = SLOTS / masters.len();
    for i in 0..SLOTS {
        slot_belong.push(i / each_node);
    }

    let hashed_client = |key: &str| -> volo_gen::volo::redis::ItemServiceClient {
        let mut hasher = DefaultHasher::new();
        key.hash(&mut hasher);
        let hash_result = hasher.finish();
        let id = slot_belong[(hash_result % SLOTS as u64) as usize];
        info!("proxyed to {}.", masters[id].to_string());
        get_client(masters[id])
    };
    
    // ==================

    let mut state: String = "cluster:proxy".into();
    let mut cmdline = DefaultEditor::new().expect("==command line failure==");
    let mut local_transaction_id: Option<String> = None;
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
        let trimmed = split(line.trim());
        let real_args = match trimmed {
            Ok(inside) => inside,
            Err(e) => {
                println!("Error: {:?}", e);
                continue;
            }
        };
        let vecref: Vec<&str> = real_args.iter().map(|s| s as &str).collect();
        let args_chain = std::iter::once("").chain(vecref);

        let cli = Cli::try_parse_from(args_chain);
        if let Err(e) = cli {
            let _ = clap::error::Error::print(&e);
            continue;
        }
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
        let cli = cli.unwrap();
        match cli.command {
            Commands::Ping { args } => {
                let resp = TO_PROXY_SELF
                    .get_item(volo_gen::volo::redis::GetItemRequest {
                        cmd: RedisCommand::Ping,
                        args: args.map(|vstr| vstr.into_iter().map(|s| FastStr::new(s)).collect()),
                        client_id: None,
                        transaction_id: None,
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
            Commands::Set {
                // ! PROXYED
                key,
                value,
                ttype,
                tnum,
                transaction_id,
            } => {
                let mut args = vec![key.into(), value.into()];
                if ttype.is_some() {
                    args.push(ttype.unwrap());
                }
                if tnum.is_some() {
                    args.push(tnum.unwrap().to_string());
                }
                let resp = hashed_client(&args[0])
                    .get_item(volo_gen::volo::redis::GetItemRequest {
                        cmd: RedisCommand::Set,
                        args: Some(args.iter().map(|s| FastStr::new(s)).collect()),
                        client_id: None,
                        transaction_id: transaction_id.clone().map(|s| FastStr::new(s)),
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
            Commands::Get {
                // ! PROXYED
                key,
                transaction_id,
            } => {
                let resp = hashed_client(key.clone().as_ref())
                    .get_item(volo_gen::volo::redis::GetItemRequest {
                        cmd: RedisCommand::Get,
                        args: Some(vec![key.into()]),
                        client_id: None,
                        transaction_id: transaction_id.map(|s| s.into()),
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
            Commands::Del { key } => {
                // ! PROXYED
                let resp = hashed_client(key.clone().as_ref())
                    .get_item(volo_gen::volo::redis::GetItemRequest {
                        cmd: RedisCommand::Del,
                        args: Some(vec![key.into()]),
                        client_id: None,
                        transaction_id: None,
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
            Commands::Exit => {
                println!("Bye~");
                return;
            }
            Commands::Publish { channel, message } => {
                let resp = TO_PROXY_SELF
                    .get_item(volo_gen::volo::redis::GetItemRequest {
                        cmd: RedisCommand::Publish,
                        args: Some(vec![channel.into(), message.into()]),
                        client_id: None,
                        transaction_id: None,
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
            Commands::Subscribe { channel } => {
                // handle this carefully
                let resp = TO_PROXY_SELF
                    .get_item(volo_gen::volo::redis::GetItemRequest {
                        cmd: RedisCommand::Subscribe,
                        args: Some(vec![channel.clone().into()]),
                        client_id: None,
                        transaction_id: None,
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
                            state = channel;
                            subscribe(info.data.unwrap().into()).await;
                            continue;
                        }
                    }
                    Err(e) => tracing::error!("{:?}", e),
                }
                continue;
            }
            Commands::Watch { key } => {
                if local_transaction_id.clone().is_none() {
                    return tracing::error!("{:?}", "transaction is not started");
                }
                let resp = TO_PROXY_SELF
                    .get_item(volo_gen::volo::redis::GetItemRequest {
                        cmd: RedisCommand::Watch,
                        args: Some(vec![key.into()]),
                        client_id: None,
                        transaction_id: local_transaction_id.clone().map(|s| s.into()),
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
            Commands::Multi => {
                if local_transaction_id.is_some() {
                    return tracing::error!("{:?}", "transaction is already started");
                }
                let resp = TO_PROXY_SELF
                    .get_item(volo_gen::volo::redis::GetItemRequest {
                        cmd: RedisCommand::Multi,
                        args: None,
                        client_id: None,
                        transaction_id: None,
                    })
                    .await;
                match resp {
                    Ok(info) => {
                        local_transaction_id = Some(info.clone().data.unwrap().into());
                        colored_out(info);
                        state = "transaction".into();
                    }
                    Err(e) => tracing::error!("{:?}", e),
                }
                continue;
            }
            Commands::Exec => {
                if local_transaction_id.is_none() {
                    return tracing::error!("{:?}", "transaction is not started");
                }
                let resp = TO_PROXY_SELF
                    .exec(volo_gen::volo::redis::GetItemRequest {
                        cmd: RedisCommand::Exec,
                        args: None,
                        client_id: None,
                        transaction_id: local_transaction_id.clone().map(|s| s.into()),
                    })
                    .await;
                match resp {
                    Ok(info) => {
                        for item in info.data.unwrap() {
                            colored_out(item);
                        }
                        local_transaction_id = None;
                        state = "connected".into();
                    }
                    Err(e) => tracing::error!("{:?}", e),
                }
                continue;
            }
        };
    }
}
