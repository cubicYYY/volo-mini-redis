#!/bin/bash

# 启动你的程序
# ./target/debug/client-cli 2>&1 &


# echo "开始测试"
# -- -s "127.0.0.1:8080"
{
    echo -e "input args: -- -s 127.0.0.1:8080" > /dev/stderr
    read -p ''
    read -p $'del 2'
    echo -e "del 2"
    sleep 1
} | (./target/debug/client-cli )
#| ./target/debug/client-cli