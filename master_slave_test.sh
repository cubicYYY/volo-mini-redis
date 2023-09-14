#!/bin/bash

# 启动你的程序
# ./target/debug/client-cli 2>&1 &


# echo "开始测试"
# -- -s "127.0.0.1:8080"
{
    echo -e "input args: -- -s 127.0.0.1:8080" > /dev/stderr
    read -p ''
    read -p $'set key frommaster'
    echo -e "set key frommaster"
    sleep 1
} | (./target/debug/client-cli )

{
    echo -e "input args: -- -s 127.0.0.1:8888" > /dev/stderr
    read -p ''
    read -p $'get key slave'
    echo -e "get key"
    sleep 1
} | (./target/debug/client-cli )
