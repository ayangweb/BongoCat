# ADR-0065: 模型模式元数据与卡片 Badge

状态：已接受（2026-09-25）
依赖：ADR-0037（在应用内导入 BongoCatMver 模型）、ADR-0047（模型元数据编辑边界）、ADR-0059（UI protocol 边界）

## 背景

模型管理页需要让用户直接看出每个模型属于标准、键盘还是手柄模式。模式必须跨重启稳定，不能只从标题或目录名临时推断。

普通 BongoCat 模型包没有独立的模式字段，但产品现有的键图资源约定提供了可验证的形状：

- `resources/left-keys` 的 PNG 键图是标准模式的基础；
- `resources/right-keys` 的 PNG 键图表示额外的右手输入资源；
- 手柄模式使用 `DPad*`、`*Trigger*`、`South/East/West/North`、`Start/Select` 等手柄专用键名。

这些是**导入时的分类规则**，不是运行时每次渲染都重新执行的猜测。分类在模型 staging 通过完整包校验后、原子提交前完成；没有任何可用键图、无法归入三类资源形状的普通包直接拒绝，不写入 store，也不写入配置。

BongoCatMver 是例外：它有明确的 legacy `standard`、`keyboard`、`gamepad` source section，所选 section 比转换后目录形状更权威。构建预置模型则以稳定 id `standard`、`keyboard`、`gamepad` 标识自身模式。

## 决策

### 1. installed 模型元数据持久化 `input_mode`

当前 v1 的 `model.installed_models[]` 每条记录包含：

```json
{
  "id": "model-storage-id",
  "title": "User-facing title",
  "input_mode": "standard"
}
```

`input_mode` 是必填的 `standard`、`keyboard` 或 `gamepad`。它与标题一起在导入事务完成后写入 `config.json`，重启时从配置读取。模型改名只修改 `title`，必须保留原 `input_mode`；删除模型随整条元数据删除。

`model.preset_models[]` 继续只保存 `id` 与 `title`。预置模式由构建拥有的稳定 id 派生，不在用户配置里重复保存。

`input_mode` 不加入 `(origin, model_id)` 模型身份，不新增模式编辑 command，也不从显示标题反解析。

### 2. 导入时判定普通包模式

`bongocat-model-store` 在普通包复制、键名归一化和第二次 `PreparedModel` 校验之后扫描 staging 树，使用与 runtime 键图读取相同的直接 `.png`/文件名语义：

1. 任一手存在手柄专用键名 → `gamepad`；
2. 否则任一手存在 `right-keys` → `keyboard`；
3. 否则存在 `left-keys` → `standard`；
4. 两者都不存在 → `InvalidPackage`，导入事务失败。

判定结果和模型一起从 store API 返回，Application 在同一导入循环中写入 metadata。模式判定失败时，用户源目录、store 目的地、staging 和 config 都保持不变。`right-keys` 单独存在即可判定 keyboard；手柄判定不只依赖 `East`，还检查完整的预置/转换手柄词汇子集。

Mver 转换不重新用上述启发式覆盖 source section，而是把用户选择的 legacy mode 传入同一提交尾部。预置模型也不从资源目录反推，直接由稳定 preset id 映射。

### 3. settings snapshot 投影强类型模式

`bongocat-ui-protocol` 独立拥有 `SettingsModelMode`。Application 把 config/model 值穷尽映射为该协议类型，并把它放进 `SettingsModelEntry.input_mode`。正常导入的模型一定有该值；只有手工放入 store 且无法通过包校验的异常目录才会没有模式 Badge。

GPUI 页面只消费 snapshot，不读取 config、不扫描模型目录，也不自行解释 id 或标题。

配置、model-store、UI protocol 各自保留自己的稳定枚举，避免跨层依赖或第三方类型泄漏。Application 的映射必须覆盖全部枚举值。

### 4. 卡片右上角使用无图标、中性色的 GPUI Kit 标记

固定 GPUI Kit revision 的 `Badge` 原生支持数字、圆点和图标，不是文字标签 primitive。Models 页面保留 `Badge` 作为封面覆盖层容器，但不再使用它的 icon/number overlay；可见内容改为与原实现一致的 compact `secondary` `Tag`：

- 标准、键盘、手柄使用相同的中性主题色；
- 模式差异由本地化文字表达，不增加彩色背景噪声。

标记定位在封面右上角，不新增卡片行，也不改变封面、标题编辑面或卡片高度。模式文字始终可见，标记是只读展示，不加入 Tab 顺序。

### 5. 本 ADR 不改变 runtime 输入绑定

`input_mode` 在本阶段是导入来源/模型类别元数据，用于配置完整性、快照和模型卡片展示。它不在本 ADR 中切换 runtime 的 key/gamepad binding；现有 installed 模型默认键盘绑定及预置 gamepad 特例仍按其独立任务维护。

因此“卡片显示 Gamepad”表示该模型来自或被构建标识为 gamepad 模式，不等于本 ADR 已完成 installed Mver gamepad 的 runtime 输入接线。未来若让该字段驱动输入行为，必须单独修改 runtime/overlay contract、迁移既有绑定测试并完成双平台手柄验证，不能在纯 UI 改动中静默发生。

### 6. 当前 v1 直接增加必填字段

`next` 尚未发布完整 v1，因此不增加 schema 版本、migration、alias 或旧字段 fallback。缺少 `input_mode` 的 installed metadata、未知枚举值或其它旧开发结构都不是当前完整 v1，按现有严格配置恢复边界处理。Development 与 Production 使用相同结构、不同存储根。

## 结果

- 普通模型不会以“未分类”进入 store 或 config；无法判定就是导入失败。
- Mver 三种转换模式在配置、快照与卡片之间有强类型闭环。
- 普通包的标题、目录名和具体美术文件不会改变已保存的模式；改名、删图、目录变化和重启不会重新推断。
- 预置模式不与用户可编辑标题或封面元数据混在一起。
- UI 使用官方 GPUI Kit 组件、主题色和本地化可见文案，没有自绘状态；模式标记无图标、位于封面右上，并保持中性 `secondary` 颜色。

## 未验证 / 后续

- Windows/macOS 设置窗口中右上角模式标记的真实观感、Retina/DPI 和长本地化文本仍需平台 smoke 与截图检查。
- installed Mver gamepad 的 runtime 输入接线不属于本 ADR；如产品要让模式 Badge 同时代表实际输入行为，需要独立任务。
