namespace rs volo.redis

enum RedisCommand {
    Ping,
    Get,
    Set,
    Del,
    Publish,
    Subscribe
}

struct GetItemRequest {
    1: required RedisCommand cmd,
    2: optional list<string> args,
}

struct GetItemResponse {
    1: required bool ok,
    2: optional string data,
}

service ItemService {
    GetItemResponse GetItem (1: GetItemRequest req),
}

