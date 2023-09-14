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
}

struct GetItemRequest {
    1: required RedisCommand cmd,
    2: optional list<string> args,
    3: optional string client_id,
}

struct GetItemResponse {
    1: required bool ok,
    2: optional string data,
}

service ItemService {
    GetItemResponse GetItem (1: GetItemRequest req),
}

