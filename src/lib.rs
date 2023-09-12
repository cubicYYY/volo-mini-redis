#![feature(impl_trait_in_assoc_type)]

mod redis;

use std::cell::RefCell;

use anyhow::anyhow;
use pilota::FastStr;
use volo_gen::volo::redis::RedisCommand;

pub struct S {
    redis: RefCell<redis::Redis>,
}

impl S {
    pub fn new() -> S {
        S {
            redis: RefCell::new(redis::Redis::new()),
        }
    }
}

// FIXME: Mutex or RwLock
// SAFETY: NONE!
unsafe impl Sync for S {}
unsafe impl Send for S {}

#[volo::async_trait]
impl volo_gen::volo::redis::ItemService for S {
    async fn get_item(
        &self,
        _req: volo_gen::volo::redis::GetItemRequest,
    ) -> ::core::result::Result<volo_gen::volo::redis::GetItemResponse, ::volo_thrift::AnyhowError>
    {
        // use volo_gen::volo::redis::GetItemRequest;
        use volo_gen::volo::redis::GetItemResponse;
        println!("Received: {:?} {:?}", _req.cmd, _req.args);
        match _req.cmd {
            RedisCommand::Ping => {
                if let Some(arg) = _req.args {
                    let ans = if arg.len() == 0 {
                        FastStr::from("pong")
                    } else {
                        FastStr::from(arg.join(" "))
                    };
                    Ok(GetItemResponse {
                        ok: true,
                        data: Some(ans),
                    })
                } else {
                    Ok(GetItemResponse {
                        ok: true,
                        data: Some(FastStr::from("pong")),
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
                        if let Some(value) = self.redis.borrow_mut().get(arg[0].as_ref()) {
                            Ok(GetItemResponse {
                                ok: true,
                                data: Some(FastStr::from(value)),
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
                                let exp_after = if let Ok(num) = exp_num.parse::<u128>() {
                                    num
                                } else {
                                    return Err(anyhow!("Illegal time duration {exp_num}"));
                                };

                                match exp_type.to_lowercase().as_ref() {
                                    "ex" => milliseconds = exp_after * 1000,
                                    "px" => milliseconds = exp_after,
                                    _ => {
                                        return Err(anyhow!("Unsupported time type {exp_type}"));
                                    }
                                }
                            } else {
                                return Err(anyhow!("Duration number not provided."));
                            }
                        }
                        self.redis
                            .borrow_mut()
                            .set(key.as_ref(), value.as_ref(), milliseconds);
                        println!("SET!{}", milliseconds);
                        Ok(GetItemResponse {
                            ok: true,
                            data: None,
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
                            success += self.redis.borrow_mut().del(key.as_ref()) as u16;
                        }
                        Ok(GetItemResponse {
                            ok: true,
                            data: Some(FastStr::from(format!("{success}"))),
                        })
                    }
                } else {
                    Err(anyhow!("No arguments given (required)"))
                }
            }
            RedisCommand::Publish => {
                unimplemented!()
            }
            RedisCommand::Subscribe => {
                unimplemented!()
            }
        }
    }
}
