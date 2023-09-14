#!/bin/bash
{
    read -p $'multi'
    echo -e "multi"
    sleep 1
    read -p "transaction id:" t_id
    read -p $"set 1 123 -t $t_id"
    echo -e "set 1 123 -t $t_id"
    sleep 1
    read -p $"set 2 321 -t $t_id"
    echo -e "set 2 321  -t $t_id"
    sleep 1
    #get 2 var
    read -p $"get 2 -t $t_id"
    echo -e "get 2 -t $t_id"
    sleep 1
    read -p $"get 1 -t $t_id"
    echo -e "get 1 -t $t_id"
    sleep 1
    read -p $'exec'
    echo -e "exec"
    read 
} | ./target/debug/client-cli