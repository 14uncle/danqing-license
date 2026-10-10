//! @author 十四叔
//! @date 2026/10/10
//!
//! license key 的解析与校验 (成稿来源: danqing-log v1.x `license.rs`, 参数化下沉)。
//!
//! 只做离线校验, 零网络请求。公钥由调用方注入 (产品常量), 私钥永远仓库外。

use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use ed25519_dalek::{Signature, VerifyingKey};

/// 当前 payload 版本。格式演进时旧版本 key 必须明确拒绝, 不猜着解。
const PAYLOAD_VERSION: u64 = 1;

/// 合法 key 长度上限。合法 key 约 300–400 字节, 4 KiB 有 10 倍余量 ——
/// 超长输入直接格式错, 不进 base64/验签 (超大粘贴的线性扫描自残面)。
const MAX_KEY_LEN: usize = 4096;

/// 付费档位 (同一 key 格式; seat 数不技术强制, 君子协定写明)。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tier {
    /// 个人。
    Personal,
    /// 企业。
    Enterprise,
}

impl Tier {
    /// wire 形态的档位名。
    fn as_str(self) -> &'static str {
        match self {
            Tier::Personal => "personal",
            Tier::Enterprise => "enterprise",
        }
    }

    fn parse(s: &str) -> Option<Tier> {
        match s {
            "personal" => Some(Tier::Personal),
            "enterprise" => Some(Tier::Enterprise),
            _ => None,
        }
    }
}

/// 校验通过的 key 载荷。
#[derive(Clone, PartialEq, Eq)]
pub struct Payload {
    /// 档位。
    pub tier: Tier,
    /// 购买邮箱。
    pub email: String,
    /// 签发时间 (epoch 秒, 字符串 —— 与既有 wire 格式一致)。
    pub issued_at: String,
    /// 随机串 (hex)。
    pub nonce: String,
    /// 预留字段: 买断制当前永不过期, **解析但校验侧不执行** (将来策略变化时旧 key
    /// 仍有表达能力; 别以为签个带 expires 的 key 就会自动到期)。
    pub expires: Option<String>,
}

/// **手写 Debug**: 邮箱走遮蔽 —— derive 会把完整邮箱留在格式化输出里,
/// 将来任何一行 `log::info!("{entitlement:?}")` 都会把它写进日志文件。
impl std::fmt::Debug for Payload {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Payload")
            .field("tier", &self.tier)
            .field("email", &mask_email(&self.email))
            .field("issued_at", &self.issued_at)
            .field("nonce", &self.nonce)
            .field("expires", &self.expires)
            .finish()
    }
}

/// key 校验失败的三态 (产品按态分文案)。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KeyError {
    /// 格式错: 前缀不对 / 不是三段 / base64 或 JSON 解不出 / 必填字段缺失 / 版本未知。
    Malformed,
    /// 签名错: 格式都对但签名验不过 (篡改 / 别的密钥签的 / 公钥占位未回填)。
    BadSignature,
    /// 产品不匹配: 签名有效, 但 payload 的 product 不是本应用。
    WrongProduct,
}

/// 解析并校验 key: `"<prefix>.<payload_b64url>.<sig_b64url>"`。
///
/// 顺序固定: **先验签 (对原始 payload 字节) 再解 JSON** —— 篡改 payload 必须报
/// [`KeyError::BadSignature`], 不能报成格式错。长度闸见 [`MAX_KEY_LEN`]。
///
/// `prefix` / `product` / `pubkey` 由调用方注入 (产品常量)。
pub fn verify_key(
    key: &str,
    prefix: &str,
    product: &str,
    pubkey: &[u8; 32],
) -> Result<Payload, KeyError> {
    let trimmed = key.trim();
    if trimmed.len() > MAX_KEY_LEN {
        return Err(KeyError::Malformed);
    }
    let mut parts = trimmed.split('.');
    let (Some(got_prefix), Some(payload_b64), Some(sig_b64), None) =
        (parts.next(), parts.next(), parts.next(), parts.next())
    else {
        return Err(KeyError::Malformed);
    };
    if got_prefix != prefix {
        return Err(KeyError::Malformed);
    }
    let payload_bytes = URL_SAFE_NO_PAD
        .decode(payload_b64)
        .map_err(|_| KeyError::Malformed)?;
    let sig_bytes: [u8; 64] = URL_SAFE_NO_PAD
        .decode(sig_b64)
        .ok()
        .and_then(|v| v.try_into().ok())
        .ok_or(KeyError::Malformed)?;

    // 先验签
    let vk = VerifyingKey::from_bytes(pubkey).map_err(|_| KeyError::BadSignature)?;
    let sig = Signature::from_bytes(&sig_bytes);
    vk.verify_strict(&payload_bytes, &sig)
        .map_err(|_| KeyError::BadSignature)?;

    // 验签通过才解 JSON
    let v: serde_json::Value =
        serde_json::from_slice(&payload_bytes).map_err(|_| KeyError::Malformed)?;
    let obj = v.as_object().ok_or(KeyError::Malformed)?;
    if obj.get("v").and_then(|x| x.as_u64()) != Some(PAYLOAD_VERSION) {
        return Err(KeyError::Malformed);
    }
    match obj.get("product").and_then(|x| x.as_str()) {
        None => return Err(KeyError::Malformed),
        Some(p) if p != product => return Err(KeyError::WrongProduct),
        _ => {}
    }
    let tier = obj
        .get("tier")
        .and_then(|x| x.as_str())
        .and_then(Tier::parse)
        .ok_or(KeyError::Malformed)?;
    let required_str = |name: &str| -> Result<String, KeyError> {
        obj.get(name)
            .and_then(|x| x.as_str())
            .filter(|s| !s.is_empty())
            .map(str::to_owned)
            .ok_or(KeyError::Malformed)
    };
    Ok(Payload {
        tier,
        email: required_str("email")?,
        issued_at: required_str("issued_at")?,
        nonce: required_str("nonce")?,
        expires: obj
            .get("expires")
            .and_then(|x| x.as_str())
            .map(str::to_owned),
    })
}

/// 构造 payload JSON 串 —— keygen (`src/bin/keygen.rs`) 的唯一签发入口。
/// 字段名集合与 [`verify_key`] 的解析是同一真身: 两边不许各写一份。
pub fn build_payload(
    product: &str,
    tier: Tier,
    email: &str,
    issued_at: &str,
    nonce: &str,
    expires: Option<&str>,
) -> String {
    let mut v = serde_json::json!({
        "v": PAYLOAD_VERSION,
        "product": product,
        "tier": tier.as_str(),
        "email": email,
        "issued_at": issued_at,
        "nonce": nonce,
    });
    if let Some(e) = expires {
        v["expires"] = serde_json::Value::String(e.to_owned());
    }
    v.to_string()
}

/// 邮箱遮蔽显示: `alice@example.com` → `a***@example.com`。
/// 本地段只留首字符; 没有 `@` 或本地段为空的畸形输入整体遮蔽为 `***`, 不猜。
pub fn mask_email(email: &str) -> String {
    match email.split_once('@') {
        Some((local, domain)) if !local.is_empty() => {
            let first = local.chars().next().expect("已判非空");
            format!("{first}***@{domain}")
        }
        _ => "***".to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ed25519_dalek::{Signer, SigningKey};

    // ── 测试常量 (产品注入形态; crate 内不嵌任何真实密钥) ──
    const PREFIX: &str = "testapp1";
    const PRODUCT: &str = "danqing-test";

    /// 测试专用私钥。入库无害: 产品公钥与它无关, 它签的 key 在真公钥下必败
    /// (有测试守着这一条)。
    const TEST_SECRET: [u8; 32] = [7u8; 32];

    fn test_pubkey() -> [u8; 32] {
        SigningKey::from_bytes(&TEST_SECRET)
            .verifying_key()
            .to_bytes()
    }

    /// 用指定私钥签一个 payload JSON 串, 拼出完整 key。
    fn sign_with(payload_json: &str, sk: &SigningKey) -> String {
        let sig = sk.sign(payload_json.as_bytes());
        format!(
            "{PREFIX}.{}.{}",
            URL_SAFE_NO_PAD.encode(payload_json.as_bytes()),
            URL_SAFE_NO_PAD.encode(sig.to_bytes())
        )
    }

    /// 默认用测试私钥签。
    fn sign(payload_json: &str) -> String {
        sign_with(payload_json, &SigningKey::from_bytes(&TEST_SECRET))
    }

    const GOOD: &str = r#"{"v":1,"product":"danqing-test","tier":"personal","email":"a@b.c","issued_at":"2026-09-19","nonce":"n1"}"#;

    #[test]
    fn valid_personal_key_verifies() {
        let p = verify_key(&sign(GOOD), PREFIX, PRODUCT, &test_pubkey()).expect("有效 key 必须过");
        assert_eq!(p.tier, Tier::Personal);
        assert_eq!(p.email, "a@b.c");
        assert_eq!(p.issued_at, "2026-09-19");
        assert_eq!(p.nonce, "n1");
        assert_eq!(p.expires, None);
    }

    #[test]
    fn valid_enterprise_key_verifies() {
        let key = sign(&GOOD.replace("\"personal\"", "\"enterprise\""));
        assert_eq!(
            verify_key(&key, PREFIX, PRODUCT, &test_pubkey())
                .expect("有效 key 必须过")
                .tier,
            Tier::Enterprise
        );
    }

    #[test]
    fn expires_field_roundtrips() {
        let payload = GOOD.replace(
            "\"nonce\":\"n1\"",
            "\"nonce\":\"n1\",\"expires\":\"2027-01-01\"",
        );
        let p = verify_key(&sign(&payload), PREFIX, PRODUCT, &test_pubkey())
            .expect("带 expires 的有效 key 必须过");
        assert_eq!(p.expires.as_deref(), Some("2027-01-01"));
    }

    #[test]
    fn tampered_payload_fails_signature_not_format() {
        // 翻 payload 段中间一个字符 (保持合法 base64url 字符): 字节变了, 签名必败 ——
        // 且必须报签名错, 不许报格式错 (顺序红线: 先验签再解 JSON)。
        let key = sign(GOOD);
        let mut parts: Vec<String> = key.split('.').map(str::to_owned).collect();
        let mid = parts[1].len() / 2;
        let flipped = if parts[1].as_bytes()[mid] == b'A' {
            'B'
        } else {
            'A'
        };
        parts[1].replace_range(mid..=mid, &flipped.to_string());
        let tampered = parts.join(".");
        assert_eq!(
            verify_key(&tampered, PREFIX, PRODUCT, &test_pubkey()),
            Err(KeyError::BadSignature)
        );
    }

    #[test]
    fn tampered_signature_fails() {
        // 翻签名段**首字符**: 必须落在解码后的字节里 (末字符只有 2 个有效位,
        // 翻它会被 base64 解码头尾位检查拦成格式错 —— 那种报 Malformed 是对的)。
        let key = sign(GOOD);
        let mut parts: Vec<String> = key.split('.').map(str::to_owned).collect();
        let flipped = if parts[2].as_bytes()[0] == b'A' {
            'B'
        } else {
            'A'
        };
        parts[2].replace_range(0..=0, &flipped.to_string());
        let tampered = parts.join(".");
        assert_eq!(
            verify_key(&tampered, PREFIX, PRODUCT, &test_pubkey()),
            Err(KeyError::BadSignature)
        );
    }

    #[test]
    fn key_signed_by_other_key_is_bad_signature_not_malformed() {
        let other = SigningKey::from_bytes(&[9u8; 32]);
        assert_eq!(
            verify_key(&sign_with(GOOD, &other), PREFIX, PRODUCT, &test_pubkey()),
            Err(KeyError::BadSignature)
        );
    }

    #[test]
    fn truncated_key_is_malformed() {
        let key = sign(GOOD);
        let truncated = &key[..key.len() / 2];
        assert_eq!(
            verify_key(truncated, PREFIX, PRODUCT, &test_pubkey()),
            Err(KeyError::Malformed)
        );
    }

    #[test]
    fn wrong_prefix_is_malformed() {
        let key = sign(GOOD).replacen(PREFIX, "otherapp1", 1);
        assert_eq!(
            verify_key(&key, PREFIX, PRODUCT, &test_pubkey()),
            Err(KeyError::Malformed)
        );
    }

    #[test]
    fn wrong_product_is_wrong_product() {
        let key = sign(&GOOD.replace("\"danqing-test\"", "\"danqing-other\""));
        assert_eq!(
            verify_key(&key, PREFIX, PRODUCT, &test_pubkey()),
            Err(KeyError::WrongProduct)
        );
    }

    #[test]
    fn unknown_payload_version_is_malformed() {
        let key = sign(&GOOD.replace("\"v\":1", "\"v\":2"));
        assert_eq!(
            verify_key(&key, PREFIX, PRODUCT, &test_pubkey()),
            Err(KeyError::Malformed)
        );
    }

    #[test]
    fn missing_required_field_is_malformed() {
        let no_email = r#"{"v":1,"product":"danqing-test","tier":"personal","issued_at":"2026-09-19","nonce":"n1"}"#;
        assert_eq!(
            verify_key(&sign(no_email), PREFIX, PRODUCT, &test_pubkey()),
            Err(KeyError::Malformed)
        );
    }

    #[test]
    fn placeholder_zero_pubkey_rejects_any_key() {
        // 收银台未开业的安全默认: 全零公钥下连测试私钥签的 key 都必须败。
        assert_eq!(
            verify_key(&sign(GOOD), PREFIX, PRODUCT, &[0u8; 32]),
            Err(KeyError::BadSignature)
        );
    }

    #[test]
    fn build_payload_roundtrips_through_verify() {
        // keygen 与 verify 共用同一构造器的回归锁: 签发端产出的 key 必须能验回。
        let payload = build_payload(
            PRODUCT,
            Tier::Enterprise,
            "x@y.z",
            "1760000000",
            "deadbeef",
            None,
        );
        let p = verify_key(&sign(&payload), PREFIX, PRODUCT, &test_pubkey())
            .expect("build_payload 产出的 key 必须验回");
        assert_eq!(p.tier, Tier::Enterprise);
        assert_eq!(p.email, "x@y.z");
        assert_eq!(p.nonce, "deadbeef");
    }

    #[test]
    fn build_payload_carries_expires_when_given() {
        let payload = build_payload(
            PRODUCT,
            Tier::Personal,
            "a@b.c",
            "1",
            "n",
            Some("2027-01-01"),
        );
        let p = verify_key(&sign(&payload), PREFIX, PRODUCT, &test_pubkey()).unwrap();
        assert_eq!(p.expires.as_deref(), Some("2027-01-01"));
    }

    #[test]
    fn oversize_key_is_malformed_without_decoding() {
        let huge = format!("{PREFIX}.{}.x", "a".repeat(5000));
        assert_eq!(
            verify_key(&huge, PREFIX, PRODUCT, &test_pubkey()),
            Err(KeyError::Malformed)
        );
    }

    #[test]
    fn payload_debug_masks_email() {
        let p = verify_key(&sign(GOOD), PREFIX, PRODUCT, &test_pubkey()).unwrap();
        let dbg = format!("{p:?}");
        assert!(!dbg.contains("a@b.c"), "Debug 不得含完整邮箱: {dbg}");
        assert!(dbg.contains("a***@b.c"));
    }

    #[test]
    fn empty_and_garbage_inputs_are_malformed() {
        for input in ["", " ", PREFIX, "testapp1.x.y.z", "垃圾", "testapp1.!.!"] {
            assert_eq!(
                verify_key(input, PREFIX, PRODUCT, &test_pubkey()),
                Err(KeyError::Malformed),
                "输入 {input:?} 应为 Malformed"
            );
        }
    }

    #[test]
    fn mask_email_keeps_first_char_and_domain() {
        assert_eq!(mask_email("alice@example.com"), "a***@example.com");
        assert_eq!(mask_email("a@b.c"), "a***@b.c");
    }

    #[test]
    fn mask_email_malformed_input_fully_masked() {
        assert_eq!(mask_email("no-at-sign"), "***");
        assert_eq!(mask_email("@domain.com"), "***");
        assert_eq!(mask_email(""), "***");
    }
}
