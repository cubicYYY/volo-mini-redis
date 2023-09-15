# Volo Redis

迷你redis实验作业，基于volo-thrift.  

## 用法

`cargo run --bin server` 启动服务（端口8080）
`cargo run --bin client-cli` 客户端CLI

## 已实现命令
- ping （完整支持）
- get （完整支持）
- del （完整支持，批量）
- set （支持过期时间）
- publish*
- subscribe*
- client-cli
- 中间件（过滤非ASCII可打印字符，请求计时）
- 持久化（AOF）
- gracefully shutdown（服务端等待所有客户端退出后关闭）
- 主从模式
- Cluster模式
- Bloom过滤器

## TODOs

- 更细粒度锁
- Slave只接受可信UUID
- 全量同步时利用缓冲区允许写入
- 批量删除（已实现，客户端未跟进）

## 备注

\* **阻塞操作**

返回的请求Err/Ok更多代表数据、命令格式是否正确，而返回体中的`ok`字段更多表示操作是否成功。  
功能验证参考命令（请务必先启动服务端）：  
```plaintext
# server1
ping
ping "114 514" 1919 810
get nope
set abc xyz
get abc
set tenmin 19260817 ex 600
get tenmin
ping 啊波测得
subscribe aaa
```
```shell
# server2
publish aaa abcdefg
```
主从+Cluster：  
```shell
cargo run --bin server -- -c -i 127.0.0.1 -p 8080 --name master1 # 主@8080
cargo run --bin server -- -c -i 127.0.0.1 -p 8081 --name slave1 --slaveof 127.0.0.1:8080 # 从@8081
cargo run --bin server -- -c -i 127.0.0.1 -p 8082 --name master2 # 主@8082
cargo run --bin server -- -c -i 127.0.0.1 -p 8083 --name slave2 --slaveof 127.0.0.1:8082 # 从@8083
cargo run --bin server -- -c -i 127.0.0.1 -p 8888 --name proxy #PROXY@8888
cargo run --bin proxy -- -a 127.0.0.1:8080 --masters 127.0.0.1:8080 --masters 127.0.0.1:8082 # proxy客户端， 通过命令行设置masters
```
Proxy配置（TODO）：
```shell
# cluster配置文件默认cluster.toml
cargo run --bin proxy -- --cfg cluster.toml # 挂载到proxy服务器
```
## 测试结果

![full test](statics/test.png)
![subscribe](statics/image.png)
更多测试结果详见pdf附件:)