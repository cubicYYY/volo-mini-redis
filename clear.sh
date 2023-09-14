#!/bin/bash
echo "删除本地所有aof文件"

# 获取当前命令行所在的目录
directory=$(pwd)

# 使用 find 命令查找当前目录中所有扩展名为 .aof 的文件并删除它们
find "$directory" -type f -name "*.aof" -exec rm {} \;

echo "删除完成"
