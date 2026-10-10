# danqing-license

丹青产品线的**授权基建**(兄弟 crate,零 UI 依赖)。

收编此前在三个产品里各自拷贝的同一套授权栈,产品侧只注入常量:

- **便携版**:license key 离线 Ed25519 签名校验(零网络请求),key 串格式
  `<prefix>.<payload_b64url>.<sig_b64url>`
- **商店版**:MSIX add-on 授权快照查询 + 购买对话框拉起(`feature = "store"`)
- **签发**:参数化 keygen CLI(`feature = "keygen"`),私钥始终在仓库外

## 产品侧只需注入

| 常量 | 说明 |
|---|---|
| `KEY_PREFIX` | key 串前缀,如 `loglens1` / `shelf1` |
| `PRODUCT` | payload 里的产品名,别的产品线的 key 验不过 |
| `PRODUCT_PUBKEY` | 产品公钥(`[u8; 32]`,由 keygen `pubkey` 产出),全零 = 收银台未开业(fail-closed) |
| `FULL_VERSION_OFFER_ID` | 商店 add-on 的 Offer ID(`feature = "store"`) |
| `Feature` 枚举 + 门控点位 | **留在产品侧** —— 本 crate 只回答「此刻是否付费态」 |

## 用法

```rust
// 启动加载
let ent = danqing_license::load_from(&path, KEY_PREFIX, PRODUCT, &PRODUCT_PUBKEY);

// 门控(产品侧一行适配自己的 Feature 枚举)
fn allows(&self, _f: Feature) -> bool {
    self.entitlement.allows_at(now_epoch())
}

// 激活(校验通过 → 原子落盘)
danqing_license::activate(&path, key, KEY_PREFIX, PRODUCT, &PRODUCT_PUBKEY)?;

// 商店轨(feature = "store")
let snap = danqing_license::store::query_snapshot(FULL_VERSION_OFFER_ID);
let ent  = danqing_license::map_store_snapshot(&snap);
```

## 签发

```bash
cargo run --features keygen --bin keygen -- generate ~/.danqing-log-keys/danqing-log.secret
cargo run --features keygen --bin keygen -- pubkey   ~/.danqing-log-keys/danqing-log.secret
cargo run --features keygen --bin keygen -- sign --prefix loglens1 --product danqing-log \
    ~/.danqing-log-keys/danqing-log.secret buyer@example.com personal
```

**私钥永不入库** —— 只存在仓库外,由命令行路径传入。

## 语义红线

- 先验签后解 JSON(篡改必报签名错)
- 加载失败一律按免费层,绝不 panic、绝不挡免费层
- 商店 `ExpirationDate` 读失败 → fail-closed(不升级为付费)
- `activate` 原子写(半截 key 文件会白丢一次激活)

## License

MIT OR Apache-2.0
