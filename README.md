# Volo Redis

迷你redis实验作业，基于volo-thrift.  

## 已实现命令
- ping （完整支持）
- get （完整支持）
- del （完整支持，批量）
- set （支持过期时间）
- client-cli
- 中间件 （过滤非ASCII可打印字符，请求计时）

## TODOs

- publish
- subscribe
- 布隆过滤器
- 持久化

## 备注

**WIP**

返回的请求Err/Ok更多代表数据、命令格式是否正确，而返回体中的`ok`字段更多表示操作是否成功。  
功能验证参考命令：  
```plaintext
ping
ping 114 514
get nope
set abc xyz
get abc
set tenmin 19260817 ex 600
get tenmin
ping 啊波测得
```