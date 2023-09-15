#![feature(impl_trait_in_assoc_type)]

pub mod cmdargs;
mod redis;

use anyhow::{ anyhow, Ok };
use clap::{ self, Parser };
use cmdargs::ServerConfig;
use lazy_static::lazy_static;
use nanoid::nanoid;
use pilota::FastStr;
use redis::Timestamp;
use std::collections::HashMap;
use std::net::IpAddr;
use std::str::FromStr;
use std::sync::Arc;
use std::{
    fs::OpenOptions,
    io::Write,
    net::SocketAddr,
    time::{ Duration, Instant, SystemTime, UNIX_EPOCH },
};
use tokio::sync::mpsc;
use tokio::{ signal, sync::Mutex };
use tracing::info;
use uuid::Uuid;
use volo::net::Address;
use volo_gen::volo::redis::{ GetItemRequest, GetItemResponse, MultiGetItemResponse, RedisCommand };

pub type Host = IpAddr;
pub type Port = u16;

pub enum RedisState {
    Single,
    Master, // m-s mode / cluster mode
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

pub fn get_client(addr: impl Into<Address>) -> volo_gen::volo::redis::ItemServiceClient {
    volo_gen::volo::redis::ItemServiceClientBuilder
        ::new("volo-redis")
        .layer_outer(TimedLayer)
        .layer_outer(AsciiFilterLayer)
        .address(addr)
        .build()
}
type AMutex<T> = Arc<Mutex<T>>;
lazy_static! {
    static ref REDIS: AMutex<redis::Redis> = Arc::new(Mutex::new(redis::Redis::new()));
    // Command line args
    static ref CMD_ARGS: ServerConfig = ServerConfig::parse();

    static ref NAME: Option<String> = CMD_ARGS.name.clone();
    static ref SLAVE_OF: Option<String> = CMD_ARGS.slaveof.clone();
    static ref SELF_PUB_ADDR: String = format!("{}:{}",CMD_ARGS.ip.clone(), CMD_ARGS.port.clone());
    static ref MASTER_ADDR: String = String::from((CMD_ARGS.slaveof).clone().expect("No master ADDR specified."));
    // static ref IS_CLUSTER: bool = CMD_ARGS.cluster > 0;
    static ref PRE_RUN: Option<Vec<String>> = CMD_ARGS.pre_run.clone();

    static ref CTRL_C: Arc<Mutex<bool>> = Arc::new(Mutex::new(false));
    static ref TO_MASTER_CLIENT: volo_gen::volo::redis::ItemServiceClient = {
        let addr: SocketAddr = MASTER_ADDR.parse().unwrap();
        get_client(addr)
    };

    //transaction id match the commands in the transaction
    pub static ref TRANSACTION_HASHMAP: Arc<Mutex<HashMap<String, Transaction>>> = Arc::new(Mutex::new(HashMap::new()));
    //key match the transaction pair with the value of the key
    pub static ref TRANSACTION_WATCHER: Arc<Mutex<HashMap<String, HashMap<String, Option<String>>>>> = Arc::new(Mutex::new(HashMap::new()));
    //keys a transaction is watching
    pub static ref KEY_WATCHED: Arc<Mutex<HashMap<String, Vec<String>>>> = Arc::new(Mutex::new(HashMap::new()));
}

pub struct S {
    pub redis: &'static AMutex<redis::Redis>,
    sender: AMutex<mpsc::Sender<String>>,
    pub state: AMutex<RedisState>,
    pub uuid: AMutex<Uuid>, // TODO: remove this lock as it will only be modify once by main thread
    pub client_addrs: AMutex<HashMap<Uuid, SocketAddr>>,
}
pub struct Transaction {
    pub commands: Vec<GetItemRequest>,
    pub is_wrong: bool,
}
impl S {
    pub async fn new() -> S {
        let (sender, mut receiver) = mpsc::channel(1024);
        // Spawn a thread to periodically flush the data into AOF file
        tokio::spawn(async move {
            let mut command: Vec<String> = Vec::new();
            let mut aof_file = OpenOptions::new()
                .append(true)
                .create(true)
                .open(
                    format!("{}.aof", if NAME.is_none() {
                        "server"
                    } else {
                        NAME.as_ref().unwrap().as_str()
                    })
                )
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
                        if msg == "SHUTDOWN".to_string() {
                            flush(&command);
                            command.clear();
                        } else {
                            command.push(msg);
                            // 检查是否距上次写入已经过去了 1 秒
                            if last_write_time.elapsed() >= Duration::from_secs(1) {
                                flush(&command);
                                command.clear();
                                last_write_time = Instant::now();
                            }
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

        let s = S {
            redis: &REDIS,
            sender: Arc::new(Mutex::new(sender)),
            state: Arc::new(Mutex::new(RedisState::Single)),
            uuid: Arc::new(Mutex::new(Uuid::nil())),
            client_addrs: Arc::new(Mutex::new(HashMap::new())),
        };
        // pre-run commands,
        // TODO
        // ...

        // if SLAVE: then sync
        if let Some(master) = SLAVE_OF.clone() {
            let addr: SocketAddr = master.parse().unwrap();
            s.react_to_command(GetItemRequest {
                cmd: RedisCommand::Replicaof,
                args: Some(vec![addr.ip().to_string().into(), addr.port().to_string().into()]),
                client_id: None,
                transaction_id: None,
            }).await.expect("Failed to execute init commands!");
            println!("Sync send.");
        } else {
            // Gen for no-slaves
            *s.uuid.lock().await = Uuid::new_v4();
        }
        s
    }
    async fn send_message(&self, msg: String) {
        let _ = self.sender.lock().await.send(msg).await;
    }
}

#[derive(Clone)]
pub struct AsciiFilter<S>(S);

#[volo::service]
impl<Cx, Req, S> volo::Service<Cx, Req>
    for AsciiFilter<S>
    where
        Req: std::fmt::Debug + Send + 'static,
        S: Send + 'static + volo::Service<Cx, Req> + Sync,
        S::Response: std::fmt::Debug,
        S::Error: std::fmt::Debug,
        anyhow::Error: Into<S::Error>,
        Cx: Send + 'static
{
    async fn call(&self, cx: &mut Cx, req: Req) -> Result<S::Response, S::Error> {
        let resp = self.0.call(cx, req).await;
        if resp.is_err() {
            return resp;
        }
        if
            format!("{:?}", resp)
                .chars()
                .any(|c| ((c as u32) < 32 || (c as u32) > 127))
        {
            Err(
                anyhow!(
                    "Invalid chars found. Only printable ASCII chars are allowed in requests."
                ).into()
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
impl<Cx, Req, S> volo::Service<Cx, Req>
    for Timed<S>
    where
        Req: std::fmt::Debug + Send + 'static,
        S: Send + 'static + volo::Service<Cx, Req> + Sync,
        S::Response: std::fmt::Debug,
        S::Error: std::fmt::Debug,
        Cx: Send + 'static
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
impl S {
    async fn react_to_command(
        &self,
        _req: GetItemRequest
    ) -> ::core::result::Result<
        volo_gen::volo::redis::GetItemResponse,
        ::volo_thrift::AnyhowError
    > {
        match _req.cmd {
            RedisCommand::Ping => {
                if let Some(arg) = _req.args {
                    let ans = if arg.len() == 0 { "pong".into() } else { arg.join(" ").into() };
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
                if _req.args.is_none() {
                    return Err(anyhow!("No arguments given (required)"));
                }
                let arg = _req.args.unwrap();
                if arg.len() != 1 {
                    return Err(anyhow!("Invalid arguments count: {} (expected 1)", arg.len()));
                }
                if let Some(value) = REDIS.lock().await.get(arg[0].as_ref()) {
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
            RedisCommand::Set => {
                // let mut is_master: bool = false;
                {
                    let curr_state = self.state.lock().await;
                    if let RedisState::SlaveOf(_, _) = *curr_state {
                        if _req.client_id.is_none() {
                            return Err(
                                anyhow!("Set is forbidden on slave node if no uuid provided.")
                            );
                        }
                    }
                    // if let RedisState::Master = *curr_state {
                    //     is_master = true;
                    // }
                }
                if _req.args.is_none() {
                    return Err(anyhow!("No arguments given (required)"));
                }
                let arg = _req.args.unwrap();
                if arg.len() < 2 {
                    return Err(anyhow!("Invalid arguments count: {} (expected >=2)", arg.len()));
                }
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
                let command_str = format!("SET {} {} {}\n", key, value, if milliseconds != 0 {
                    now() + milliseconds
                } else {
                    0u128
                });
                self.send_message(command_str).await;
                println!("xxx");
                REDIS.lock().await.set_after(key.as_ref(), value.as_ref(), milliseconds);
                // propagate to slaves
                // no need to be master!
                let caddr = self.client_addrs.lock().await;
                println!("propagate to {} clients", caddr.len());
                for (_, cliaddr) in (*caddr).iter() {
                    println!("set... to {}.", cliaddr.clone().to_string());
                    let _resp = get_client(cliaddr.clone()).get_item(
                        volo_gen::volo::redis::GetItemRequest {
                            cmd: RedisCommand::Set,
                            args: Some(arg.clone()),
                            client_id: Some((*self.uuid.lock().await).to_string().into()), // Not Forwarded
                            transaction_id: _req.transaction_id.clone(), // Forwarded
                        }
                    ).await;
                }

                Ok(GetItemResponse {
                    ok: true,
                    data: Some("OK".into()),
                })
            }
            RedisCommand::Del => {
                {
                    let curr_state = self.state.lock().await;
                    if let RedisState::SlaveOf(_, _) = *curr_state {
                        if _req.client_id.is_none() {
                            return Err(
                                anyhow!("Del is forbidden on slave node if no uuid provided.")
                            );
                        }
                    }
                    // if let RedisState::Master = *curr_state {
                    //     is_master = true;
                    // }
                }
                if _req.args.is_none() {
                    return Err(anyhow!("No arguments given (required)"));
                }
                let arg = _req.args.unwrap();
                if arg.len() < 1 {
                    return Err(anyhow!("Invalid arguments count: {} (expected >= 1)", arg.len()));
                }
                let mut success: u16 = 0;
                for key in arg.iter() {
                    success += REDIS.lock().await.del(key.as_ref()) as u16;
                    let command_str = format!("DEL {:} 0 0\n", key);
                    self.send_message(command_str).await;
                }
                // propagate to slaves
                // no need to be master!
                let caddr = self.client_addrs.lock().await;
                println!("propagate to {} clients", caddr.len());
                for (_, cliaddr) in (*caddr).iter() {
                    println!("del... to {}.", cliaddr.clone().to_string());
                    let _resp = get_client(cliaddr.clone()).get_item(
                        volo_gen::volo::redis::GetItemRequest {
                            cmd: RedisCommand::Del,
                            args: Some(arg.clone()),
                            client_id: Some((*self.uuid.lock().await).to_string().into()), // Not Forwarded
                            transaction_id: _req.transaction_id.clone(), // Forwarded
                        }
                    ).await;
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
                    return Err(anyhow!("Invalid arguments count: {} (expected =2)", arg.len()));
                }
                let (chan, s) = (&arg[0], &arg[1]);
                Ok(GetItemResponse {
                    ok: true,
                    data: Some(REDIS.lock().await.broadcast(chan, s).to_string().into()),
                })
            }
            RedisCommand::Subscribe => {
                if _req.args.is_none() {
                    return Err(anyhow!("No arguments given (required)"));
                }
                let arg = _req.args.unwrap();
                if arg.len() != 1 {
                    return Err(anyhow!("Invalid arguments count: {} (expected =1)", arg.len()));
                }
                let channel = &arg[0];
                Ok(GetItemResponse {
                    ok: true,
                    data: Some(REDIS.lock().await.add_subscriber(channel).to_string().into()),
                })
            }
            RedisCommand::Replicaof => {
                // This will change the master.

                let mut curr_state = self.state.lock().await;

                if _req.args.is_none() {
                    return Err(anyhow!("No arguments given (required)"));
                }
                let arg = _req.args.unwrap();
                if arg.len() != 2 {
                    return Err(anyhow!("Invalid arguments count: {} (expected =0)", arg.len()));
                }

                // Modify master, and replace the CLIENT to master node
                let mst_host: Host = arg[0].to_string().parse()?;
                let mst_port: Port = arg[1].parse()?;
                let _mst_addr = SocketAddr::new(mst_host, mst_port);
                *curr_state = RedisState::SlaveOf(mst_host.clone(), mst_port.clone());

                let self_addr: SocketAddr = SELF_PUB_ADDR.parse().unwrap();
                // TODO: replace the global client as a performance boost
                let resp = get_client(SocketAddr::new(mst_host, mst_port)).get_item(
                    volo_gen::volo::redis::GetItemRequest {
                        cmd: RedisCommand::Sync,
                        args: Some(
                            vec![
                                self_addr.ip().to_string().into(),
                                self_addr.port().to_string().into()
                            ]
                        ),
                        client_id: None,
                        transaction_id: None,
                    }
                ).await?;

                if !resp.ok {
                    return Err(anyhow!("Sync failed: error at server side"));
                }
                let uuid = resp.data.unwrap();
                *self.uuid.lock().await = Uuid::from_str(&uuid.to_string())?;

                Ok(GetItemResponse {
                    ok: true,
                    data: Some("OK".into()),
                })
            }
            RedisCommand::Sync => {
                // Start full sync server-side, return a UUID as an identifier of this client
                // Should provide 2 ags: client ip, port
                {
                    let mut curr_state = self.state.lock().await;
                    if let RedisState::Single = *curr_state {
                        *curr_state = RedisState::Master;
                    }
                }
                let arg = _req.args.unwrap();
                if arg.len() != 2 {
                    return Err(
                        anyhow!(
                            "Invalid arguments count: {} (expected =2, pub_ip, pub_port)",
                            arg.len()
                        )
                    );
                }
                let clihost: IpAddr = arg[0].to_string().parse()?;
                let cliport: Port = arg[1].parse::<u16>()?;

                let gen_uuid = Uuid::new_v4();
                // add this uuid to client list
                let mut caddr = self.client_addrs.lock().await;
                caddr.insert(gen_uuid, SocketAddr::new(clihost, cliport));

                tokio::spawn(async move {
                    let readonly = REDIS.lock().await;
                    let data = readonly.serialize(); // HEAVY WORKLOAD
                    //...When the data is generated:
                    let _resp = get_client(SocketAddr::new(clihost, cliport)).get_item(
                        volo_gen::volo::redis::GetItemRequest {
                            cmd: RedisCommand::SyncGot,
                            args: Some(vec![unsafe { FastStr::from_vec_u8_unchecked(data) }]),
                            client_id: None,
                            transaction_id: None,
                        }
                    ).await;
                });
                Ok(GetItemResponse {
                    ok: true,
                    data: Some(gen_uuid.to_string().into()), // this UUID will be decoded in Replicaof command at the client side
                })
            }
            RedisCommand::ClusterMeet => { unimplemented!() }
            RedisCommand::ClusterAddSlots => { unimplemented!() }
            RedisCommand::ClusterCreate => { unimplemented!() }
            RedisCommand::Exec => { unimplemented!() }
            RedisCommand::Multi => {
                //generate a nanoid as transaction id make sure it is not a key in the Transaction hashmap
                let mut transaction_id = nanoid!(5);
                while TRANSACTION_HASHMAP.lock().await.contains_key(&transaction_id) {
                    transaction_id = nanoid!(5);
                }
                TRANSACTION_HASHMAP.lock().await.insert(transaction_id.clone(), Transaction {
                    commands: Vec::new(),
                    is_wrong: false,
                });
                Ok(GetItemResponse {
                    ok: true,
                    data: Some(transaction_id.into()),
                })
            }
            RedisCommand::Watch => {
                if let Some(arg) = &_req.args {
                    let watch_key = arg[0].clone().to_string();
                    let watch_transaction_id = _req.transaction_id.clone().unwrap().to_string();
                    let mut transaction_watcher = TRANSACTION_WATCHER.lock().await;
                    let mut key_watched = KEY_WATCHED.lock().await;
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
            // Internal commands, you can seen as remote interupts
            RedisCommand::Fetch => {
                if _req.args.is_none() {
                    return Err(anyhow!("No arguments given (required)"));
                }
                let arg = _req.args.unwrap();
                if arg.len() != 1 {
                    return Err(anyhow!("Invalid arguments count: {} (expected =1)", arg.len()));
                }
                let handler = arg[0].parse::<usize>()?;
                let try_query = REDIS.lock().await.fetch(handler);
                Ok(GetItemResponse {
                    ok: try_query.is_ok(),
                    data: if try_query.is_ok() {
                        Some(try_query.expect("").into())
                    } else {
                        None
                    },
                })
            }
            RedisCommand::SyncGot => {
                if _req.args.is_none() {
                    return Err(anyhow!("Failed to get deserialized data."));
                }
                let payload = _req.args.unwrap();
                if payload.len() != 1 {
                    return Err(anyhow!("Illegal deserial data format."));
                }
                REDIS.lock().await.deserialize(payload[0].to_owned().into_bytes().to_vec());
                info!("Serialize success!!!");
                Ok(GetItemResponse {
                    ok: true,
                    data: None,
                })
            }
        }
    }
}

#[volo::async_trait]
impl volo_gen::volo::redis::ItemService for S {
    async fn get_item(
        &self,
        _req: volo_gen::volo::redis::GetItemRequest
    ) -> ::core::result::Result<
        volo_gen::volo::redis::GetItemResponse,
        ::volo_thrift::AnyhowError
    > {
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
        let response = self.react_to_command(_req).await;
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
        _req: volo_gen::volo::redis::GetItemRequest
    ) -> ::core::result::Result<
        volo_gen::volo::redis::MultiGetItemResponse,
        ::volo_thrift::AnyhowError
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
                let mut transactions = TRANSACTION_HASHMAP.lock().await;
                let mut transaction = transactions.get_mut(&transaction_id.to_string()).unwrap();
                let mut transaction_watcher = TRANSACTION_WATCHER.lock().await;
                let key_watched = KEY_WATCHED.lock().await;
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
                                        Err(
                                            anyhow!(
                                                "Invalid arguments count: {} (expected >=2)",
                                                arg.len()
                                            )
                                        )
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
                                                        return Err(
                                                            anyhow!(
                                                                "Unsupported time type `{exp_type}`"
                                                            )
                                                        );
                                                    }
                                                }
                                            } else {
                                                return Err(
                                                    anyhow!("Duration number not provided.")
                                                );
                                            }
                                        }
                                        let command_str = format!("SET {} {} {}\n", key, value, if
                                            milliseconds != 0
                                        {
                                            now() + milliseconds
                                        } else {
                                            0u128
                                        });
                                        self.send_message(command_str).await;
                                        self.redis
                                            .lock().await
                                            .set_after(key.as_ref(), value.as_ref(), milliseconds);
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
                                        Err(
                                            anyhow!(
                                                "Invalid arguments count: {} (expected 1)",
                                                arg.len()
                                            )
                                        )
                                    } else {
                                        if
                                            let Some(value) = self.redis
                                                .lock().await
                                                .get(arg[0].as_ref())
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
