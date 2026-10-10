//! @author 十四叔
//! @date 2026/10/10
//!
//! keygen —— license key 签发工具 (feature `keygen`)。
//!
//! **私钥永不入库**: 私钥文件 (32 字节, hex 编码) 只存在仓库外, 由命令行路径指定;
//! 本工具本身入库可审计。`prefix` / `product` 是产品注入项, 与产品侧常量一致 ——
//! 一次工具签发全家产品的 key。
//!
//! 用法:
//!   keygen generate <secret_path>                          生成新私钥 (已存在则拒绝覆盖)
//!   keygen pubkey  <secret_path>                           打印公钥 (hex + Rust 数组字面量)
//!   keygen sign --prefix <P> --product <N> <secret_path> <email> <personal|enterprise>
//!
//! 示例 (产品侧常量见各自 `license.rs`):
//!   keygen sign --prefix loglens1 --product danqing-log       ~/.danqing-log-keys/danqing-log.secret alice@example.com personal
//!   keygen sign --prefix shelf1   --product danqing-dropshelf ~/.danqing-shelf-keys/danqing-dropshelf.secret alice@example.com personal

use std::fs;
use std::path::Path;
use std::process::ExitCode;
use std::time::{SystemTime, UNIX_EPOCH};

use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use danqing_license::{Tier, build_payload};
use ed25519_dalek::{Signer, SigningKey};

const USAGE: &str = "keygen generate <secret_path> | keygen pubkey <secret_path> | \
     keygen sign --prefix <P> --product <N> <secret_path> <email> <personal|enterprise>";

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("keygen: {e}");
            ExitCode::FAILURE
        }
    }
}

fn run() -> Result<(), String> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match args.first().map(String::as_str) {
        Some("generate") => {
            let path = arg(&args, 1, "generate <secret_path>")?;
            generate(Path::new(path))
        }
        Some("pubkey") => {
            let path = arg(&args, 1, "pubkey <secret_path>")?;
            let sk = read_secret(Path::new(path))?;
            let pk = sk.verifying_key();
            // 两种形态都给: hex 供肉眼/文档, Rust 数组字面量直接可贴进产品侧
            // 的 `PRODUCT_PUBKEY` (消掉手工转换 64 个 hex 字节的手滑点)。
            println!("hex:   {}", hex_encode(&pk.to_bytes()));
            println!("rust:  {:?}", pk.to_bytes());
            Ok(())
        }
        Some("sign") => sign_cmd(&args),
        _ => Err(format!("用法: {USAGE}")),
    }
}

fn sign_cmd(args: &[String]) -> Result<(), String> {
    let mut prefix: Option<String> = None;
    let mut product: Option<String> = None;
    let mut pos: Vec<&str> = Vec::new();
    let mut it = args.iter().skip(1);
    while let Some(a) = it.next() {
        match a.as_str() {
            "--prefix" => prefix = Some(next_value(&mut it, "--prefix")?),
            "--product" => product = Some(next_value(&mut it, "--product")?),
            other => pos.push(other),
        }
    }
    let prefix = prefix.ok_or_else(|| format!("缺 --prefix, 用法: {USAGE}"))?;
    let product = product.ok_or_else(|| format!("缺 --product, 用法: {USAGE}"))?;
    let path = pos
        .first()
        .ok_or_else(|| format!("缺 <secret_path>, 用法: {USAGE}"))?;
    let email = pos
        .get(1)
        .ok_or_else(|| format!("缺 <email>, 用法: {USAGE}"))?;
    let tier = match pos.get(2).copied() {
        Some("personal") => Tier::Personal,
        Some("enterprise") => Tier::Enterprise,
        other => {
            return Err(format!(
                "tier 只能是 personal | enterprise, 收到: {}",
                other.unwrap_or("(缺)")
            ));
        }
    };
    let sk = read_secret(Path::new(path))?;
    println!("{}", sign_key(&sk, &prefix, &product, email, tier)?);
    Ok(())
}

fn next_value<'a>(it: &mut impl Iterator<Item = &'a String>, flag: &str) -> Result<String, String> {
    it.next().cloned().ok_or_else(|| format!("{flag} 缺值"))
}

fn arg<'a>(args: &'a [String], i: usize, usage: &str) -> Result<&'a str, String> {
    args.get(i)
        .map(String::as_str)
        .ok_or_else(|| format!("缺参数, 用法: keygen {usage}"))
}

/// 生成新私钥。**已存在则拒绝覆盖** —— 覆盖私钥 = 已签发的全部 key 失去签发者。
fn generate(path: &Path) -> Result<(), String> {
    if path.exists() {
        return Err(format!(
            "{} 已存在, 不覆盖 (要换密钥对请先手动挪走旧文件)",
            path.display()
        ));
    }
    let mut secret = [0u8; 32];
    getrandom::getrandom(&mut secret).map_err(|e| format!("取操作系统随机数失败: {e}"))?;
    fs::write(path, hex_encode(&secret) + "\n")
        .map_err(|e| format!("写入 {}: {e}", path.display()))?;
    println!("私钥已写入 {} —— 保管好, 别进任何仓库", path.display());
    Ok(())
}

fn read_secret(path: &Path) -> Result<SigningKey, String> {
    let text = fs::read_to_string(path)
        .map_err(|e| format!("读私钥文件 {} 失败 (路径应指向仓库外): {e}", path.display()))?;
    let bytes = hex_decode(text.trim()).map_err(|e| format!("私钥文件内容不是合法 hex: {e}"))?;
    let arr: [u8; 32] = bytes
        .try_into()
        .map_err(|_| "私钥必须是 32 字节 (64 个 hex 字符)".to_string())?;
    Ok(SigningKey::from_bytes(&arr))
}

fn sign_key(
    sk: &SigningKey,
    prefix: &str,
    product: &str,
    email: &str,
    tier: Tier,
) -> Result<String, String> {
    let mut nonce = [0u8; 8];
    getrandom::getrandom(&mut nonce).map_err(|e| format!("取操作系统随机数失败: {e}"))?;
    let issued_at = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|_| "系统时钟早于 1970?".to_string())?
        .as_secs()
        .to_string();
    let payload = build_payload(product, tier, email, &issued_at, &hex_encode(&nonce), None);
    let sig = sk.sign(payload.as_bytes());
    Ok(format!(
        "{prefix}.{}.{}",
        URL_SAFE_NO_PAD.encode(payload.as_bytes()),
        URL_SAFE_NO_PAD.encode(sig.to_bytes())
    ))
}

fn hex_encode(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

fn hex_decode(s: &str) -> Result<Vec<u8>, String> {
    if s.len() % 2 != 0 {
        return Err(format!("hex 长度必须是偶数, 收到 {}", s.len()));
    }
    (0..s.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&s[i..i + 2], 16).map_err(|e| format!("偏移 {i}: {e}")))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    const PREFIX: &str = "testapp1";
    const PRODUCT: &str = "danqing-test";

    #[test]
    fn hex_roundtrip() {
        let bytes = [0x00u8, 0xff, 0x07, 0xab];
        assert_eq!(hex_decode(&hex_encode(&bytes)).unwrap(), bytes);
    }

    #[test]
    fn hex_decode_rejects_odd_length_and_bad_chars() {
        assert!(hex_decode("abc").is_err());
        assert!(hex_decode("zz").is_err());
        assert!(hex_decode("").is_ok_and(|v| v.is_empty()));
    }

    #[test]
    fn generate_refuses_to_overwrite_existing_secret() {
        let path = std::env::temp_dir().join(format!(
            "danqing-license-keygen-{}-exists.secret",
            std::process::id()
        ));
        fs::write(&path, "deadbeef").unwrap();
        let err = generate(&path).unwrap_err();
        assert!(err.contains("已存在"), "应拒绝覆盖: {err}");
        let _ = fs::remove_file(&path);
    }

    #[test]
    fn sign_produces_key_that_verifies() {
        // 端到端: 临时私钥文件 → read_secret → sign_key → verify_key 验回。
        // 临时文件名带进程 id, 不与任何其他测试共享 (并行 flake 教训在档)。
        let path = std::env::temp_dir().join(format!(
            "danqing-license-keygen-{}-e2e.secret",
            std::process::id()
        ));
        let secret = [3u8; 32];
        fs::write(&path, hex_encode(&secret)).unwrap();
        let sk = read_secret(&path).unwrap();
        let pubkey = sk.verifying_key().to_bytes();
        let key = sign_key(&sk, PREFIX, PRODUCT, "e2e@test", Tier::Personal).unwrap();
        let payload = danqing_license::verify_key(&key, PREFIX, PRODUCT, &pubkey)
            .expect("keygen 签的 key 必须验回");
        assert_eq!(payload.email, "e2e@test");
        assert_eq!(payload.tier, Tier::Personal);
        // 换个产品名就验不过 (product 是签名覆盖的一部分)
        assert!(danqing_license::verify_key(&key, PREFIX, "danqing-other", &pubkey).is_err());
        let _ = fs::remove_file(&path);
    }
}
