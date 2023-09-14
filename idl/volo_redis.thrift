namespace rs volo.redis

enum RedisCommand {
    Ping,
    Get,
    Set,
    Del,
    Publish,
    Subscribe,
    Replicaof,
    Sync,
    ClusterCreate,
    ClusterMeet,
    ClusterAddSlots,
    // INTERNALS:
    Fetch,
    WATCH,
    MULTI,
    EXEC,
}

struct GetItemRequest {
    1: required RedisCommand cmd,
    2: optional list<string> args,
    3: optional string transactionId,
}

struct GetItemResponse {
    1: required bool ok,
    2: optional string data,
}

struct MultiGetItemResponse {
    1: required bool ok,
    1: optional list<GetItemResponse> data,
}

service ItemService {
    GetItemResponse GetItem(1: GetItemRequest req),
    MultiGetItemResponse Exec(1: GetItemRequest req),
}