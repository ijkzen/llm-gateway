# 04: prod compose 整目录挂载

**What to build:** 生产 compose 的 volumes 由两条子路径挂载（`./data/db:/config/db`、`./data/logs:/config/logs`）改为单条整目录挂载 `./data:/config`，使外置模型目录 `data/models.json` 与数据库、日志一同持久化，容器重建不丢失；host 目录布局保持 `data/db`、`data/logs`、`data/models.json`。

**Blocked by:** None (can start immediately).

**Status:** ready-for-agent

- [ ] `deploy/docker-compose.yaml` volumes 改为 `./data:/config`，db/logs 路径不变。
- [ ] 顶部目录布局注释补充 `data/models.json`。
- [ ] 不修改 `deploy.sh` 的 SSH 主机/远端路径/域名；备注需在部署机手动同步一次 compose 文件。

**Acceptance criteria 验证方式:** compose 语法有效（`docker compose config` 或在部署机同步后容器健康且 `/config/models.json` 落在 `data/` 下）。
