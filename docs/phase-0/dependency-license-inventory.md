# Phase 0 Dependency License Inventory

状态：直接依赖第一轮，完整传递依赖审计待完成
日期：2026-08-28

| Dependency | Version/source    | License    | Runtime role     | Replacement boundary                    |
| ---------- | ----------------- | ---------- | ---------------- | --------------------------------------- |
| GPUI       | crates.io `0.2.2` | Apache-2.0 | Settings UI only | `bongocat-ui` command/snapshot boundary |

GPUI 的 Apache-2.0 许可证可与项目 MIT 源码并存，但发布包必须保留要求的 notices。当前表不代表 705 个已解析 package 的完整传递依赖结论；进入 Phase 1 前仍需使用 lockfile 驱动的许可证扫描，逐项处理 unknown、copyleft、binary asset 和 notice 要求。

官方 Cubism Core 不属于开源 Rust 依赖，其版本、下载来源、hash、再分发条款和 attribution 必须单独形成清单。在授权结论完成前不得制作可公开分发的 Native Rewrite 安装包。
