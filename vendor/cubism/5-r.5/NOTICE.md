[English](NOTICE.md) / [日本語](NOTICE.ja.md)

---

# Notices

## [Notes] Building with Visual Studio 2017 (2025-05-29)

Visual Studio 2017 provides Windows 10 SDK versions up to 10.0.17763.0 only, which may cause execution issues in some situations.
If you are building on Visual Studio 2017, please manually install and use the latest version of the Windows SDK.

## [Notes] Building Metal Renderer for Mac Catalyst (2025-05-29)

When building Metal renderer for Mac Catalyst, an issue has been confirmed in which an error ("NSURL not found") occurs with CMake versions 3.27 or higher.
If you're utilizing this platform, please use CMake version 3.26 or earlier.

## [Caution] Support for CMake 4.0 in Linux OpenGL Environment (2025-05-29)

Regarding support for CMake 4.0, the product currently uses `GLEW 2.2.0`, which does not support CMake 4.0.
Therefore, this sample is not compatible with CMake 4.0. We will update accordingly once GLEW supports it, so please be aware of this in advance.

Please refer to the issue below for details.

- [GLEW issue](https://github.com/nigels-com/glew/issues/432)

## [Caution] Regarding iOS ARM Builds (2025-05-29)

Regarding ARM builds for deployment on physical iOS devices, we have identified an issue causing installation errors on devices due to ios-cmake 4.5.0 used in Cubism SDK for Native.
We are continuing to investigate this issue.

### Workaround

1. Revert ios-cmake to version 4.4.1 or another stable version.

2. In your .xcodeproj settings, select your build target (such as "Demo") under "TARGETS", then in Build Settings > User-Defined, set the following all to YES:
   2.1. CODE_SIGNING_ALLOWED and all child settings under this key
   2.2. CODE_SIGNING_REQUIRED and all child settings under this key

## [Caution] Regarding Cubism SDK for Native Cocos2d-x Support (2024-12-19)

Support for Cocos2d-x in the Cubism SDK for Native has been discontinued starting with Cubism 5 SDK for Native R2.
For any inquiries regarding this matter, please contact us through the [contact form](https://www.live2d.jp/contact/).

## [Caution] Support for Apple's Privacy Manifest Policy

This product does not use the APIs or third-party products specified in Apple's privacy manifest policy.
This will be addressed in future updates if this product requires such support.
Please check the documentation published by Apple for details.

- [Privacy updates for App Store submissions](https://developer.apple.com/news/?id=3d8a9yyh)

## [Caution] Support for Wayland in Linux OpenGL Environment (2024-03-26)

`GLFW 3.4`, which this product uses in the Linux OpenGL environment, can now be compiled for Wayland.
However, this sample is not compatible with Wayland because `glewInit` is currently not working properly when compiling for Wayland (EGL) in `GLEW 2.2.0`.

If you are using the Linux OpenGL environment, please continue to use the X11 build as usual.

Please refer to the issue below for details.

- [GLEW issue](https://github.com/nigels-com/glew/issues/172)

## [Caution] Mac Catalyst build error for Metal (2024-01-25)

There is an error when building Mac Catalyst for Metal.
Specifically, when generating a project using `cmake3.27.0` or higher, errors such as `'Foundation/NSURLHandle.h' file not found` will occur.
We will continue to investigate the details.

### Workaround

- Use cmake with versions less than 3.27

## [Caution] Memory leak in macOS sample builds of Metal (2021-12-09)

A memory leak occurs in the macOS sample builds of Metal.
This happens within the Metal framework, so please wait for official support.
Note that we cannot answer inquiries regarding this matter.

Please check the Apple Developer Forums for more information.

- [Apple Developer Forums - Memory leak in MTLCommandBuffer?](https://developer.apple.com/forums/thread/120931)

## Cubism Core

### [Caution] About using `.bundle` and `.dylib` on macOS Catalina or later

To use `.bundle` and `.dylib` on macOS Catalina, you need to be connected to an online environment to verify your notarization.

Please check the official Apple documentation for details.

- [Apple Official Documentation](https://developer.apple.com/documentation/security/notarizing_your_app_before_distribution)

---

©Live2D
