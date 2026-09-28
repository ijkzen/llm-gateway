# 01: 外置目录读取（catalog 改造）

**What to build:** 匹配模型时优先使用「外置模型目录」文件——位于 db 目录父目录下的 `models.json`（由 `DATABASE_URL` 推导）；每次读取按 mtime/size 判断是否需要重新解析，未变更复用缓存；外置文件缺失、读不到或解析失败时回退编译期内嵌快照。路径推导复用 `DATABASE_URL → 文件路径` 的既有解析口径（提取为公共 helper，单一事实源）。

**Blocked by:** None (can start immediately).

**Status:** ready-for-agent

- [ ] `DATABASE_URL`（`sqlite::memory:` / `sqlite://rel` / `sqlite:///abs`）到数据库文件路径的解析提取为公共 helper，`ensure_sqlite_dir` 改为复用它，行为不变。
- [ ] 由数据库文件路径推导外置目录文件：取 db 目录的父目录拼 `models.json`；dev 得工作目录下 `models.json`，prod 得 `/config/models.json`；内存库无外置路径。
- [ ] `catalog` 支持可注入外置路径的实例（读取 API 保持名字与语义），进程级全局实例由启动期注入；未注入时等同内嵌。
- [ ] 外置文件存在且合法时，`find_by_suffix` / `similar_entries` / `search` / `entry_count` 反映外置内容。
- [ ] 外置文件内容变更（mtime/size 变）后无需重启即返回新内容。
- [ ] 外置文件缺失、读不到、非法 JSON 时回退内嵌内容且不报错；同一坏文件不重复读取（键未变则复用兜底结果）。
- [ ] 返回类型由 `&'static` 改为拥有值，调用点（候选刷新）随之适配。
- [ ] 单元测试覆盖：外置优先、变更重解析、删除回退、非法回退、无外置路径；路径推导 dev/prod/内存三例。

**Acceptance criteria 验证方式:** `cargo test`（catalog 与 db 单元测试全绿）。
