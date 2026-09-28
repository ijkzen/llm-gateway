# 0001 — 模型目录以 vendor 文件形式编译期内嵌

## Status

accepted（「不做运行时更新」条款由 ADR-0025 取代：模型目录仍编译期内嵌为兜底，但匹配优先读外置文件、并由每日任务刷新）

## Context

「智能填充」依赖 models.dev 的全量模型元数据（432 条，minified 约 392KB）。可选途径：运行时从 models.dev 拉取、前端打包、后端内嵌。运行时拉取使功能受制于外部可用性与网络环境（本服务面向内网部署），前端打包则让后端的尾段匹配拿不到数据。

## Decision

models.json 以 minified JSON 文件 vendor 进仓库 `src/provider_model/data/models.json`，通过 `include_str!` 编译期内嵌进二进制，作为零依赖的兜底快照。~~不做运行时更新；目录过期时手动重新下载 minify 后替换该文件并随版本发布。~~ 运行时更新条款由 ADR-0025 取代：匹配优先读「外置模型目录」、每日任务刷新，内嵌文件作为兜底保留。

## Consequences

- 二进制体积增加约 392KB，换取零外部依赖与匹配行为的确定性。
- 目录数据滞后于 models.dev 上游，更新依赖手动替换与发版。
- 替换文件时需保持既有字段结构兼容（顶层 `vendor/model` 键、`limit`、`reasoning`、`tool_call`、`modalities.input`）。
