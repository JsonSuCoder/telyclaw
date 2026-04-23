# Tauri 签名构建指南

## 配置签名

1. 编辑 `.env.signing` 文件，填入你的证书信息：

```bash
# 你的 Apple Developer Team ID（从证书中提取）
APPLE_TEAM_ID=你的TeamID

# 你的签名身份（从 p12 证书文件中的身份）
APPLE_SIGNING_IDENTITY="Apple Development: (LINX COMMUNICATION INC.)"

# 产品名称
PRODUCT_NAME=TelyClaw

# 证书文件路径
APPLE_CERTIFICATE_PATH=/Users/qmk/Documents/notarize.p12
APPLE_CERTIFICATE_PASSWORD=你的证书密码
```

## 构建签名版本

运行以下命令构建签名的 DMG 文件：

```bash
npm run tauri:build:signed
```

## 查看证书信息

如果需要查看证书中的签名身份，可以运行：

```bash
# 查看 p12 证书内容
openssl pkcs12 -in /Users/qmk/Documents/notarize.p12 -nokeys -info

# 查看系统钥匙串中的证书
security find-identity -v -p codesigning
```

## 注意事项

- 确保证书文件路径正确
- 证书密码要正确设置
- 构建过程中可能需要输入钥匙串密码
- 生成的 DMG 文件将在 `tauri/target/release/bundle/dmg/` 目录中