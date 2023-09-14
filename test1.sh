#!/bin/bash

# 启动你的程序
# ./target/debug/client-cli 2>&1 &


# echo "开始测试"
# -- -s "127.0.0.1:8080"
{
    echo -n "input args: " > /dev/stderr
    read -p ''
    read -p $'ping'
    echo -e "ping"
    sleep 1
    read -p $'ping 123 321'
    echo -e "ping 123 321"
    sleep 1
    read -p $'get 1'
    echo -e "get 1" 
    sleep 1
    read -p $'set 1 123 ex 2'
    echo -e "set 1 123 ex 2"
    sleep 1
    echo -e "get 1" > /dev/stderr
    echo -e "get 1"
    sleep 2
    read -p $'get 1'
    echo -e "get 1"
    sleep 1
    read -p $'set 1 114514'
    echo -e "set 1 114514"
    sleep 1
    read -p $'get 1'
    echo -e "get 1"
    sleep 1
    read -p $'del 1'
    echo -e "del 1"
    sleep 1
    read -p $'get 1'
    echo -e "get 1"
} | (./target/debug/client-cli )
#| ./target/debug/client-cli

