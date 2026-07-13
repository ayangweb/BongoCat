#!/usr/bin/env bash
# BongoCat Bark 客户端手动验证：向 htnanako/bark-server 推送明文/加密消息
#
# 用法:
#   ./scripts/bark-manual.sh <server> <device_key>                 # 明文
#   ./scripts/bark-manual.sh <server> <device_key> <key16> <iv16>  # 追加 AES-128-CBC 加密消息
#
# device_key 在 BongoCat 设置页 Chat → Bark 推送里注册后显示。
set -euo pipefail

SERVER=${1:?usage: bark-manual.sh <server> <device_key> [aes128key(16char)] [iv(16char)]}
DEVICE_KEY=${2:?missing device_key}

echo "--- 明文消息 ---"
curl -fsS "$SERVER/$DEVICE_KEY/测试标题/来自 bark-manual 的正文" && echo " <- ok（气泡应显示：测试标题 换行 正文）"

if [[ $# -ge 4 ]]; then
  KEY=$3
  IV=$4
  [[ ${#KEY} -eq 16 && ${#IV} -eq 16 ]] || { echo "key/iv 必须都是 16 字符（AES-128-CBC）"; exit 1; }

  echo "--- 加密消息（AES-128-CBC）---"
  PLAIN='{"title":"加密标题","body":"加密正文"}'
  CIPHER=$(printf %s "$PLAIN" | openssl enc -aes-128-cbc -K "$(printf %s "$KEY" | xxd -p)" -iv "$(printf %s "$IV" | xxd -p)" | base64)
  curl -fsS -G "$SERVER/$DEVICE_KEY" --data-urlencode "ciphertext=$CIPHER" --data-urlencode "iv=$IV" && echo " <- ok（需在设置页配置相同的 CBC 密钥/IV）"
fi
