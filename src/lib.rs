#![feature(impl_trait_in_assoc_type)]

mod redis;

use anyhow::{anyhow, Ok};
use lazy_static::lazy_static;
use rand::distributions::Alphanumeric;
use rand::Rng;
use redis::Timestamp;
use std::borrow::BorrowMut;
use std::sync::Arc;
use std::{
    fs::{File, OpenOptions},
    io::{BufRead, BufReader, Seek, SeekFrom, Write},
    net::SocketAddr,
    thread,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};
use tokio::sync::mpsc;
use tokio::{signal, sync::Mutex};
use tracing::info;
use uuid::Uuid;
use volo_gen::volo::redis::RedisCommand;

pub type RedisID = String;
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

lazy_static! {
    static ref MASTER_ADDR: String = String::from("127.0.0.1:6379"); // TODO: read from command line
    static ref CTRL_C: Arc<Mutex<bool>> = Arc::new(Mutex::new(false));
    static ref CLIENT: volo_gen::volo::redis::ItemServiceClient = {
        let addr: SocketAddr = MASTER_ADDR.parse().unwrap();
        volo_gen::volo::redis::ItemServiceClientBuilder::new("volo-redis")
            .layer_outer(TimedLayer)
            .layer_outer(AsciiFilterLayer)
            .address(addr)
            .build()
    };
}

type ArwLock<T> = Arc<Mutex<T>>;
pub struct S {
    pub redis: ArwLock<redis::Redis>,
    sender: mpsc::Sender<String>,
    state: ArwLock<RedisState>,
    in_cluster: bool,
    id: RedisID,
}

impl S {
    pub fn new() -> S {
        let (sender, mut receiver) = mpsc::channel(1024);
        let id: RedisID = rand::thread_rng()
            .sample_iter(&Alphanumeric)
            .take(16)
            .map(char::from)
            .collect();
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
            in_cluster: false,
            id, // TODO: can recover from old id
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
                if let Some(arg) = _req.args {
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
                        println!("xxx");
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
                if let Some(arg) = _req.args {
                    if arg.len() < 1 {
                        Err(anyhow!(
                            "Invalid arguments count: {} (expected >= 1)",
                            arg.len()
                        ))
                    } else {
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
                } else {
                    Err(anyhow!("No arguments given (required)"))
                }
            }
            RedisCommand::Publish => {
                if let Some(arg) = _req.args {
                    if arg.len() != 2 {
                        Err(anyhow!(
                            "Invalid arguments count: {} (expected =2)",
                            arg.len()
                        ))
                    } else {
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
                } else {
                    Err(anyhow!("No arguments given (required)"))
                }
            }
            RedisCommand::Subscribe => {
                if let Some(arg) = _req.args {
                    if arg.len() != 1 {
                        Err(anyhow!(
                            "Invalid arguments count: {} (expected =1)",
                            arg.len()
                        ))
                    } else {
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
                } else {
                    Err(anyhow!("No arguments given (required)"))
                }
            }
            RedisCommand::Fetch => {
                if let Some(arg) = _req.args {
                    if arg.len() != 1 {
                        Err(anyhow!(
                            "Invalid arguments count: {} (expected =1)",
                            arg.len()
                        ))
                    } else {
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
                } else {
                    Err(anyhow!("No arguments given (required)"))
                }
            }
            RedisCommand::Replicaof => {
                let mut curr_state = self.state.lock().await;
                if let RedisState::Single = *curr_state {
                    Err(anyhow!(
                        "REPLICAOF is not supported in this state: `{}`.",
                        *curr_state
                    ))
                } else {
                    if let Some(arg) = _req.args {
                        if arg.len() != 2 {
                            Err(anyhow!(
                                "Invalid arguments count: {} (expected =0)",
                                arg.len()
                            ))
                        } else {
                            let host: Host = arg[0].to_string();
                            let port: Port = arg[1].parse::<u32>()?;
                            *curr_state = RedisState::SlaveOf(host, port);

                            let resp = CLIENT
                                .get_item(volo_gen::volo::redis::GetItemRequest {
                                    cmd: RedisCommand::Sync,
                                    args: None,
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
                    } else {
                        Err(anyhow!("No arguments given (required)"))
                    }
                }
            }
            RedisCommand::Sync => {
                let curr_state = self.state.lock().await;
                if let RedisState::Single = *curr_state {
                    Err(anyhow!(
                        "SYNC is not supported in this state: `{}`.",
                        *curr_state
                    ))
                } else {
                    if let Some(arg) = _req.args {
                        if arg.len() != 0 {
                            Err(anyhow!(
                                "Invalid arguments count: {} (expected =0)",
                                arg.len()
                            ))
                        } else {
                            let data = self.redis.lock().await.serialize();
                            Ok(GetItemResponse {
                                ok: true,
                                data: Some(String::from_utf8(data).unwrap().into()),
                            })
                        }
                    } else {
                        Err(anyhow!("No arguments given (required)"))
                    }
                }
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
