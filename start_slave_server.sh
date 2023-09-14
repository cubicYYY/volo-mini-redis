#!/bin/bash

# 启动你的程序
# ./target/debug/client-cli 2>&1 &


# echo "开始测试"
# -- -i 127.0.0.1 -p 8888 -n slave_server
{
    echo -e "input args: -- -i 127.0.0.1 -p 8888 -n slave_server" > /dev/stderr
    read -p ''
    read -p 'slave_server running'
} | (./target/debug/client-cli )
#| ./target/debug/client-cli