# Volo Redis

迷你redis实验作业，基于volo-thrift.  

## 已实现命令
- ping （完整支持）
- get （完整支持）
- del （支持批量）
- set （部分支持过期时间）

## TODOs

- publish
- subscribe
- client-cli
- exit
- 布隆过滤器
- 持久化

## 备注

**WIP**

返回的请求Err/Ok更多代表数据、命令格式是否正确，而返回体中的`ok`字段更多表示操作是否成功。