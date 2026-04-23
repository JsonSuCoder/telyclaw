# 代码签名配置指南

## 当前配置状态

### Electron 配置 (src/electron/config.js)
- ✅ 已配置 macOS 签名和公证
- ✅ 应用 ID: `org.telyai.telyai`
- ✅ 产品名称: `TelyAI`
- ✅ 启用公证: `true`
- ✅ 强制代码签名: `true`

### Tauri 配置 (tauri/tauri.conf.json)
- ✅ 启用 hardenedRuntime
- ⚠️ 需要配置签名身份

## 配置步骤

### 1. 获取 Apple Developer 证书

1. 登录 [Apple Developer](https://developer.apple.com/)
2. 进入 "Certificates, Identifiers & Profiles"
3. 创建 "Developer ID Application" 证书
4. 下载并双击安装到钥匙串

### 2. 获取 Team ID

在 Apple Developer 账户中，Team ID 显示在右上角，格式类似：`ABC123DEFG`

### 3. 获取签名身份

在终端中运行以下命令查看可用的签名身份：
```bash
security find-identity -v -p codesigning
```

输出示例：
```
1) ABC123DEFG "Developer ID Application: Your Name (ABC123DEFG)"
```

### 4. 配置 .env.signing 文件

根据上面获取的信息，更新 `.env.signing` 文件：

```bash
# 你的 Apple Developer Team ID
APPLE_TEAM_ID=ABC123DEFG

# 完整的签名身份名称
APPLE_SIGNING_IDENTITY="Developer ID Application: Your Name (ABC123DEFG)"
```

### 5. 配置公证（可选但推荐）

如果要启用公证，需要创建 App Store Connect API Key：

1. 登录 [App Store Connect](https://appstoreconnect.apple.com/)
2. 进入 "Users and Access" > "Keys"
3. 创建新的 API Key，角色选择 "Developer"
4. 下载 `.p8` 文件
5. 更新 `.env.signing`：

```bash
APPLE_API_KEY_ID=你的Key ID
APPLE_API_ISSUER_ID=你的Issuer ID
APPLE_API_KEY_PATH=/path/to/AuthKey_KEYID.p8
```

## 使用签名构建

### Tauri 构建
```bash
# 设置环境变量
export APPLE_SIGNING_IDENTITY="Developer ID Application: Your Name (ABC123DEFG)"
export APPLE_TEAM_ID="ABC123DEFG"

# 构建
npm run tauri:build
```

### Electron 构建
Electron 配置已经设置好，只需确保环境变量正确：
```bash
# 如果有 electron 构建脚本
npm run build:electron
```

## 验证签名

构建完成后，可以验证签名：
```bash
# 验证 .app 文件
codesign -dv --verbose=4 /path/to/YourApp.app

# 验证公证状态
spctl -a -vv /path/to/YourApp.app
```

## 注意事项

1. **证书有效期**：Developer ID 证书有效期为 5 年
2. **公证时间**：公证过程可能需要几分钟到几小时
3. **网络要求**：公证需要网络连接到 Apple 服务器
4. **费用**：需要付费的 Apple Developer 账户（$99/年）

## 故障排除

### 常见错误
- `errSecInternalComponent`: 证书未正确安装
- `The specified item could not be found in the keychain`: 签名身份不正确
- `notarization failed`: API Key 配置错误或网络问题

### 解决方法
1. 重新安装证书
2. 检查签名身份名称是否完全匹配
3. 验证 API Key 权限和路径