#!/bin/bash

# 加载签名环境变量
if [ -f .env.signing ]; then
    source .env.signing
    echo "✅ 已加载签名配置"
else
    echo "❌ 未找到 .env.signing 文件"
    exit 1
fi

# 检查必要的环境变量
if [ -z "$APPLE_CERTIFICATE_PATH" ]; then
    echo "❌ APPLE_CERTIFICATE_PATH 未设置"
    exit 1
fi

if [ ! -f "$APPLE_CERTIFICATE_PATH" ]; then
    echo "❌ 证书文件不存在: $APPLE_CERTIFICATE_PATH"
    exit 1
fi

echo "🔐 使用证书: $APPLE_CERTIFICATE_PATH"

# 导入证书到临时钥匙串（如果需要）
if [ -n "$APPLE_CERTIFICATE_PASSWORD" ]; then
    echo "🔑 导入证书到钥匙串..."
    security import "$APPLE_CERTIFICATE_PATH" -P "$APPLE_CERTIFICATE_PASSWORD" -A
fi

# 设置 Tauri 签名环境变量
export TAURI_SIGNING_PRIVATE_KEY_PATH="$APPLE_CERTIFICATE_PATH"
export TAURI_SIGNING_PRIVATE_KEY_PASSWORD="$APPLE_CERTIFICATE_PASSWORD"

# 如果设置了签名身份，使用它
if [ -n "$APPLE_SIGNING_IDENTITY" ]; then
    export TAURI_BUNDLE_MACOS_SIGNING_IDENTITY="$APPLE_SIGNING_IDENTITY"
fi

echo "🚀 开始构建签名版本..."

# 运行构建
npm run build:production && \
npm run build:openclaw && \
cross-env NODE_ENV=production BASE_URL=tauri://localhost tauri build \
  --config '{"build":{"frontendDist":"../dist"}}' \
  -- --verbose

echo "✅ 构建完成！"