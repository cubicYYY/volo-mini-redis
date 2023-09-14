#![feature(impl_trait_in_assoc_type)]

mod redis;

use anyhow::{anyhow, Ok};
use clap::{self, Parser};
use lazy_static::lazy_static;
use nanoid::nanoid;
use rand::distributions::Alphanumeric;
use rand::Rng;
use redis::Timestamp;
use std::collections::HashMap;
use std::f32::consts::E;
use std::sync::Arc;
use std::{
    fs::{File, OpenOptions},
    io::{BufRead, BufReader, Seek, SeekFrom, Write},
    net::SocketAddr,
    thread,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};
use tokio::sync::mpsc;
use tokio::{
    signal,
    sync::{Mutex, RwLock},
};
use tracing::info;
use uuid::Uuid;
use volo_gen::volo::redis::{GetItemRequest, GetItemResponse, MultiGetItemResponse, RedisCommand};

pub type Host = String;
pub type Port = u32;

enum RedisState {
    Single,
    Master,              // m-s mode / cluster mode
    SlaveOf(Host, Port), // m-s mode / cluster mode
}

impl std::fmt::Display for RedisState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            RedisState::Single => write!(f, "Single"),
            RedisState::Master => write!(f, "Master"),
            RedisState::SlaveOf(h, p) => write!(f, "Slave of {h}:{p}"),
        }
    }
}

#[derive(Parser)]
#[command(author, version, about, long_about = None)]
struct ServerConfig {
    /// Optional AOF file path
    #[arg(short, long, value_name = "FILE")]
    aof: Option<String>,

    /// Mark this Vodis instance as a slave
    #[arg(short, long, value_name = "IP:PORT")]
    slaveof: Option<String>,

    /// Sets a custom config file
    #[arg(short, long, action = clap::ArgAction::Count)]
    cluster: u8,

    /// Execute provided commands after initialization
    #[arg(short, long)]
    pre_run: Option<Vec<String>>,
}

lazy_static! {
    // Command line args
    static ref CMD_ARGS: ServerConfig = ServerConfig::parse();
    static ref MASTER_ADDR: String = String::from((CMD_ARGS.slaveof).clone().expect("No master ADDR specified."));
    static ref IS_CLUSTER: bool = CMD_ARGS.cluster > 0;
    static ref PRE_RUN: Option<Vec<String>> = CMD_ARGS.pre_run.clone();

    static ref CTRL_C: Arc<Mutex<bool>> = Arc::new(Mutex::new(false));
    static ref CLIENT: volo_gen::volo::redis::ItemServiceClient = {
        let addr: SocketAddr = MASTER_ADDR.parse().unwrap();
        volo_gen::volo::redis::ItemServiceClientBuilder::new("volo-redis")
            .layer_outer(TimedLayer)
            .layer_outer(AsciiFilterLayer)
            .address(addr)
            .build()
    };
    //transaction id match the commands in the transaction
    pub static ref TRANSACTION_HASHMAP: Arc<RwLock<HashMap<String, Transaction>>> = Arc::new(RwLock::new(HashMap::new()));
    //key match the transaction pair with the value of the key
    pub static ref TRANSACTION_WATCHER: Arc<RwLock<HashMap<String, HashMap<String, Option<String>>>>> = Arc::new(RwLock::new(HashMap::new()));
    //keys a transaction is watching
    pub static ref KEY_WATCHED: Arc<RwLock<HashMap<String, Vec<String>>>> = Arc::new(RwLock::new(HashMap::new()));
}

type ArwLock<T> = Arc<Mutex<T>>;
pub struct S {
    pub redis: ArwLock<redis::Redis>,
    sender: mpsc::Sender<String>,
    state: ArwLock<RedisState>,
    uuid: Uuid,
}

pub struct Transaction {
    pub commands: Vec<GetItemRequest>,
    pub is_wrong: bool,
}

impl S {
    pub fn new() -> S {
        let (sender, mut receiver) = mpsc::channel(1024);
        let id = Uuid::new_v4();
        let dupid = id.clone();
        // Spawn a thread to handle received messages
        thread::spawn(move || {
            let mut command: Vec<String> = Vec::new();
            let mut aof_file = OpenOptions::new()
                .append(true)
                .create(true)
                .open(format!("{}.aof", dupid))
                .expect("Failed to open AOF file");
            let mut last_write_time = Instant::now();
            loop {
                // Flush contents inside command buffer
                let mut flush = |command: &Vec<String>| {
                    if command.is_empty() {
                        return;
                    }
                    // 将缓存中的操作写入 AOF 文件
                    command.iter().for_each(|cmd| {
                        write!(aof_file, "{}", cmd).expect("Failed to write to AOF file");
                    });
                    aof_file.flush().expect("Failed to flush file");
                    println!("COMMAND SAVED!!");
                };
                match receiver.try_recv() {
                    std::result::Result::Ok(msg) => {
                        command.push(msg);
                        // 检查是否距上次写入已经过去了 1 秒
                        if last_write_time.elapsed() >= Duration::from_secs(1) {
                            flush(&command);
                            command.clear();
                            last_write_time = Instant::now();
                        }
                    }
                    Err(_) => {
                        flush(&command);
                        command.clear();
                        last_write_time = Instant::now();
                    }
                }
            }
        });
        S {
            redis: Arc::new(Mutex::new(redis::Redis::new())),
            sender,
            state: Arc::new(Mutex::new(RedisState::Single)), // TODO: can be changed
            uuid: id,                                        // TODO: can recover from old id
        }
    }
    async fn send_message(&self, msg: String) {
        let _ = self.sender.send(msg).await;
    }
}

#[derive(Clone)]
pub struct AsciiFilter<S>(S);

#[volo::service]
impl<Cx, Req, S> volo::Service<Cx, Req> for AsciiFilter<S>
where
    Req: std::fmt::Debug + Send + 'static,
    S: Send + 'static + volo::Service<Cx, Req> + Sync,
    S::Response: std::fmt::Debug,
    S::Error: std::fmt::Debug,
    anyhow::Error: Into<S::Error>,
    Cx: Send + 'static,
{
    async fn call(&self, cx: &mut Cx, req: Req) -> Result<S::Response, S::Error> {
        let resp = self.0.call(cx, req).await;
        if resp.is_err() {
            return resp;
        }
        if format!("{:?}", resp)
            .chars()
            .any(|c| ((c as u32) < 32 || (c as u32) > 127))
        {
            Err(
                anyhow!("Invalid chars found. Only printable ASCII chars are allowed in requests.")
                    .into(),
            )
        } else {
            resp
        }
    }
}

/// Block non-ASCII characters are
pub struct AsciiFilterLayer;

impl<S> volo::Layer<S> for AsciiFilterLayer {
    type Service = AsciiFilter<S>;

    fn layer(self, inner: S) -> Self::Service {
        AsciiFilter(inner)
    }
}

#[derive(Clone)]
pub struct Timed<S>(S);

#[volo::service]
impl<Cx, Req, S> volo::Service<Cx, Req> for Timed<S>
where
    Req: std::fmt::Debug + Send + 'static,
    S: Send + 'static + volo::Service<Cx, Req> + Sync,
    S::Response: std::fmt::Debug,
    S::Error: std::fmt::Debug,
    Cx: Send + 'static,
{
    async fn call(&self, cx: &mut Cx, req: Req) -> Result<S::Response, S::Error> {
        let now = std::time::Instant::now();
        tracing::debug!("Received request {:?}", &req);
        let resp = self.0.call(cx, req).await;
        tracing::debug!("Sent response {:?}", &resp);
        tracing::debug!("Request took {}ms", now.elapsed().as_millis());
        resp
    }
}

fn now() -> Timestamp {
    let current_timestamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("Failed to get current timestamp");

    current_timestamp.as_millis()
}

pub struct TimedLayer;

impl<S> volo::Layer<S> for TimedLayer {
    type Service = Timed<S>;

    fn layer(self, inner: S) -> Self::Service {
        Timed(inner)
    }
}

#[volo::async_trait]
impl volo_gen::volo::redis::ItemService for S {
    async fn get_item(
        &self,
        _req: volo_gen::volo::redis::GetItemRequest,
    ) -> ::core::result::Result<volo_gen::volo::redis::GetItemResponse, ::volo_thrift::AnyhowError>
    {
        {
            let ctrl_c = CTRL_C.lock().await;
            if *ctrl_c {
                return Err(anyhow!("Server is shutting").into());
            }
        }
        let shared_data = Arc::new(Mutex::new(false));
        let shared_data_clone = Arc::clone(&shared_data);
        let child = volo::spawn(async move {
            let _ = signal::ctrl_c().await;
            info!("Ctrl-C received, shutting down");
            *shared_data_clone.lock().await = true;
        });
        use volo_gen::volo::redis::GetItemResponse;
        let response = match _req.cmd {
            RedisCommand::Ping => {
                tokio::time::sleep(Duration::from_secs(10)).await;
                if let Some(arg) = _req.args {
                    let ans = if arg.len() == 0 {
                        "pong".into()
                    } else {
                        arg.join(" ").into()
                    };
                    Ok(GetItemResponse {
                        ok: true,
                        data: Some(ans),
                    })
                } else {
                    Ok(GetItemResponse {
                        ok: true,
                        data: Some("pong".into()),
                    })
                }
            }
            RedisCommand::Get => {
                let get_transaction_id = _req.transaction_id.clone();
                if let Some(t_id) = get_transaction_id {
                    let mut transactions = TRANSACTION_HASHMAP.write().await;
                    //check if there is an transaction with the given id
                    if transactions.contains_key(&t_id.to_string()) {
                        let mut transaction = transactions.get_mut(&t_id.to_string()).unwrap();
                        info!("{:?}", &_req);
                        transaction.commands.push(_req.clone());
                        return Ok(GetItemResponse {
                            ok: true,
                            data: Some("OK".into()),
                        });
                    } else {
                        return Err(anyhow!("Transaction not found"));
                    }
                }
                if let Some(arg) = &_req.args {
                    if arg.len() != 1 {
                        Err(anyhow!(
                            "Invalid arguments count: {} (expected 1)",
                            arg.len()
                        ))
                    } else {
                        if let Some(value) = self.redis.lock().await.get(arg[0].as_ref()) {
                            Ok(GetItemResponse {
                                ok: true,
                                data: Some(value.into()),
                            })
                        } else {
                            Ok(GetItemResponse {
                                ok: false,
                                data: None,
                            })
                        }
                    }
                } else {
                    Err(anyhow!("No arguments given (required)"))
                }
            }
            RedisCommand::Set => {
                let set_transaction_id = _req.transaction_id.clone();
                if let Some(t_id) = set_transaction_id {
                    let mut transactions = TRANSACTION_HASHMAP.write().await;
                    //check if there is an transaction with the given id
                    if transactions.contains_key(&t_id.to_string()) {
                        let mut transaction = transactions.get_mut(&t_id.to_string()).unwrap();
                        transaction.commands.push(_req.clone());
                        return Ok(GetItemResponse {
                            ok: true,
                            data: Some("OK".into()),
                        });
                    } else {
                        return Err(anyhow!("Transaction not found"));
                    }
                }
                if let Some(arg) = _req.args {
                    if arg.len() < 2 {
                        Err(anyhow!(
                            "Invalid arguments count: {} (expected >=2)",
                            arg.len()
                        ))
                    } else {
                        let (key, value) = (&arg[0], &arg[1]);
                        let mut milliseconds = 0;
                        if let Some(exp_type) = arg.get(2) {
                            if let Some(exp_num) = arg.get(3) {
                                let exp_after = exp_num.parse::<u128>()?;

                                match exp_type.to_lowercase().as_ref() {
                                    "ex" => {
                                        milliseconds = exp_after * 1000;
                                    }
                                    "px" => {
                                        milliseconds = exp_after;
                                    }
                                    _ => {
                                        return Err(anyhow!("Unsupported time type `{exp_type}`"));
                                    }
                                }
                            } else {
                                return Err(anyhow!("Duration number not provided."));
                            }
                        }
                        let command_str = format!(
                            "SET {} {} {}\n",
                            key,
                            value,
                            if milliseconds != 0 {
                                now() + milliseconds
                            } else {
                                0u128
                            }
                        );
                        self.send_message(command_str).await;
                        self.redis.lock().await.set_after(
                            key.as_ref(),
                            value.as_ref(),
                            milliseconds,
                        );
                        Ok(GetItemResponse {
                            ok: true,
                            data: Some("OK".into()),
                        })
                    }
                } else {
                    Err(anyhow!("No arguments given (required)"))
                }
            }
            RedisCommand::Del => {
                if _req.args.is_none() {
                    return Err(anyhow!("No arguments given (required)"));
                }
                let arg = _req.args.unwrap();
                if arg.len() < 1 {
                    return Err(anyhow!(
                        "Invalid arguments count: {} (expected >= 1)",
                        arg.len()
                    ));
                }
                let mut success: u16 = 0;
                for key in arg {
                    success += self.redis.lock().await.del(key.as_ref()) as u16;
                    let command_str = format!("DEL {:} 0 0\n", key);
                    self.send_message(command_str).await;
                }
                Ok(GetItemResponse {
                    ok: true,
                    data: Some(success.to_string().into()),
                })
            }
            RedisCommand::Publish => {
                if _req.args.is_none() {
                    return Err(anyhow!("No arguments given (required)"));
                }
                let arg = _req.args.unwrap();
                if arg.len() != 2 {
                    return Err(anyhow!(
                        "Invalid arguments count: {} (expected =2)",
                        arg.len()
                    ));
                }
                let (chan, s) = (&arg[0], &arg[1]);
                Ok(GetItemResponse {
                    ok: true,
                    data: Some(
                        self.redis
                            .lock()
                            .await
                            .broadcast(chan, s)
                            .to_string()
                            .into(),
                    ),
                })
            }
            RedisCommand::Subscribe => {
                if _req.args.is_none() {
                    return Err(anyhow!("No arguments given (required)"));
                }
                let arg = _req.args.unwrap();
                if arg.len() != 1 {
                    return Err(anyhow!(
                        "Invalid arguments count: {} (expected =1)",
                        arg.len()
                    ));
                }
                let channel = &arg[0];
                Ok(GetItemResponse {
                    ok: true,
                    data: Some(
                        self.redis
                            .lock()
                            .await
                            .add_subscriber(channel)
                            .to_string()
                            .into(),
                    ),
                })
            }
            RedisCommand::Fetch => {
                if _req.args.is_none() {
                    return Err(anyhow!("No arguments given (required)"));
                }
                let arg = _req.args.unwrap();
                if arg.len() != 1 {
                    return Err(anyhow!(
                        "Invalid arguments count: {} (expected =1)",
                        arg.len()
                    ));
                }
                let handler = arg[0].parse::<usize>()?;
                let try_query = self.redis.lock().await.fetch(handler);
                Ok(GetItemResponse {
                    ok: try_query.is_ok(),
                    data: if try_query.is_ok() {
                        Some(try_query.expect("").into())
                    } else {
                        None
                    },
                })
            }
            RedisCommand::Replicaof => {
                let mut curr_state = self.state.lock().await;
                if let RedisState::Single = *curr_state {
                    return Err(anyhow!(
                        "REPLICAOF is not supported in this state: `{}`.",
                        *curr_state
                    ));
                }
                if _req.args.is_none() {
                    return Err(anyhow!("No arguments given (required)"));
                }
                let arg = _req.args.unwrap();
                if arg.len() != 2 {
                    return Err(anyhow!(
                        "Invalid arguments count: {} (expected =0)",
                        arg.len()
                    ));
                }
                let host: Host = arg[0].to_string();
                let port: Port = arg[1].parse::<u32>()?;
                *curr_state = RedisState::SlaveOf(host, port);

                let resp = CLIENT
                    .get_item(volo_gen::volo::redis::GetItemRequest {
                        cmd: RedisCommand::Sync,
                        args: None,
                        transaction_id: None,
                    })
                    .await?;
                if !resp.ok || resp.data.is_none() {
                    return Err(anyhow!("Failed to get deserialized data."));
                }
                self.redis
                    .lock()
                    .await
                    .deserialize(resp.data.unwrap().into_bytes().to_vec());
                Ok(GetItemResponse {
                    ok: true,
                    data: None,
                })
            }
            RedisCommand::Sync => {
                let curr_state = self.state.lock().await;
                if let RedisState::Single = *curr_state {
                    return Err(anyhow!(
                        "SYNC is not supported in this state: `{}`.",
                        *curr_state
                    ));
                }
                let arg = _req.args.unwrap();
                if arg.len() != 0 {
                    return Err(anyhow!(
                        "Invalid arguments count: {} (expected =0)",
                        arg.len()
                    ));
                }
                let data = self.redis.lock().await.serialize();
                Ok(GetItemResponse {
                    ok: true,
                    data: Some(String::from_utf8(data).unwrap().into()),
                })
            }
            RedisCommand::ClusterMeet => {
                unimplemented!()
            }
            RedisCommand::ClusterAddSlots => {
                unimplemented!()
            }
            RedisCommand::ClusterCreate => {
                unimplemented!()
            }
            RedisCommand::Multi => {
                //generate a nanoid as transaction id make sure it is not a key in the Transaction hashmap
                let mut transaction_id = nanoid!(5);
                while TRANSACTION_HASHMAP
                    .read()
                    .await
                    .contains_key(&transaction_id)
                {
                    transaction_id = nanoid!(5);
                }
                TRANSACTION_HASHMAP.write().await.insert(
                    transaction_id.clone(),
                    Transaction {
                        commands: Vec::new(),
                        is_wrong: false,
                    },
                );
                Ok(GetItemResponse {
                    ok: true,
                    data: Some(transaction_id.into()),
                })
            }
            RedisCommand::Watch => {
                if let Some(arg) = &_req.args {
                    let watch_key = arg[0].clone().to_string();
                    let watch_transaction_id = _req.transaction_id.clone().unwrap().to_string();
                    let mut transaction_watcher = TRANSACTION_WATCHER.write().await;
                    let mut key_watched = KEY_WATCHED.write().await;
                    let mut transactions_watched_the_key = if key_watched.contains_key(&watch_key) {
                        key_watched.get_mut(&watch_key).unwrap()
                    } else {
                        let mut transactions = Vec::new();
                        key_watched.insert(watch_key.clone(), transactions);
                        key_watched.get_mut(&watch_key).unwrap()
                    };
                    let match_value = self.redis.lock().await.get(watch_key.as_ref());
                    //transaction_watcher is key map the watching transaction id and the value of the key
                    let response = if transaction_watcher.contains_key(&watch_key) {
                        let mut watch_pair = transaction_watcher.get_mut(&watch_key).unwrap();
                        if watch_pair.contains_key(&watch_transaction_id) {
                            Err(anyhow!("Transaction already watched"))
                        } else {
                            watch_pair.insert(watch_transaction_id.clone(), match_value.clone());
                            transactions_watched_the_key.push(watch_transaction_id.clone());
                            Ok(GetItemResponse {
                                ok: true,
                                data: Some("OK".into()),
                            })
                        }
                    } else {
                        let mut watch_pair = HashMap::new();
                        watch_pair.insert(watch_transaction_id.clone(), match_value.clone());
                        transaction_watcher.insert(watch_key.clone(), watch_pair);
                        transactions_watched_the_key.push(watch_transaction_id.clone());
                        Ok(GetItemResponse {
                            ok: true,
                            data: Some("OK".into()),
                        })
                    };
                    response
                } else {
                    Err(anyhow!("No arguments given (required)"))
                }
            }
            RedisCommand::Sync => {
                let curr_state = self.state.lock().await;
                if let RedisState::Single = *curr_state {
                    return Err(anyhow!(
                        "SYNC is not supported in this state: `{}`.",
                        *curr_state
                    ));
                }
                let arg = _req.args.unwrap();
                if arg.len() != 0 {
                    return Err(anyhow!(
                        "Invalid arguments count: {} (expected =0)",
                        arg.len()
                    ));
                }
                let data = self.redis.lock().await.serialize();
                Ok(GetItemResponse {
                    ok: true,
                    data: Some(String::from_utf8(data).unwrap().into()),
                })
            }
            RedisCommand::ClusterMeet => {
                unimplemented!()
            }
            RedisCommand::ClusterAddSlots => {
                unimplemented!()
            }
            RedisCommand::ClusterCreate => {
                unimplemented!()
            }
            _ => Err(anyhow!("Unsupported command")),
        };
        {
            {
                let ctrl_c = CTRL_C.lock().await;
                if *ctrl_c {
                    self.send_message("SHUTDOWN".to_string()).await;
                }
            }
            if *shared_data.lock().await {
                self.send_message("SHUTDOWN".to_string()).await;
                let mut ctrl_c = CTRL_C.lock().await;
                *ctrl_c = true;
                info!("New requests rejected whe shutting down.");
            }
        }
        let _ = child.abort();
        return response;
    }

    async fn exec(
        &self,
        _req: volo_gen::volo::redis::GetItemRequest,
    ) -> ::core::result::Result<
        volo_gen::volo::redis::MultiGetItemResponse,
        ::volo_thrift::AnyhowError,
    > {
        info!("deal with exec");
        {
            let ctrl_c = CTRL_C.lock().await;
            if *ctrl_c {
                return Err(anyhow!("Server is shutting").into());
            }
        }
        let shared_data = Arc::new(Mutex::new(false));
        let shared_data_clone = Arc::clone(&shared_data);
        let child = volo::spawn(async move {
            let _ = signal::ctrl_c().await;
            info!("Ctrl-C received, shutting down");
            *shared_data_clone.lock().await = true;
        });
        let response = match _req.cmd {
            RedisCommand::Exec => {
                let transaction_id = _req.transaction_id.unwrap();
                let mut transactions = TRANSACTION_HASHMAP.write().await;
                let mut transaction = transactions.get_mut(&transaction_id.to_string()).unwrap();
                let mut transaction_watcher = TRANSACTION_WATCHER.write().await;
                let key_watched = KEY_WATCHED.read().await;
                info!("dealing with checking");
                for key in key_watched.keys() {
                    if transaction_watcher.contains_key(key) {
                        let mut watch_pair = transaction_watcher.get_mut(&key.clone()).unwrap();
                        let old_value = watch_pair.get(&transaction_id.to_string()).unwrap();
                        let new_value = self.redis.lock().await.get(key.as_ref());
                        if old_value.to_owned() != new_value {
                            transaction.is_wrong = true;
                            break;
                        }
                    }
                }
                info!("checking done");
                if transaction.is_wrong {
                    Err(anyhow!("Transaction is wrong"))
                } else {
                    let mut responses = Vec::new();
                    for command in transaction.commands.iter() {
                        let resp = match command.cmd {
                            RedisCommand::Set => {
                                info!("deal with set");
                                if let Some(arg) = &command.args {
                                    if arg.len() < 2 {
                                        Err(anyhow!(
                                            "Invalid arguments count: {} (expected >=2)",
                                            arg.len()
                                        ))
                                    } else {
                                        let (key, value) = (&arg[0], &arg[1]);
                                        let mut milliseconds = 0;
                                        if let Some(exp_type) = arg.get(2) {
                                            if let Some(exp_num) = arg.get(3) {
                                                let exp_after = exp_num.parse::<u128>()?;

                                                match exp_type.to_lowercase().as_ref() {
                                                    "ex" => {
                                                        milliseconds = exp_after * 1000;
                                                    }
                                                    "px" => {
                                                        milliseconds = exp_after;
                                                    }
                                                    _ => {
                                                        return Err(anyhow!(
                                                            "Unsupported time type `{exp_type}`"
                                                        ));
                                                    }
                                                }
                                            } else {
                                                return Err(anyhow!(
                                                    "Duration number not provided."
                                                ));
                                            }
                                        }
                                        let command_str = format!(
                                            "SET {} {} {}\n",
                                            key,
                                            value,
                                            if milliseconds != 0 {
                                                now() + milliseconds
                                            } else {
                                                0u128
                                            }
                                        );
                                        self.send_message(command_str).await;
                                        self.redis.lock().await.set_after(
                                            key.as_ref(),
                                            value.as_ref(),
                                            milliseconds,
                                        );
                                        Ok(GetItemResponse {
                                            ok: true,
                                            data: Some("OK".into()),
                                        })
                                    }
                                } else {
                                    Err(anyhow!("No arguments given (required)"))
                                }
                            }
                            RedisCommand::Get => {
                                info!("deal with get");
                                if let Some(arg) = &command.args {
                                    if arg.len() != 1 {
                                        Err(anyhow!(
                                            "Invalid arguments count: {} (expected 1)",
                                            arg.len()
                                        ))
                                    } else {
                                        if let Some(value) =
                                            self.redis.lock().await.get(arg[0].as_ref())
                                        {
                                            Ok(GetItemResponse {
                                                ok: true,
                                                data: Some(value.into()),
                                            })
                                        } else {
                                            Ok(GetItemResponse {
                                                ok: false,
                                                data: None,
                                            })
                                        }
                                    }
                                } else {
                                    info!("{:?}", &_req.args);
                                    Err(anyhow!("No arguments given (required)"))
                                }
                            }
                            _ => Err(anyhow!("Unsupported command")),
                        };
                        responses.push(resp.unwrap());
                    }
                    transactions.remove(&transaction_id.to_string());
                    info!("exec done with {:?}", responses);
                    Ok(MultiGetItemResponse {
                        ok: true,
                        data: Some(responses),
                    })
                }
            }
            _ => Err(anyhow!("Unsupported command")),
        };
        {
            {
                let ctrl_c = CTRL_C.lock().await;
                if *ctrl_c {
                    self.send_message("SHUTDOWN".to_string()).await;
                }
            }
            if *shared_data.lock().await {
                self.send_message("SHUTDOWN".to_string()).await;
                let mut ctrl_c = CTRL_C.lock().await;
                *ctrl_c = true;
                info!("New requests rejected whe shutting down.");
            }
        }
        let _ = child.abort();
        return response;
    }
}
