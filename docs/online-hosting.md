# Hosting a room on Ubuntu

Use Ubuntu 22.04 x86_64 with Docker and Docker Compose, and expose one TCP port. No domain, certificate, or database is needed. The public room is anonymous: transport is unencrypted and displayed IDs do not authenticate people.

Run from the source root:

```sh
docker compose up -d --build
```

Expected: the room container starts and becomes healthy.

```sh
curl -f http://127.0.0.1:8080/health
```

Expected: `ok`. Allow TCP 8080 in the host firewall and your cloud security group. On Windows, open **Settings → Online**, enter `ws://YOUR_PUBLIC_IP:8080/ws`, and click **Join**. Clients do not need the server's login credentials.

To change the host port, create `.env` in the source root with `BONGOCAT_ROOM_PORT=18080`, then run the startup command again. Clients use `ws://YOUR_PUBLIC_IP:18080/ws`. The container port remains 8080, so its health check does not change.

```sh
docker compose ps
```

Expected: healthy status and the published port.

```sh
docker compose logs --tail=50 room
```

Expected: startup status or errors, without keys, coordinates, or per-action logs. Docker retains at most three 10 MB log files.

The room supports eight people including yourself. A full room requires manual retry. Network failures reconnect automatically and may change the temporary ID. Restarting the server clears room membership; there is no room database to back up. With one Windows computer, developers can run `just online-smoke` to test seven remote cats with simulated members. Two real clients still require two computers for acceptance testing.

```sh
docker compose restart room
```

Expected: clients disconnect and rejoin without retaining old members or actions.

```sh
docker compose down
```

Expected: stop and remove the room container and network. Starting again uses the same configuration.
