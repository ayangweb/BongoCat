#!/usr/bin/env bash
#
# 手工模拟气泡 HTTP 接口请求。
#   消息接口 /say   ：推一条气泡，可带「一次性」样式覆盖，不改保存的设置。
#   控制接口 /config：写入「持久」默认值（实时同步设置页），无参数时回读当前配置。
#
# 前置：在「设置 → AI」开启「启用 HTTP 接口」，改动端口/Token 后需重启 app。
#
# 用法：
#   [BUBBLE_PORT=7800] [BUBBLE_TOKEN=xxx] ./scripts/bubble-http.sh <命令> [参数...]
#
# 命令：
#   say [文本]                 推一条默认样式气泡（默认「你好呀~」）
#   say-styled [文本]          推一条带一次性样式覆盖的气泡（红字/大号/深色底/4s）
#   set k=v [k=v...]           /config 写默认值，例：set bgColor=#000000 bgOpacity=80 fontSize=18
#   get                        /config 回读当前配置（JSON）
#   bad                        故意越界（fontSize=999），预期 400
#   demo                       依次跑：say → set → get → say（看持久生效）→ bad
#
# 颜色里的 # 不用手动转义，脚本用 curl --data-urlencode 处理。
set -euo pipefail

PORT="${BUBBLE_PORT:-7800}"
TOKEN="${BUBBLE_TOKEN:-}"
BASE="http://127.0.0.1:${PORT}"

# req <path> [k=v ...]：GET 请求，自动 URL 编码参数并附带 token（若设置），打印响应体 + 状态码
req() {
  local path="$1"; shift
  local args=(-sS -G "${BASE}${path}" -w $'\n[HTTP %{http_code}]\n')
  local kv
  for kv in "$@"; do
    args+=(--data-urlencode "$kv")
  done
  [ -n "$TOKEN" ] && args+=(--data-urlencode "token=${TOKEN}")
  curl "${args[@]}"
}

usage() {
  sed -n '2,27p' "$0"
}

cmd="${1:-help}"
shift || true

case "$cmd" in
  say)
    req /say "text=${1:-你好呀~}"
    ;;
  say-styled)
    req /say "text=${1:-红色大字}" "textColor=#ff0000" "fontSize=28" "bgColor=#000000" "bgOpacity=70" "duration=4"
    ;;
  set)
    [ "$#" -gt 0 ] || { echo "用法：set k=v [k=v...]，例：set bgColor=#000000 bgOpacity=80" >&2; exit 2; }
    req /config "$@"
    ;;
  get)
    req /config
    ;;
  bad)
    req /say "text=x" "fontSize=999"
    ;;
  demo)
    echo "== 1. /say 默认样式 =="
    req /say "text=demo 默认"
    echo "== 2. /config 写默认值（底色黑、透明度 80、字号 18）=="
    req /config "bgColor=#000000" "bgOpacity=80" "fontSize=18"
    echo "== 3. /config 回读 =="
    req /config
    echo "== 4. /say 不带样式，应使用刚写入的新默认值 =="
    req /say "text=demo 持久生效"
    echo "== 5. 越界参数，预期 400 =="
    req /say "text=x" "fontSize=999" || true
    ;;
  help|-h|--help)
    usage
    ;;
  *)
    echo "未知命令：$cmd" >&2
    usage >&2
    exit 2
    ;;
esac
