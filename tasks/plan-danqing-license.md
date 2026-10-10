# Plan: danqing-license 落地

- Spec:`danqing/docs/specs/SPEC-danqing-license.md`(2026-10-10 用户批准)
- 来源：复盘 `danqing/docs/intent/retro-trio-reuse-2026-10-10.md` S1
- 状态:**已批准 (2026-10-10, 用户「S1+B 批准」), 成稿于本仓**。T0–T6 已于 2026-10-10 落地(闸 A 通过 + 公开仓已建);T7/T8 消费方迁移未开始。

## 刀序与依赖

```
T0 建仓 → T1 key.rs → T2 entitlement.rs → (T3 store.rs ∥ T4 keygen) → T5 三件套收口
  → 闸A(crate 成稿验收) → T6 远程建仓+push【用户逐项点头:对外动作】
  → T7 dropshelf 迁移 → 闸B(dropshelf 人工验收) → T8 log 迁移 → 闸C(log 人工验收) → T9 收口
```

- T3/T4 互不依赖可并行;T1/T2 顺序(key 的类型是 entitlement 的输入)
- **T7/T8 额外依赖**:对应产品仓的复盘修复批已落地(避免与在途编辑撞车)
- T8 排在 T7 后:一仓一迁移,dropshelf(未发布)先当小白鼠

## 任务清单

- [ ] **T0 建仓(本地)**
  - Acceptance: `cargo new --lib`;`rustup override set stable-x86_64-pc-windows-gnu`;local git 身份核对=十四叔 <gwhun@qq.com>;`.gitignore`(target/);CLAUDE.md/README.md 骨架;spec+plan 从 danqing 仓迁入(`docs/specs/`、`tasks/`);initial commit(中文,注明关联复盘 S1)
  - Files: 新仓 `F:/github/farm01/danqing-license/`
  - 注:新仓 checklist 第 1/2/5 项(打包脚本/logo)N/A——库 crate 不出便携包;第 7 项(feature 评估)N/A——本 crate 自己就是能力方

- [ ] **T1 key.rs**:Payload(遮蔽 Debug)/Tier/KeyError/verify_key/build_payload/mask_email
  - Acceptance: log license.rs:85-176 成稿参数化(prefix/product/pubkey 入参);既有测试(~365 行中的 key 域)参数化迁移逐条存活;「先验签后解 JSON」「4KiB 闸」「全零公钥 fail-closed」锁全带过来
  - Verify: `cargo test key`
  - Files: src/key.rs, src/lib.rs

- [ ] **T2 entitlement.rs**:Entitlement/PaidSource/allows_at/load_from/activate/StoreSnapshot/map_store_snapshot/DURABLE_THRESHOLD_EPOCH + crate 内原子写 helper(~15 行,不依赖 danqing/persist)
  - Acceptance: 语义裁定三条落地(fail-closed 锁测试、activate 原子性 tmp 不残留测试、四态映射测试);Feature 无感化(允许语义 = Paid/Trial 未过期,不认功能枚举)
  - Verify: `cargo test entitlement`
  - Files: src/entitlement.rs

- [ ] **T3 store.rs**(feature `store`):is_packaged(自含两 cfg 变体)/query_snapshot/purchase_full_version/find_main_window/PurchaseOutcome
  - Acceptance: WinRT 互操作编译过+clippy 零警告;映射纯函数留 entitlement.rs;pomodoro 实测坑注释(必须遍历 AddOnLicenses)随迁;模块头标「不可单测区」
  - Verify: `cargo check --features store` + `cargo clippy --features store -- -D warnings`
  - Files: src/store.rs, Cargo.toml

- [ ] **T4 bin/keygen.rs**(feature `keygen`):generate/pubkey/sign 三子命令,prefix/product 走 CLI 参数
  - Acceptance: 端到端测试(generate→pubkey→sign→verify_key 闭环);已存在拒绝覆盖+栈上抹零纪律保留;README 给 log/dropshelf 两产品示例命令
  - Verify: `cargo test --features keygen`
  - Files: src/bin/keygen.rs, README.md

- [ ] **T5 收口**:三件套全绿(fmt/clippy -D warnings/test 含 store/keygen feature 组合);lib.rs 中文文档头;CHANGELOG 骨架
  - Verify: 三件套输出

**闸 A(crate 成稿验收)**:我自查 diff + 测试矩阵,用户点头后进 T6

- [x] **T6 远程建仓【对外动作,逐项点头】** ✅ 2026-10-10:用户明确点头「公开仓 14uncle/danqing-license」→ `gh repo create --public --source=. --remote=origin --push` + topics
  - 结果: `https://github.com/14uncle/danqing-license`(PUBLIC,默认分支 **master** —— 兄弟 crate 单分支惯例,与 encoding/logfile 同;非 dev/master 双分支)
  - 产品仓消费方式=git 依赖钉 rev + 本机 `[patch]` 联动

- [ ] **T7 dropshelf 迁移**:Cargo.toml 加 git dep(features 按需 store)→ 删 license.rs/store_license.rs/bin/keygen.rs(915 行)→ 薄壳(常量+Feature→allows_at 适配+默认 license.key 路径)→ 产品侧只留接线测试
  - Acceptance: 净删 ~750+ 行;三件套绿;门控行为不变(四 Feature 点位原样)
  - Verify: `cargo test` + diff stat
  - lock 手法:直接 `cargo check` 最小重解,**不** `cargo update -p danqing`

**闸 B**:dropshelf 便携版 key 激活人工走通(keygen 签 key→激活→四功能门开)

- [ ] **T8 log 迁移**:同上(1158 行→薄壳);**行为变化进 changelog**:①ExpirationDate fail-open→fail-closed(修收入漏洞)②activate 裸写→原子写;真公钥回填位不变
  - Acceptance: 净删 ~900+ 行;log 既有授权测试已在 crate 存活,产品侧留接线测试;三件套绿
  - Verify: `cargo test` + diff stat

**闸 C**:log 人工验收(激活/七功能门控/升级弹层;商店轨留到开业后验)

- [ ] **T9 收口**:复盘报告 S1 标落地;`danqing/docs/intent/framework-sinking.md` 使用方式节加「下沉验收须含既有消费者迁移」闸(S8 教训);记忆更新;两产品仓 commit(中文、注明关联)**均不 push,push 单独点头**

## 风险与对策

| 风险 | 对策 |
|---|---|
| store.rs WinRT 区不可单测,迁移回归无测试兜 | 四态映射纯函数测试全在 crate;商店轨真机验收留到 log 开业后(现状本就如此,Offer ID 是占位) |
| log 在 v1.x 发布窗口期动授权心脏 | 窗口理由已载 spec:收银台未开业、零付费用户,此刻不动开业后更贵;闸 C 人工验收兜底 |
| keygen 参数化改变既有肌肉记忆命令 | README 双产品示例;旧命令形态在 log/dropshelf 仓 README 同步更新 |
| 产品仓 lock 重解踩 wgpu-hal 边 | 只用 `cargo check` 驱动;已坏时 `cargo update -p gpu-allocator` 外科修复(记忆手法) |
| ed25519-dalek 版本漂移 | 钉 `2`(与两产品现版一致),Cargo.lock 说话 |

## 工作量粗估

T0+T5 半小时级;T1/T2 各 1-2 小时(成稿在前,主要是参数化+测试迁移);T3/T4 各 1 小时;T7/T8 各 1-2 小时(净删改+验收);全程 1 天量级。
