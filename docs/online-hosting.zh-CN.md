# Ubuntu 房间部署

需要 Ubuntu 22.04 x86_64、Docker 和 Docker Compose，以及一个可供客户端访问的 TCP 端口。无需域名、证书或数据库。此服务只提供匿名公共房间，不加密传输，不验证成员真实身份。

在源码根目录执行：

```sh
docker compose up -d --build
```

预期：房间容器启动并变为 healthy。

```sh
curl -f http://127.0.0.1:8080/health
```

预期：返回 `ok`。通过服务器防火墙及云平台安全组开放 TCP 8080 后，在 Windows 的“设置 → 联机”输入 `ws://服务器公网IP:8080/ws`，点击“加入”。无需提供服务器登录密码给客户端。

默认主机端口为 8080。如需更换，在源码根目录创建 `.env`，写入 `BONGOCAT_ROOM_PORT=18080`，再运行启动命令；客户端对应使用 `ws://公网IP:18080/ws`。容器内部端口固定 8080，健康检查无需修改。

```sh
docker compose ps
```

预期：显示 healthy 和端口映射。

```sh
docker compose logs --tail=50 room
```

预期：显示启动状态或错误；不会记录按键、坐标或逐条动作。Docker 日志最多保留三个 10 MB 文件。

房间最多八人，包含自己；满员后需要手动重试。客户端遇到网络故障会自动重连，临时 ID 可能改变。服务端重启会清空成员，无需备份房间数据。只有一台 Windows 电脑时，可先运行开发验证命令 `just online-smoke`，它以模拟成员测试七只远端猫；双真实客户端验收仍需两台电脑。

```sh
docker compose restart room
```

预期：客户端断开后重新加入，旧成员和旧动作不会保留。

```sh
docker compose down
```

预期：停止并移除本次房间容器与网络。重新启动仍使用原配置。
