# Changelog

本 crate 的显著变更。格式随 [Keep a Changelog](https://keepachangelog.com/) 精神。

## [Unreleased]

### Added

- 初始成稿(2026-10-10):从三仓同构拷贝下沉 —— `key`(Ed25519 离线校验)、
  `entitlement`(状态机 + 原子写激活 + 商店快照映射)、`store`(WinRT broker,feature 门)、
  `bin/keygen`(参数化签发 CLI,feature 门)。
- 语义裁定落地:① 商店 `ExpirationDate` 读失败 fail-closed(修 log 反向收入洞);
  ② `activate` 原子写(temp+rename);③ 商店快照映射拆出可测纯函数 `snapshot_from_readings`。
