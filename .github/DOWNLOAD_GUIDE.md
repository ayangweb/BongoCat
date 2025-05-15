# 下载指南

## 系统要求

- macOS 12 或更高版本。
- Windows 10 或更高版本。
- Linux 带有 X11 环境。

## macOS

### 手动下载

- Apple Silicon：下载 `BongoCat_aarch64.dmg`
- Intel Chip：下载 `BongoCat_x64.dmg`

### Homebrew 下载

1. 添加 BongoCat 的 tap 源：

```bash
brew tap ayangweb/BongoCat
```

2. 安装：

```bash
brew install bongo-cat
```

3. 更新：

```bash
brew upgrade bongo-cat
```

4. 卸载：

```bash
brew uninstall --cask bongo-cat

brew untap ayangweb/BongoCat
```

## Windows

- 64 位系统：下载 `BongoCat_x64.exe`
- 32 位系统：下载 `BongoCat_x86.exe`
- ARM64 架构：下载 `BongoCat_arm64.exe`

## Linux(X11)

### 手动下载

- 64 位系统：
  - Debian / Ubuntu：下载 `BongoCat_amd64.deb`
  - Fedora / RHEL：下载 `BongoCat_x86_64.rpm`
  - 通用版本：下载 `BongoCat_amd64.AppImage`
- ARM64 架构：
  - Debian / Ubuntu：下载 `BongoCat_arm64.deb`
  - Fedora / RHEL：下载 `BongoCat_aarch64.rpm`
  - 通用版本：下载 `BongoCat_aarch64.AppImage`

### AUR 下载

- Manjaro / ArchLinux: `yay -S bongo-cat`
