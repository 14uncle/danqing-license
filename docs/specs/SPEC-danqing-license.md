# SPEC: danqing-license —— 授权基建兄弟 crate

- 来源：三新仓复盘 `danqing/docs/intent/retro-trio-reuse-2026-10-10.md` S1(头号下沉候选,判据四占)
- 形态裁决(2026-10-10 用户按推荐 go):**新兄弟 crate `danqing-license`**,零 UI 依赖,照 danqing-encoding / danqing-logfile 前例
- 状态:**已批准 (2026-10-10, 用户「S1+B 批准」), 成稿于本仓**

## Objective

授权栈现状是全家第三份同构拷贝:pomodoro license.rs(580 行,鼻祖)→ danqing-log license.rs(735)+store_license.rs(246)+bin/keygen.rs(177)→ danqing-dropshelf license.rs(484)+store_license.rs(252)+bin/keygen.rs(179)。`find_main_window` 三仓逐字相同;`verify_key`/`build_payload`/`Payload`/`Entitlement`/`mask_email` 结构一致仅换常量;两仓 keygen 近乎双胞胎。框架无授权模块。

目标:**一份成稿,产品只注入常量**。建 `danqing-license` crate 收编通用授权基建,dropshelf 与 log 迁移;消费方式照家族惯例:git 依赖 + 本机 `[patch]` 联动。

非目标:
- **pomodoro 不动**(已上架,compile-time `full` 双轨变体,迁移风险大于收益;列未来可选项)
- 产品侧 Feature 枚举、门控点位、升级弹层 UI、购买页文案——全部留在产品仓
- 商店轨**更新检查**不归此 crate(那是 danqing update.rs 域的产品侧半轨,复盘 S 外)

## 语义裁定(下沉必须统一的三处分叉)

| # | 分叉 | log 现状 | dropshelf 现状 | **裁定** |
|---|---|---|---|---|
| 1 | 商店 `ExpirationDate` 读失败 | fail-open 朝付费(store_license.rs:103-106 `.ok()`→None→durable→Paid,**收入漏洞**) | fail-closed(:103-110 查询失败=整体 NONE) | **取 fail-closed**,log 迁移时顺带修洞 |
| 2 | `activate` 落盘 | 裸 `fs::write`(license.rs:318) | 原子写(temp+rename) | **取原子写**(crate 自带 ~15 行 helper,不依赖 danqing/persist——兄弟 crate 零 UI 依赖红线,persist 在 UI 框架里) |
| 3 | payload `expires` 字段 | 解析,仅商店侧 Trial 用 | 解析不执行(:58-59) | 保留解析;key 侧 Trial 是否出租 → Open Question 2 |

既有不变语义(两仓一致,crate 继承):先验签后解 JSON(篡改必报 BadSignature);4 KiB 长度闸;load 失败一律 Free 不 panic 不挡免费层;会话内不踢人(entitlement 只升不降);私钥永远仓库外。

## Tech Stack

- Rust edition 2021,windows-gnu 工具链(仓库第一件事 `rustup override set stable-x86_64-pc-windows-gnu`)
- 依赖:`ed25519-dalek` 2(verify_strict)+ `base64` 0.22(URL_SAFE_NO_PAD)+ `serde`/`serde_json`(payload JSON)
- feature `store`(默认关):`windows` 0.61(Services_Store/ApplicationModel/Win32_Foundation/Win32_System_WinRT/Win32_UI_Shell/Win32_UI_WindowsAndMessaging)+ `windows-collections` 0.2
- feature `keygen`(默认关,`[[bin]] required-features`):`getrandom` 0.2
- **零 UI 依赖、零 danqing 依赖**;MSIX 判定需要的 `is_packaged` 在 crate 内自含(framework platform.rs:26/40 同款两 cfg 变体,~20 行,避免反向依赖 UI 框架)

## Commands

```bash
cargo test                    # 全部测试(纯逻辑,store  feature 的 WinRT 互操作区不可单测)
cargo clippy -- -D warnings   # 零警告
cargo fmt
cargo test --features store   # store 映射纯函数测试
cargo run --features keygen --bin keygen -- generate <secret_path>
cargo run --features keygen --bin keygen -- pubkey <secret_path>
cargo run --features keygen --bin keygen -- sign --prefix <P> --product <N> <secret_path> <email> <personal|enterprise>
```

## Project Structure

```
danqing-license/
├── src/
│   ├── lib.rs           # crate 根:re-export + 文档
│   ├── key.rs           # 密钥:Payload/Tier/KeyError/verify_key/build_payload/mask_email
│   ├── entitlement.rs   # 状态机:Entitlement/PaidSource/allows_at/load_from/activate(原子写)/StoreSnapshot/map_store_snapshot/DURABLE_THRESHOLD_EPOCH
│   ├── store.rs         # cfg(feature="store"):WinRT broker 查询/购买拉起/find_main_window/is_packaged
│   └── bin/keygen.rs    # 参数化签发 CLI(prefix/product 走参数,不再硬编码)
├── Cargo.toml
├── CLAUDE.md / README.md
└── tools/               # package_portable.ps1 不需要(库 crate 不出便携包)
```

## API 设计(草案,build 前锁定)

```rust
// key.rs
pub struct Payload { pub tier: Tier, pub email: String, pub issued_at: String,
                     pub nonce: String, pub expires: Option<i64> }  // Debug 遮蔽邮箱
pub enum Tier { Personal, Enterprise }
pub enum KeyError { Malformed, BadSignature, WrongProduct }
pub fn verify_key(key: &str, prefix: &str, product: &str, pubkey: &[u8; 32])
    -> Result<Payload, KeyError>;          // 长度闸→三段切分→验签→解 JSON→版本/product 闸
pub fn build_payload(product: &str, tier: Tier, email: &str, issued_at: i64,
                     nonce: [u8; 8], expires: Option<i64>) -> serde_json::Value;
pub fn mask_email(email: &str) -> String;

// entitlement.rs —— Feature 无感化:crate 不认功能枚举,只回答「此刻是否付费态」
pub enum PaidSource { License(Payload), StoreAddOn }
pub enum Entitlement { Free, Trial { expires_epoch: i64 }, Paid { source: PaidSource } }
impl Entitlement {
    pub fn allows_at(&self, now_epoch: i64) -> bool;  // Paid=true / Trial=now<expires / Free=false
}
pub fn load_from(path: &Path, prefix: &str, product: &str, pubkey: &[u8; 32]) -> Entitlement;
pub fn activate(path: &Path, key: &str, prefix: &str, product: &str, pubkey: &[u8; 32])
    -> Result<Payload, ActivateError>;                 // 先验签,后原子写落盘
pub struct StoreSnapshot { pub addon_active: bool, pub addon_expires_epoch: Option<i64> }
pub const DURABLE_THRESHOLD_EPOCH: i64 = 250_000_000_000;
pub fn map_store_snapshot(s: &StoreSnapshot) -> Entitlement;  // 纯函数,含裁定 1 的 fail-closed

// store.rs(feature 门,WinRT 不可单测区;映射纯函数留在 entitlement.rs)
pub fn is_packaged() -> bool;
pub fn query_snapshot(offer_id: &str) -> StoreSnapshot;      // 非打包→NONE;含 pomodoro 实测坑注释:必须遍历 AddOnLicenses,应用级 IsActive 免费上架恒真
pub enum PurchaseOutcome { Purchased, Cancelled, Failed }
pub fn purchase_full_version(offer_id: &str) -> PurchaseOutcome;  // IInitializeWithWindow 挂主窗口属主
```

**产品侧注入(留在产品仓)**:KEY_PREFIX / PRODUCT / PRODUCT_PUBKEY / FULL_VERSION_OFFER_ID / PURCHASE_URL / Feature 枚举与门控文案 / 许可页 UI。
**产品侧迁移写法**:`log::license` 薄壳保留——常量定义 + `Entitlement::allows_at` 的 Feature 适配(一行委托)+ 默认 license.key 路径。

## Code Style

- 公开 API 一律经 `src/lib.rs` re-export;公共类型/函数中文文档注释
- 新文件头 `//! @author 十四叔` + `//! @date yyyy/MM/dd`
- 测试内置 `#[cfg(test)]`;WinRT 互操作区(store.rs)标「不可单测区」,可测逻辑一律抽纯函数放 entitlement.rs
- 成稿来源以 danqing-log v1.x 版为底(测试最厚),吸收 dropshelf 的两处加严(fail-closed + 原子写)

## Testing Strategy

- log license.rs 既有 ~365 行测试**参数化迁移**进 crate(前缀/产品/公钥改测试常量),逐条存活
- 新增锁:①裁定 1——ExpirationDate 读失败 → Free/Trial 不升 Paid(fail-closed 锁);②裁定 2——activate 落盘原子性(tmp 不残留);③keygen 端到端(generate→pubkey→sign→verify_key 闭环)
- store.rs 四态映射测试沿用(纯函数)
- keygen 已存在拒绝覆盖、栈上抹零纪律保留并加锁

## Boundaries

- **Always**:私钥仓库外(~/.danqing-*-keys/);fail-closed;先验签后解 JSON;load 失败=Free 不挡免费层;三件套全绿才提交
- **Ask first**:crates.io 发布姿态;payload expires 的 key 侧 Trial 出租;对外文档/宣传提及授权机制
- **Never**:不碰 pomodoro;不改产品 Feature 枚举语义与门控点位;不引 UI/网络依赖(windows crate 仅 feature 门);不在 crate 内嵌任何真实公钥/私钥(测试用临时密钥对)

## 迁移计划(build 阶段顺序)

1. 建仓 danqing-license(本地)→ 成稿 + 三件套绿 → **建 GitHub 远程仓(public,与 encoding/logfile 同姿态)并 push**——此步需用户逐项点头(对外动作)
2. dropshelf 迁移:删 license.rs/store_license.rs/bin/keygen.rs(915 行),改薄壳注入常量;既有测试能迁的迁到 crate,产品侧只留接线测试;三件套绿
3. log 迁移:同上(1158 行→薄壳);**行为变化点**:ExpirationDate fail-open→fail-closed(修收入漏洞)、activate 裸写→原子写——进 changelog;真公钥回填位不变;**窗口理由**:log 付费层收银台未开业、商店 Offer ID 尚是占位,此刻迁移零付费用户冲击,开业后再迁移风险翻倍
4. pomodoro:不动,档案注记

## Success Criteria

- 两仓授权代码净删约 1800~2000 行,crate 内成稿 ~700 行(含测试)
- log/dropshelf 既有授权测试全部以 crate 测试形态存活且绿;三仓三件套绿
- keygen 签出的 key 在 dropshelf 与 log 实机激活走通(人工验收,记账)
- log v1.x 付费层发布时授权栈已是 crate 版

## Open Questions

1. **crates.io 发布姿态?** encoding/logfile 是「可独立发布」姿态但未见发布动作;danqing-license 同姿态即可,还是明确不发(授权基建公开仓已够,crates.io 发布是额外宣传面)?
2. **payload `expires` 的 key 侧 Trial 出租?** 现状两仓都只让商店侧产 Trial;key 带 expires 可实现「限期 key」(测评机/推广码场景)。建议本期不出租,字段保留。
3. **Tier 枚举留 crate 还是产品?** 建议留 crate(Personal/Enterprise 结构通用;$29/$59 定价是产品政策,不进 crate)。
4. **crate 内自含 `is_packaged`(~20 行两 cfg 变体)是否与框架 platform.rs 构成新重复?** 判断:兄弟 crate 零 UI 依赖优先于此处去重,自含;若将来平台判定逻辑变复杂,再议。
