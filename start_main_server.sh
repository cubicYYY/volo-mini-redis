#!/bin/bash

# 启动你的程序
# ./target/debug/client-cli 2>&1 &


# echo "开始测试"
# -- -i 127.0.0.1 -p 8080 -n main_server
{
    echo -e "input args: -- -i 127.0.0.1 -p 8080 -n main_server" > /dev/stderr
    read -p ''
    read -p 'main_server running'
} |( ./target/debug/server)
#| ./target/debug/client-cli