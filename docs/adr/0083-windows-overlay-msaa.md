# ADR-0083: Windows 悬浮窗使用多重采样降低模型边缘锯齿

状态：Accepted
日期：2026-10-06

## 背景

旧版 WebGL 模型画布使用浏览器的抗锯齿路径。Rust 重写后的 Windows
D3D11 overlay 直接把 drawable 绘制到单采样的 DirectComposition 交换链，
三角形轮廓和透明遮罩边缘因此出现明显锯齿，尤其是缩放显示旧 MVer 模型时。

## 决策

Windows renderer 使用 4x multisample 的 `B8G8R8A8_UNORM` 内部 render target，
包括用于 clipping mask 的中间 target。每个 mask 在被 drawable shader 采样前先
通过 `ResolveSubresource` 写入单采样纹理；DirectComposition flip-model 交换链
继续保持单采样，每帧绘制完成后再 resolve 到交换链，然后执行 readback 或 present。
内部 target 使用 `TEXTURE2DMS` RTV，交换链、mask shader resource 和 staging
texture 仍使用现有单采样格式与 premultiplied-alpha contract。模型资源、配置和
跨平台 renderer API 不变。

## 验证

自动化验证覆盖采样数契约、资源格式、格式化和 `bongocat-overlay` 测试；
Windows x64 target 的交叉编译检查用于验证 D3D11 resource/view 类型。Windows
实机截图和同一模型的前后像素对照仍需在目标硬件上完成。
