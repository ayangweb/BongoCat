# Shared Behavior Fixtures

本目录保存平台无关的行为输入和规范化结果。Fixture 是产品协议，不绑定 GPUI、系统 API 或 renderer。

```text
fixtures/
  input-sequences/
    schema.json
    *.json
  expected-state/
    schema.json
    *.json
  model-fixtures/
    README.md
    cases.json
    cases/
    preset-models.json
```

## 约定

- `schemaVersion` 从 1 开始；破坏性变更必须递增。
- `atMs` 是序列开始后的单调相对时间。
- 相同 `atMs` 的事件按数组顺序处理。
- `key_down/up` 使用稳定物理键名，不使用本地化字符。
- Expected snapshot 中所有集合按 UTF-8 字典序排序。
- 浮点值在比较前按 runner 规定的精度规范化；v1 使用小数点后 6 位。
- Expected snapshot 必须声明 `provenance`：`legacy_observation`、`product_decision` 或 `bug_fix`；golden 更新不得没有来源。
- Fixture 不包含绝对路径、用户模型 id、真实快捷键或个人数据。

## Runner 责任

未来 Rust fixture runner 必须：

1. 校验 input 和 expected schema；
2. 使用可注入时钟按顺序执行事件；
3. 在 checkpoint 生成规范化 snapshot；
4. 输出字段级差异；
5. 在 Windows/macOS 对相同 fixture 给出相同业务结果。

Platform adapter 的 scan code、系统权限和原始消息另设平台 fixture，不混入本目录。动画和模型 command 的优先级、切换清理和音效顺序见 `shared/behavior/animation-semantics.md`。

运行不依赖第三方包的跨文件检查：

```text
python3 tools/validate-fixtures.py
python3 tools/run-input-fixtures.py
```

`validate-fixtures.py` 检查 input/expected 配对、id、事件时间顺序和 checkpoint 对应关系；同时把合成模型包复制到临时目录，验证 package discovery、JSON 解析、引用路径和纹理头限制。`run-input-fixtures.py` 使用无平台依赖的确定性协议模型执行输入事件，并逐 checkpoint 比较 pressed state、设备、鼠标、光标、hand parameter 和动画/model 状态；它不是产品 runtime，也不能替代 Windows/macOS 平台采集测试。音效事件只验证为有序、非阻塞的协议输入，不会在 expected snapshot 中伪造音频设备结果。模型 fixture 只定义导入 preflight 行为，不包含可供 Cubism Core 加载的数据，也不替代运行时兼容测试。JSON Schema Draft 2020-12 校验仍应由 CI 中固定版本的标准 validator 执行。
