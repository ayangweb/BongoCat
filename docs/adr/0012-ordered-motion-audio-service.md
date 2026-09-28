# ADR-0012: Ordered Motion Audio Service

状态：Accepted
日期：2026-08-31

## Context

预置 model3 的 motion 通过相对路径引用 FLAC。音效是动作被接受后的产品副作用，
不能进入 `RenderSnapshot`、阻塞 runtime/input，也不能因文件、解码或输出设备故障让
动作或画面失败。旧产品同一时刻只保留一个 motion voice；新动作、显式停止、禁用、
成功切模和退出都需要明确停止边界。

## Decision

- 使用 crates.io 当日最新稳定版 `rodio 0.22.2`，精确 pin，只启用 `playback` 与 `flac`。
  不启用录音、MP3、MP4、Vorbis、WAV、dither 或其它未使用 feature。
- `bongocat-audio` 是独立 Rust owner。它持有固定容量 16 的有序 command queue、专用
  worker、decoder、output stream 和唯一 voice；runtime 只非阻塞发布强类型
  `Play`/`Stop`，不执行文件、解码或设备工作。
- motion request 只有在 runtime priority 与 Live2D resource 校验均接受后才触发音效。
  新 motion 替换当前 voice；无声音的新 motion 也停止旧 voice。显式 motion stop、关闭
  配置、成功 model commit 和 shutdown 立即停止 voice，不等待 motion fade 结束。
- 模型 activation 在 renderer prepare 成功后，把该模型的去重 sound paths 作为 `Prepare` 发送给
  audio worker。worker 在这里解码 FLAC 为 immutable PCM cache；renderer prepare 与 PCM prepare 都完成后
  才提交该模型。预热失败只形成稳定 audio 诊断，仍允许模型提交；随后 play 会按既有降级语义失败，不能阻塞
  动作或渲染。成功 commit 后 `ActivatePrepared` 只保留新活动模型的 cache。
- 已活动模型的 motion 在同一 runtime command 中先向有序队列发布缓存 `Play`，再以同一单调 tick
  启动 motion。动作路径不执行文件 I/O、解码、设备创建、`get_pos()` 等待或 1 ms 轮询。audio owner
  优先以 512-frame buffer 打开默认设备，设备拒绝时回退 rodio 默认 sink。
- 音量使用经过校验的 `[0, 1]` 强类型值，当前产品触发值为 `1.0`。同一时刻最多一个
  motion voice，不混音、不排队延后播放。
- 队列满载不阻塞 runtime：返回原 command、增加匿名 overflow 计数、丢弃无法证明
  顺序的 backlog 并强制停止 voice。文件 I/O、解码和输出故障进入稳定 error code 与
  聚合诊断，后续 command 可以恢复服务；诊断不包含用户路径或音频内容。
- 应用先停止输入与 runtime，再停止并 join 音频 worker；worker 在返回 `Stopped` 前
  停止 voice、丢弃 pending command 并释放 rodio/CPAL output owner。

## Dependency Boundary

`rodio 0.22.2` 声明 `MIT OR Apache-2.0`，Rust 1.87+，仓库在审计日仍活跃；底层 CPAL
支持 Windows/macOS 系统输出。最新独立 CPAL 是 `0.18.2`，但 rodio 0.22.2 约束
`cpal 0.17.x`，因此 lockfile 合法解析为 `0.17.3`；项目不直接依赖或 fork CPAL 来伪造
版本升级。

rodio/CPAL 类型不离开 `bongocat-audio` 私有 backend。替换其它音频库时只需保持
`MotionAudioClient`、强类型 command、诊断和 shutdown contract，不修改 runtime、模型
或 UI 公共协议。Linux 首发后评估时再启用对应 output target；当前 Linux contract build
不链接 ALSA，也不把 Linux 音频变成 Windows/macOS 首发阻塞。

## Consequences

音频解码成本转移到模型 activation；设备打开继续由 audio owner 惰性执行，但 runtime 不等待它。
`rodio::Player::get_pos()` 只反映 mixer 位置，不能作为扬声器起播确认，因此不再用作动画时钟。当前实现支持现有预置模型实际使用的 FLAC；
新增格式必须先形成模型兼容需求，再单独启用 decoder feature 和测试。默认设备热切换、运行中 stream
error callback、端到端音频/画面时序测量、100 次 model/audio 资源测量与 8 小时 soak 仍属于后续
稳定性验收，不由本 ADR 的单元测试替代。
