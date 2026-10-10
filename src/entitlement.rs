//! @author 十四叔
//! @date 2026/10/10
//!
//! 授权状态机与持久化 (成稿来源: danqing-log v1.x `license.rs`, 参数化下沉)。
//!
//! **Feature 无感化**: 本 crate 不认功能枚举, 只回答「此刻是否付费态」
//! ([`Entitlement::allows_at`])。产品侧保留自己的 `Feature` 枚举与门控点位,
//! 用一行适配委托过来。
//!
//! 商店侧的 WinRT 互操作在 `store` 模块 (feature 门, 不可单测); 可测的
//! 「读数 → 快照 → 授权状态」全部收在本模块的纯函数里。

use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU32, Ordering};
use std::time::Duration;

use crate::key::{KeyError, Payload, verify_key};

/// 付费来源。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PaidSource {
    /// 便携版: license key 激活。携带载荷供许可区展示 (邮箱尾缀遮蔽等)。
    License(Payload),
    /// 商店版: add-on 内购。
    StoreAddOn,
}

/// 授权状态。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Entitlement {
    /// 免费层。
    Free,
    /// 商店 trial。`expires_epoch` 来自 broker 返回值 —— 时长是 Partner Center
    /// 后台配置, 代码不写死。
    Trial {
        /// 过期 epoch 秒。
        expires_epoch: u64,
    },
    /// 付费。
    Paid {
        /// 来源。
        source: PaidSource,
    },
}

impl Entitlement {
    /// 门控查询: Paid 全开; Trial 未过期开; Free 全关。
    ///
    /// 三条付费腿同一规则, 且**有效性口径单点在此** —— 快照映射只产出
    /// `Trial { expires_epoch }`, 过没过期一律由这里回答。
    pub fn allows_at(&self, now_epoch: u64) -> bool {
        match self {
            Entitlement::Free => false,
            Entitlement::Paid { .. } => true,
            Entitlement::Trial { expires_epoch } => now_epoch < *expires_epoch,
        }
    }
}

/// 激活失败的分态: key 本身的问题 vs 落盘失败 (会话内已生效但重启会丢)。
#[derive(Debug)]
pub enum ActivateError {
    /// key 校验失败。
    Key(KeyError),
    /// 落盘失败。
    Persist(String),
}

/// 从指定路径加载授权。文件不存在 = 免费 (正常路径, 首次启动); 内容坏 →
/// Free + warn, 不崩不 panic —— **授权加载失败绝不能挡住免费层**。
pub fn load_from(path: &Path, prefix: &str, product: &str, pubkey: &[u8; 32]) -> Entitlement {
    let Ok(text) = fs::read_to_string(path) else {
        return Entitlement::Free;
    };
    match verify_key(text.trim(), prefix, product, pubkey) {
        Ok(payload) => Entitlement::Paid {
            source: PaidSource::License(payload),
        },
        Err(e) => {
            log::warn!(
                "license 文件 {} 校验失败 ({e:?}), 按免费层启动",
                path.display()
            );
            Entitlement::Free
        }
    }
}

/// 激活: 校验通过 → **原子落盘** → 返回载荷。即时生效由调用方更新自身状态。
///
/// 落盘走本 crate 自含的 temp+rename (`atomic_save`) —— 兄弟 crate 零 UI 依赖,
/// 不引 danqing 的 `persist`; 半截 key 文件会让重启后验签失败, 白丢一次激活。
/// 父目录不存在时先建 (全新机器上用户直接贴 key 不能让激活栽在目录上)。
pub fn activate(
    path: &Path,
    key: &str,
    prefix: &str,
    product: &str,
    pubkey: &[u8; 32],
) -> Result<Payload, ActivateError> {
    let payload = verify_key(key, prefix, product, pubkey).map_err(ActivateError::Key)?;
    atomic_save(path, format!("{}\n", key.trim()).as_bytes())
        .map_err(|e| ActivateError::Persist(format!("{}: {e}", path.display())))?;
    Ok(payload)
}

/// 崩溃安全原子写入: 写唯一名临时文件 → rename 覆盖目标。
///
/// 与框架 `persist::atomic_save` 同形 (兄弟 crate 零 UI 依赖, 不引 danqing 的
/// `persist`): 临时名 `<文件名>.tmp.<pid>.<序号>` —— pid 防跨进程互踩, 序号防
/// 同进程多线程互踩 (红队 F2); 成功后清扫同前缀陈旧残渣 (红队 F1: pid 后缀失去
/// 「下次同名覆盖」自愈, 不扫会永久累积; 时龄阈值保护并行实例的在飞 tmp);
/// rename 失败清掉自己的半截 tmp。
fn atomic_save(path: &Path, data: &[u8]) -> io::Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let tmp = tmp_name_for(path);
    fs::write(&tmp, data)?;
    if let Err(e) = fs::rename(&tmp, path) {
        let _ = fs::remove_file(&tmp);
        return Err(e);
    }
    sweep_stale_tmp(path, STALE_TMP_MAX_AGE);
    Ok(())
}

/// 临时文件序号 (进程内单调递增)。
static TMP_SEQ: AtomicU32 = AtomicU32::new(0);

/// tmp 文件超过此时龄视为崩溃残渣 (在飞 tmp 生命期是毫秒级)。
const STALE_TMP_MAX_AGE: Duration = Duration::from_secs(3600);

/// 本调用的唯一临时文件名 (追加式; 不用 `with_extension`, 同 stem 不同扩展名
/// 会撞名)。
fn tmp_name_for(path: &Path) -> PathBuf {
    let name = path
        .file_name()
        .map(|n| n.to_string_lossy())
        .unwrap_or_default();
    path.with_file_name(format!(
        "{name}.tmp.{}.{}",
        std::process::id(),
        TMP_SEQ.fetch_add(1, Ordering::Relaxed)
    ))
}

/// 崩溃残渣清扫: 每次成功落盘后顺手清同前缀陈旧残渣。best-effort,
/// 任何一步失败都忽略, 绝不影响主链。
fn sweep_stale_tmp(path: &Path, max_age: Duration) {
    let (Some(dir), Some(name)) = (path.parent(), path.file_name()) else {
        return;
    };
    let prefix = format!("{}.tmp.", name.to_string_lossy());
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let fname = entry.file_name().to_string_lossy().into_owned();
        if !fname.starts_with(&prefix) {
            continue;
        }
        let stale = entry
            .metadata()
            .and_then(|m| m.modified())
            .map(|t| t.elapsed().unwrap_or_default() >= max_age)
            .unwrap_or(false);
        if stale {
            let _ = fs::remove_file(entry.path());
        }
    }
}

// ─── 商店轨: 读数 → 快照 → 授权状态 (纯函数区; WinRT 在 store 模块) ──────

/// 商店授权快照 —— broker 查询结果的纯数据形态。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StoreSnapshot {
    /// 我们的 add-on 在许可证列表里且 `IsActive`。
    /// (pomodoro 实测坑: `StoreAppLicense::IsActive` 是**应用级**许可证,
    /// 免费上架时所有安装者都为 true —— 必须遍历 add-on, 不许简化。)
    pub addon_active: bool,
    /// add-on 许可证的过期时间 (epoch 秒)。durable 买断 add-on 不过期,
    /// 微软对不过期写法的实测是 DateTime 最大值 (≈ 公元 9999 年)。
    pub addon_expires_epoch: Option<u64>,
}

/// 过期时间达到此阈值 (≈ 公元 9900 年) 视为 durable 买断, 否则是 trial。
pub const DURABLE_THRESHOLD_EPOCH: u64 = 250_000_000_000;

/// broker 读数 → 快照 (**裁定 1 落点**: `ExpirationDate` 读失败一律 fail-closed)。
///
/// `expires_epoch` 取失败 (`Err`) = 过期时间未知 → **整体免费态**, 不许把
/// 「查不到」当成「永不到期」—— 那会把试用升成永久买断 (log 版的反向收入洞,
/// 本 crate 取 dropshelf 的 fail-closed)。
///
/// 纯函数, 可测; WinRT 侧只需把 `ExpirationDate()` 的成败翻成这个 `Result`。
pub fn snapshot_from_readings(addon_active: bool, expires_epoch: Result<u64, ()>) -> StoreSnapshot {
    match expires_epoch {
        Ok(e) => StoreSnapshot {
            addon_active,
            addon_expires_epoch: Some(e),
        },
        Err(()) => StoreSnapshot {
            addon_active: false,
            addon_expires_epoch: None,
        },
    }
}

/// 快照 → 授权状态 (纯函数)。trial 过期与否**不在这里判** —— 映射只产出
/// `Trial { expires_epoch }`, 有效性由 [`Entitlement::allows_at`] 统一回答。
pub fn map_store_snapshot(snap: &StoreSnapshot) -> Entitlement {
    if !snap.addon_active {
        return Entitlement::Free;
    }
    match snap.addon_expires_epoch {
        Some(e) if e < DURABLE_THRESHOLD_EPOCH => Entitlement::Trial { expires_epoch: e },
        // 不过期 (None) 或 ≈ DateTime 最大值 = durable 买断
        _ => Entitlement::Paid {
            source: PaidSource::StoreAddOn,
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::key::{Tier, build_payload};
    use base64::Engine;
    use base64::engine::general_purpose::URL_SAFE_NO_PAD;
    use ed25519_dalek::{Signer, SigningKey};

    const PREFIX: &str = "testapp1";
    const PRODUCT: &str = "danqing-test";
    const TEST_SECRET: [u8; 32] = [7u8; 32];

    fn test_pubkey() -> [u8; 32] {
        SigningKey::from_bytes(&TEST_SECRET)
            .verifying_key()
            .to_bytes()
    }

    fn sign_with(payload_json: &str, sk: &SigningKey) -> String {
        let sig = sk.sign(payload_json.as_bytes());
        format!(
            "{PREFIX}.{}.{}",
            URL_SAFE_NO_PAD.encode(payload_json.as_bytes()),
            URL_SAFE_NO_PAD.encode(sig.to_bytes())
        )
    }

    fn sign(payload_json: &str) -> String {
        sign_with(payload_json, &SigningKey::from_bytes(&TEST_SECRET))
    }

    fn good_key() -> String {
        sign(&build_payload(
            PRODUCT,
            Tier::Personal,
            "a@b.c",
            "1700000000",
            "n1",
            None,
        ))
    }

    /// 临时 license 路径 (不碰用户真文件; 文件名带 pid + 用例名防并行 flake)。
    fn temp_path(tag: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("danqing-license-test-{}", std::process::id()));
        dir.join(format!("{tag}.key"))
    }

    fn cleanup(p: &std::path::Path) {
        let _ = fs::remove_file(p);
        // tmp 名带 pid+序号不可预报 —— 按前缀清同目录残渣。
        if let Some(dir) = p.parent() {
            let prefix = format!("{}.tmp.", p.file_name().unwrap().to_string_lossy());
            if let Ok(entries) = fs::read_dir(dir) {
                for e in entries.flatten() {
                    if e.file_name().to_string_lossy().starts_with(&prefix) {
                        let _ = fs::remove_file(e.path());
                    }
                }
            }
            let _ = fs::remove_dir(dir);
        }
    }

    // ── 状态机 ──

    #[test]
    fn free_allows_nothing() {
        assert!(!Entitlement::Free.allows_at(1_700_000_000));
    }

    #[test]
    fn paid_allows_everything() {
        let e = Entitlement::Paid {
            source: PaidSource::StoreAddOn,
        };
        assert!(e.allows_at(1_700_000_000));
        // 携 License 来源同样全开
        let p = Entitlement::Paid {
            source: PaidSource::License(Payload {
                tier: Tier::Enterprise,
                email: "a@b.c".into(),
                issued_at: "1".into(),
                nonce: "n".into(),
                expires: None,
            }),
        };
        assert!(p.allows_at(0));
    }

    #[test]
    fn trial_allows_only_before_expiry() {
        let e = Entitlement::Trial {
            expires_epoch: 1_700_000_000,
        };
        assert!(e.allows_at(1_699_999_999));
        assert!(!e.allows_at(1_700_000_000));
        assert!(!e.allows_at(1_700_000_001));
    }

    // ── 加载 ──

    #[test]
    fn load_missing_file_is_free() {
        let p = temp_path("missing");
        let _ = fs::remove_file(&p);
        assert_eq!(
            load_from(&p, PREFIX, PRODUCT, &test_pubkey()),
            Entitlement::Free
        );
    }

    #[test]
    fn load_garbage_file_is_free_not_panic() {
        let p = temp_path("garbage");
        fs::create_dir_all(p.parent().unwrap()).unwrap();
        fs::write(&p, "这不是一个 key\n").unwrap();
        assert_eq!(
            load_from(&p, PREFIX, PRODUCT, &test_pubkey()),
            Entitlement::Free
        );
        cleanup(&p);
    }

    #[test]
    fn load_wrong_product_key_is_free() {
        let p = temp_path("wrongproduct");
        fs::create_dir_all(p.parent().unwrap()).unwrap();
        let other = sign(&build_payload(
            "danqing-other",
            Tier::Personal,
            "a@b.c",
            "1",
            "n",
            None,
        ));
        fs::write(&p, &other).unwrap();
        assert_eq!(
            load_from(&p, PREFIX, PRODUCT, &test_pubkey()),
            Entitlement::Free
        );
        cleanup(&p);
    }

    // ── 激活 ──

    #[test]
    fn activate_persists_and_load_roundtrips() {
        let path = temp_path("roundtrip");
        let payload = activate(&path, &good_key(), PREFIX, PRODUCT, &test_pubkey())
            .expect("有效 key 激活必须成");
        assert_eq!(payload.email, "a@b.c");
        assert_eq!(
            load_from(&path, PREFIX, PRODUCT, &test_pubkey()),
            Entitlement::Paid {
                source: PaidSource::License(payload)
            }
        );
        cleanup(&path);
    }

    #[test]
    fn activate_creates_missing_parent_dirs() {
        // 全新机器上连配置目录都没有 —— 激活不能栽在这上面。
        let path = temp_path("nested").join("deeper").join("license.key");
        activate(&path, &good_key(), PREFIX, PRODUCT, &test_pubkey()).expect("应自建父目录");
        assert!(path.exists());
        let _ = fs::remove_dir_all(temp_path("nested"));
    }

    #[test]
    fn activate_rejects_bad_key_and_writes_nothing() {
        let path = temp_path("reject");
        let _ = fs::remove_file(&path);
        let foreign = SigningKey::from_bytes(&[9u8; 32]);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        let err = activate(
            &path,
            &sign_with(
                &build_payload(PRODUCT, Tier::Personal, "a@b.c", "1", "n", None),
                &foreign,
            ),
            PREFIX,
            PRODUCT,
            &test_pubkey(),
        )
        .unwrap_err();
        assert!(matches!(err, ActivateError::Key(KeyError::BadSignature)));
        assert!(!path.exists(), "坏 key 不许落盘");
        cleanup(&path);
    }

    #[test]
    fn activate_is_atomic_no_tmp_left_behind() {
        // 裁定 2: 落盘走 temp+rename —— 成功后目录里不许留半截 tmp。
        let path = temp_path("atomic");
        activate(&path, &good_key(), PREFIX, PRODUCT, &test_pubkey()).unwrap();
        let dir = path.parent().unwrap();
        let leftovers: Vec<_> = fs::read_dir(dir)
            .unwrap()
            .filter_map(Result::ok)
            .map(|e| e.file_name().to_string_lossy().into_owned())
            .filter(|n| n.contains(".tmp"))
            .collect();
        assert!(leftovers.is_empty(), "残留 tmp: {leftovers:?}");
        cleanup(&path);
    }

    #[test]
    fn atomic_save_sweeps_stale_residue_keeps_fresh() {
        // 与框架 persist 同形 (红队 F1/F9): 陈旧残渣清扫回收, 新鲜在飞不动。
        let path = temp_path("sweep");
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        let foreign = path.parent().unwrap().join("sweep.key.tmp.999999.0");
        fs::write(&foreign, b"stale").unwrap();
        sweep_stale_tmp(&path, Duration::ZERO);
        assert!(!foreign.exists(), "陈旧残渣没被扫掉");
        fs::write(&foreign, b"fresh").unwrap();
        sweep_stale_tmp(&path, STALE_TMP_MAX_AGE);
        assert!(foreign.exists(), "新鲜在飞 tmp 被误清");
        let _ = fs::remove_file(&foreign);
        cleanup(&path);
    }

    // ── 商店快照映射 (四态 + fail-closed) ──

    #[test]
    fn store_snapshot_without_addon_is_free() {
        let snap = StoreSnapshot {
            addon_active: false,
            addon_expires_epoch: None,
        };
        assert_eq!(map_store_snapshot(&snap), Entitlement::Free);
    }

    #[test]
    fn store_snapshot_durable_addon_is_paid() {
        // durable 买断: 不过期 (None) 或过期时间 ≈ DateTime 最大值
        for expires in [None, Some(253_402_300_799)] {
            let snap = StoreSnapshot {
                addon_active: true,
                addon_expires_epoch: expires,
            };
            assert_eq!(
                map_store_snapshot(&snap),
                Entitlement::Paid {
                    source: PaidSource::StoreAddOn
                }
            );
        }
    }

    #[test]
    fn store_snapshot_finite_expiry_is_trial() {
        let snap = StoreSnapshot {
            addon_active: true,
            addon_expires_epoch: Some(1_800_000_000), // 有限 = trial
        };
        let e = map_store_snapshot(&snap);
        assert_eq!(
            e,
            Entitlement::Trial {
                expires_epoch: 1_800_000_000
            }
        );
        assert!(e.allows_at(1_799_999_999));
        assert!(!e.allows_at(1_800_000_000));
    }

    #[test]
    fn expiration_read_failure_is_fail_closed_to_free() {
        // 裁定 1 的红线锁: ExpirationDate 读失败**绝不能**被当成 durable 买断
        // (那会把试用升成永久 = 收入洞)。即使 add-on IsActive 为真, 也压成免费态。
        let snap = snapshot_from_readings(true, Err(()));
        assert!(!snap.addon_active, "读失败必须 fail-closed");
        assert_eq!(snap.addon_expires_epoch, None);
        assert_eq!(map_store_snapshot(&snap), Entitlement::Free);
    }

    #[test]
    fn expiration_read_success_maps_through() {
        let snap = snapshot_from_readings(true, Ok(1_800_000_000));
        assert_eq!(
            map_store_snapshot(&snap),
            Entitlement::Trial {
                expires_epoch: 1_800_000_000
            }
        );
    }
}
