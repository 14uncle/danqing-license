//! @author 十四叔
//! @date 2026/10/10
//!
//! 丹青产品线授权基建 (兄弟 crate; 零 UI 依赖, 照 danqing-encoding / danqing-logfile 前例)。
//!
//! 收编此前三仓同构拷贝的授权栈 (pomodoro → danqing-log → danqing-dropshelf):
//! 便携版 license key 离线 Ed25519 校验 + 商店 add-on 授权状态映射。产品侧只注入
//! 常量 (`KEY_PREFIX` / `PRODUCT` / 公钥 / Offer ID) 与自己的 `Feature` 枚举 ——
//! 本 crate **不认功能枚举**, 只回答「此刻是否付费态」。
//!
//! # 模块
//!
//! - [`key`][]: key 串解析与验签 (`Payload` / `Tier` / `KeyError` / `verify_key` / `build_payload`)
//! - [`entitlement`][]: 授权状态机与持久化 (`Entitlement` / `allows_at` / `load_from` / `activate`)
//! - `store` (feature `store`): WinRT broker 查询/购买 (不可单测区)
//!
//! # 语义红线 (三仓实测统一, 见 SPEC-danqing-license)
//!
//! - **先验签后解 JSON**: 篡改 payload 必须报 [`KeyError::BadSignature`], 不许报格式错
//! - **load 失败一律 Free**: 授权加载失败绝不 panic、绝不挡免费层
//! - **fail-closed**: 商店 `ExpirationDate` 读失败 → 不升级为付费 (修 log 反向洞)
//! - **activate 原子写**: 半截 key 文件会让重启后验签失败, 白丢一次激活
//! - **私钥永远仓库外**: crate 内不嵌任何真实公私钥 (测试一律用临时密钥对)

pub mod entitlement;
pub mod key;

#[cfg(feature = "store")]
pub mod store;

pub use entitlement::{
    ActivateError, DURABLE_THRESHOLD_EPOCH, Entitlement, PaidSource, StoreSnapshot, activate,
    load_from, map_store_snapshot,
};
pub use key::{KeyError, Payload, Tier, build_payload, mask_email, verify_key};
