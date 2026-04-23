# macOS 应用分发指南

## 问题说明
在 macOS 上，未签名的应用会被系统标记为"已损坏"，这是 macOS 的安全机制。

## 解决方案

### 方案1：用户端解决（临时方案）
告诉用户使用以下方法之一：

#### 方法1：右键打开
1. 右键点击 DMG 文件
2. 选择"打开"
3. 在弹出的对话框中点击"打开"

#### 方法2：移除隔离属性
在终端中运行：
```bash
# 对 DMG 文件
sudo xattr -rd com.apple.quarantine /path/to/your-app.dmg

# 或者对解压后的 .app 文件
sudo xattr -rd com.apple.quarantine /Applications/YourApp.app
```

#### 方法3：系统设置
1. 打开"系统偏好设置" > "安全性与隐私"
2. 在"通用"标签页中，选择"任何来源"（可能需要先点击左下角的锁图标）

### 方案2：开发者端解决（推荐）
如果你有 Apple Developer 账户，可以对应用进行签名：

#### 1. 获取证书
- 登录 Apple Developer 网站
- 创建 "Developer ID Application" 证书
- 下载并安装到钥匙串

#### 2. 配置签名
编辑 `.env.signing` 文件：
```bash
APPLE_TEAM_ID=YOUR_TEAM_ID
APPLE_SIGNING_IDENTITY="Developer ID Application: Your Name (YOUR_TEAM_ID)"
```

#### 3. 使用签名打包
```bash
# 设置环境变量后打包
export APPLE_SIGNING_IDENTITY="Developer ID Application: Your Name (YOUR_TEAM_ID)"
npm run tauri:build
```

### 方案3：公证（最佳方案）
对于公开分发，建议进行公证：

1. 完成代码签名
2. 配置 App Store Connect API Key
3. 启用公证功能

## 注意事项
- 代码签名需要付费的 Apple Developer 账户（$99/年）
- 公证需要额外的配置和时间
- 对于内部使用，方案1通常就足够了