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
    SyncGot,
    WATCH,
    MULTI,
    EXEC,
}

struct GetItemRequest {
    1: required RedisCommand cmd,
    2: optional list<string> args,
    3: optional string client_id,
    4: optional string transaction_id,
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