# Project: danqing-license

丹青产品线授权基建 —— 兄弟 crate(零 UI 依赖,照 `danqing-encoding` / `danqing-logfile` 前例)。
收编此前三仓同构拷贝的授权栈(pomodoro → danqing-log → danqing-dropshelf):
便携版 license key 离线 Ed25519 校验 + 商店 add-on 授权状态映射。

来源:三新仓复用复盘 `../danqing/docs/intent/retro-trio-reuse-2026-10-10.md` S1(头号下沉候选,判据四占)。

## 状态

- 2026-10-10:建仓 + 成稿(plan T0–T5),**闸 A 通过**(用户点头),建 GitHub 公仓并 push。
  三件套全绿(clippy `--all-features` 0 警告 / 39 测试)。消费方迁移(dropshelf / log)见 plan T7/T8,**尚未开始**。

## 必读

- spec(语义裁定/API/边界):`docs/specs/SPEC-danqing-license.md`
- plan(刀序/闸门/风险):`tasks/plan-danqing-license.md`
- 农场跨仓约定:`../CLAUDE.md`

## 仓库与分支

- 分支:`master`(兄弟 crate 单一分支,与 danqing-encoding / danqing-logfile 同)
- 本地 git 身份:十四叔 <gwhun@qq.com>
- 远程:`git@github.com:14uncle/danqing-license.git`(**公开仓**,2026-10-10 建)

## Tech Stack

- Rust,edition 2024(工具链 `stable-x86_64-pc-windows-gnu`,rustup override 已设)
- 依赖:`ed25519-dalek 2` / `base64 0.22` / `serde_json 1` / `log 0.4`
- feature `store`(默认关):`windows 0.61` + `windows-collections 0.2` + `windows-sys 0.59`(**仅 Windows**)
- feature `keygen`(默认关,`[[bin]] required-features`):`getrandom 0.2`
- **零 UI 依赖、零 danqing 依赖**

## Commands

```bash
cargo test                              # 纯逻辑全测
cargo test --features keygen            # 含签发 CLI
cargo test --features store             # 含 WinRT 互操作区(仅 Windows)
cargo clippy --all-targets -- -D warnings
cargo fmt
# 签发(私钥仓库外):
cargo run --features keygen --bin keygen -- generate ~/.danqing-x-keys/x.secret
cargo run --features keygen --bin keygen -- pubkey   ~/.danqing-x-keys/x.secret
cargo run --features keygen --bin keygen -- sign --prefix <P> --product <N> ~/.danqing-x-keys/x.secret <email> personal
```

## Boundaries

- 注释/文档一律中文;新 `.rs` 文件头 `//! @author 十四叔` + `//! @date yyyy/MM/dd`
- 公开 API 一律经 `src/lib.rs` re-export
- **Always**:私钥仓库外(`~/.danqing-*-keys/`);fail-closed;先验签后解 JSON;load 失败=Free 不挡免费层;三件套全绿
- **Never**:不碰 pomodoro;不改产品 `Feature` 枚举语义与门控点位;不引 UI/网络依赖;
  crate 内不嵌任何真实公钥/私钥(测试一律临时密钥对)
- **Ask first**:crates.io 发布姿态;payload `expires` 的 key 侧 Trial 出租;对外动作(建远程仓/push)

## 结构

```
src/lib.rs           crate 根: re-export + 语义红线文档
src/key.rs           key 串解析与验签: Payload/Tier/KeyError/verify_key/build_payload/mask_email
src/entitlement.rs   状态机与持久化: Entitlement/allows_at/load_from/activate(原子写)/
                     StoreSnapshot/snapshot_from_readings(fail-closed 落点)/map_store_snapshot
src/store.rs         cfg(feature="store"): WinRT broker 查询/购买/is_packaged/find_main_window
src/bin/keygen.rs    cfg(feature="keygen"): 参数化签发 CLI(prefix/product 走参数)
```
