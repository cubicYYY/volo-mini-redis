#!/bin/bash
# -- -s "127.0.0.1:8080
{
    echo -n "input args: " > /dev/stderr
    read -p ''
    read -p $'multi'
    echo -e "multi"
    sleep 1
    read -p $"get 1"
    echo -e "get 1"
    sleep 1
    read -p $"watch 1"
    echo -e "watch 1"
    sleep 1
    read -p $"set 1 114514"
    echo -e "set 1 114514"
    sleep 1
    read -p $'exec'
    echo -e "exec"
    read
} | ./target/debug/client-cli