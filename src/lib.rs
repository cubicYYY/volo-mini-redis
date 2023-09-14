#![feature(impl_trait_in_assoc_type)]

mod redis;

use anyhow::{ anyhow, Ok };
use redis::Timestamp;
use std::{
    fs::{ File, OpenOptions },
    io::{ BufRead, BufReader, Seek, SeekFrom, Write },
    // sync::mpsc,
    thread,
    time::{ Duration, Instant, SystemTime, UNIX_EPOCH },
};
use tokio::{ sync::RwLock, signal, sync::mpsc };
use volo_gen::volo::redis::RedisCommand;
use std::sync::{ Arc, Mutex };
pub struct S {
    pub redis: RwLock<redis::Redis>,
    sender: mpsc::Sender<String>,
}
impl S {
    pub fn new() -> S {
        let (sender, mut receiver) = mpsc::channel(100);
        // Spawn a thread to handle received messages
        tokio::spawn(async move {
            let mut command: Vec<String> = Vec::new();
            let mut aof_file = OpenOptions::new()
                .append(true)
                .create(true)
                .open("./src/AOF_FILE")
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
                match receiver.recv().await {
                    Some(msg) => {
                        command.push(msg);
                        // 检查是否距上次写入已经过去了 1 秒
                        if last_write_time.elapsed() >= Duration::from_secs(1) {
                            flush(&command);
                            command.clear();
                            last_write_time = Instant::now();
                        }
                    }
                    None => {
                        flush(&command);
                        command.clear();
                        last_write_time = Instant::now();
                    }
                }
            }
        });
        S {
            redis: RwLock::new(redis::Redis::new()),
            sender,
        }
    }
    async fn send_message(&self, msg: String) {
        self.sender.send(msg).await;
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

#[volo::async_trait]
impl volo_gen::volo::redis::ItemService for S {
    async fn get_item(
        &self,
        _req: volo_gen::volo::redis::GetItemRequest
    ) -> ::core::result::Result<
        volo_gen::volo::redis::GetItemResponse,
        ::volo_thrift::AnyhowError
    > {
        // {
        //     let ctrl_c = CTRL_C.read().await;
        //     if *ctrl_c {
        //         return Err(anyhow!("Server is shutting").into());
        //     }
        // }
        // let shared_data = Arc::new(Mutex::new(false));
        // let shared_data_clone = Arc::clone(&shared_data);
        // volo::spawn(async move {
        //     let _ = signal::ctrl_c().await;
        //     *shared_data_clone.lock().unwrap() = true;
        // });
        use volo_gen::volo::redis::GetItemResponse;
        let response = match _req.cmd {
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
                if let Some(arg) = _req.args {
                    if arg.len() != 1 {
                        Err(anyhow!("Invalid arguments count: {} (expected 1)", arg.len()))
                    } else {
                        if let Some(value) = self.redis.write().await.get(arg[0].as_ref()) {
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
                        Err(anyhow!("Invalid arguments count: {} (expected >=2)", arg.len()))
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
                        let command_str = format!("SET {} {} {}\n", key, value, if
                            milliseconds != 0
                        {
                            now() + milliseconds
                        } else {
                            0u128
                        });
                        self.send_message(command_str).await;
                        println!("xxx");
                        self.redis
                            .write().await
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
            RedisCommand::Del => {
                if let Some(arg) = _req.args {
                    if arg.len() < 1 {
                        Err(anyhow!("Invalid arguments count: {} (expected >= 1)", arg.len()))
                    } else {
                        let mut success: u16 = 0;
                        for key in arg {
                            success += self.redis.write().await.del(key.as_ref()) as u16;
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
                        Err(anyhow!("Invalid arguments count: {} (expected =2)", arg.len()))
                    } else {
                        let (chan, s) = (&arg[0], &arg[1]);
                        Ok(GetItemResponse {
                            ok: true,
                            data: Some(
                                self.redis.write().await.broadcast(chan, s).to_string().into()
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
                        Err(anyhow!("Invalid arguments count: {} (expected =1)", arg.len()))
                    } else {
                        let channel = &arg[0];
                        Ok(GetItemResponse {
                            ok: true,
                            data: Some(
                                self.redis.write().await.add_subscriber(channel).to_string().into()
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
                        Err(anyhow!("Invalid arguments count: {} (expected =1)", arg.len()))
                    } else {
                        let handler = arg[0].parse::<usize>()?;
                        let try_query = self.redis.read().await.fetch(handler);
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
        };
        // {
        //     let ctrl_c = CTRL_C.read().await;
        //     if *ctrl_c {
        //         thread::sleep(Duration::from_secs(2));
        //     } else if *shared_data.lock().unwrap() {
        //         let mut ctrl_c = CTRL_C.write().await;
        //         *ctrl_c = true;
        //         thread::sleep(Duration::from_secs(2));
        //     }
        // }
        return response;
    }
}
