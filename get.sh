#!/bin/bash

# 启动你的程序
# ./target/debug/client-cli 2>&1 &


# echo "开始测试"
# -- -s "127.0.0.1:8080"
{
    echo -n "input args: " > /dev/stderr
    read -p ''
    read -p $'get 1'
    echo -e "get 1"
    sleep 1
    read -p $'get 2'
    echo -e "get 2"
    sleep 1
} | (./target/debug/client-cli )
#| ./target/debug/client-cli