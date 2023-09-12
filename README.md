# Volo Redis

迷你redis实验作业，基于volo-thrift.  

## 已实现命令
- ping （完整支持）
- get （完整支持）
- del （完整支持，批量）
- set （支持过期时间）
- client-cli

## TODOs

- publish
- subscribe
- 布隆过滤器
- 持久化

## 备注

**WIP**

返回的请求Err/Ok更多代表数据、命令格式是否正确，而返回体中的`ok`字段更多表示操作是否成功。