#![feature(impl_trait_in_assoc_type)]

mod redis;

use std::cell::RefCell;

use pilota::FastStr;
use volo_gen::volo::redis::RedisCommand;

pub struct S {
    redis: RefCell<redis::Redis>,
}

impl S {
	pub fn new() -> S {
		S {
			redis: RefCell::new(redis::Redis::new())
		}
	}
}

/// FIXME Mutex or RwLock
/// SAFETY: NONE! 
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
        match _req.cmd {
            RedisCommand::Ping => {
                if let Some(arg) = _req.args {
                    Ok(GetItemResponse {
                        ok: true,
                        data: Some(FastStr::from(arg.join(" "))),
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
                        Ok(GetItemResponse {
                            ok: false,
                            data: Some(FastStr::from(format!(
                                "Invalid arguments count: {} (expected 1)",
                                arg.len()
                            ))),
                        })
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
                    Ok(GetItemResponse {
                        ok: false,
                        data: Some(FastStr::from("No arguments given (required)")),
                    })
                }
            }
            RedisCommand::Set => {
                if let Some(arg) = _req.args {
                    if arg.len() < 2 {
                        Ok(GetItemResponse {
                            ok: false,
                            data: Some(FastStr::from(format!(
                                "Invalid arguments count: {} (expected >= 2)",
                                arg.len()
                            ))),
                        })
                    } else {
						let (key, value) = (&arg[0], &arg[1]);
                        if self.redis.borrow_mut().set(key.as_ref(), value.as_ref(), 0) { // TODO expire time from arg
                            Ok(GetItemResponse {
                                ok: true,
                                data: None,
                            })
                        } else {
                            Ok(GetItemResponse {
                                ok: false,
                                data: None,
                            })
                        }
                    }
                } else {
                    Ok(GetItemResponse {
                        ok: false,
                        data: Some(FastStr::from("No arguments given (required)")),
                    })
                }
            }
            RedisCommand::Del => {
                if let Some(arg) = _req.args {
                    if arg.len() < 1 {
                        Ok(GetItemResponse {
                            ok: false,
                            data: Some(FastStr::from(format!(
                                "Invalid arguments count: {} (expected >= 1)",
                                arg.len()
                            ))),
                        })
                    } else {
                        let mut success: u16 = 0;
                        for key in arg {
                            success += self.redis.borrow_mut().del(key.as_ref()) as u16;
                        }
                        Ok(GetItemResponse {
                            ok: true,
                            data: Some(FastStr::from(format!("{}", success))),
                        })
                    }
                } else {
                    Ok(GetItemResponse {
                        ok: false,
                        data: Some(FastStr::from("No arguments given (required)")),
                    })
                }
            }
			RedisCommand::Publish => {unimplemented!()}
			RedisCommand::Subscribe => {unimplemented!()}
        }
    }
}
